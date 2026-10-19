use crate::analysis;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

const MESSAGE_SIZE_BYTES_MAX: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub enum Query {
    Check,
    Complete(usize),
    Hover(usize),
    Definition(usize),
}
#[derive(Debug)]
pub struct Completion {
    pub label: String,
    pub detail: String,
    pub replacement: std::ops::Range<usize>,
    pub text: String,
}
#[derive(Clone, Debug)]
pub struct Location {
    pub path: PathBuf,
    pub start: (usize, usize),
    pub end: (usize, usize),
}
impl Location {
    pub fn range(&self, source: &ropey::Rope) -> std::ops::Range<usize> {
        let start = from_position(source, self.start.0, self.start.1);
        start..from_position(source, self.end.0, self.end.1).max(start)
    }
}
#[derive(Debug)]
pub enum Insight {
    Completions(Vec<Completion>),
    Hover(String),
    Definitions(Vec<Location>),
}
pub struct Checked {
    pub report: analysis::Report,
    pub insight: Option<Insight>,
}

pub struct Client {
    child: Arc<Mutex<Option<Child>>>,
    input: ChildStdin,
    messages: mpsc::Receiver<Result<Value, String>>,
    reader: Option<std::thread::JoinHandle<()>>,
    directory: PathBuf,
    uri: String,
    request_id: u32,
    version: i32,
    revision: u64,
    configuration: Value,
    directive: Option<analysis::Diagnostic>,
}

impl Client {
    pub fn new(
        path: &std::path::Path,
        cancelled: &impl Fn() -> bool,
        process: Arc<Mutex<Option<Child>>>,
    ) -> Result<Self, String> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "spice-cad-lsp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).map_err(|error| error.to_string())?;
        let configuration = json!({"platform":{"type":"standard"},"sourcemap":{"enabled":false,"autogenerate":false},"index":{"enabled":false},"diagnostics":{"workspace":false},"fflags":{"sync":false}});
        let started = (|| {
            let absolute = if path.is_absolute() {
                path.to_owned()
            } else {
                std::env::current_dir()
                    .map_err(|error| error.to_string())?
                    .join(path)
            };
            let uri = url::Url::from_file_path(&absolute)
                .map_err(|_| "Cannot encode model path")?
                .to_string();
            let root =
                url::Url::from_directory_path(absolute.parent().ok_or("Missing model directory")?)
                    .map_err(|_| "Cannot encode model directory")?
                    .to_string();
            std::fs::write(
                directory.join("cad.d.luau"),
                include_str!("../types/cad.luau")
                    .replace("local cad:", "declare cad:")
                    .split(" =\n")
                    .next()
                    .ok_or("Invalid CAD definitions")?
                    .to_owned()
                    + "\n",
            )
            .map_err(|error| error.to_string())?;
            std::fs::write(directory.join("settings.json"), configuration.to_string())
                .map_err(|error| error.to_string())?;
            std::fs::write(directory.join(".luaurc"), "{\"languageMode\":\"strict\"}")
                .map_err(|error| error.to_string())?;
            let mut command = Command::new("luau-lsp");
            command
                .arg("lsp")
                .arg("--stdio")
                .arg(format!(
                    "--base-luaurc={}",
                    directory.join(".luaurc").display()
                ))
                .arg(format!(
                    "--definitions=@spice-cad={}",
                    directory.join("cad.d.luau").display()
                ))
                .arg(format!(
                    "--settings={}",
                    directory.join("settings.json").display()
                ))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            #[cfg(target_os = "linux")]
            {
                use std::os::unix::process::CommandExt;
                unsafe {
                    command.pre_exec(|| {
                        let limit = libc::rlimit {
                            rlim_cur: 1024 * 1024 * 1024,
                            rlim_max: 1024 * 1024 * 1024,
                        };
                        if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                        Ok(())
                    });
                }
            }
            let mut child = command.spawn().map_err(|error| {
                format!(
                    "Cannot start luau-lsp: {error}. Use the Nix development shell or packaged app."
                )
            })?;
            let input = child.stdin.take().expect("Piped LSP input");
            let output = child.stdout.take().expect("Piped LSP output");
            let (sender, messages) = mpsc::sync_channel(32);
            let reader = match std::thread::Builder::new()
                .name("cad-lsp-reader".into())
                .spawn(move || {
                    let mut input = BufReader::new(output);
                    loop {
                        let message = read_message(&mut input);
                        let failed = message.is_err();
                        if sender.send(message).is_err() || failed {
                            break;
                        }
                    }
                }) {
                Ok(reader) => reader,
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error.to_string());
                }
            };
            *process.lock().expect("LSP process") = Some(child);
            let mut client = Self {
                child: process,
                input,
                messages,
                reader: Some(reader),
                directory: directory.clone(),
                uri,
                request_id: 0,
                version: 0,
                revision: 0,
                configuration: configuration.clone(),
                directive: None,
            };
            let initialized = client.request("initialize", json!({"processId":std::process::id(),"rootUri":root,"capabilities":{"general":{"positionEncodings":["utf-16"]},"workspace":{"configuration":true},"textDocument":{"diagnostic":{},"completion":{"completionItem":{"snippetSupport":false}}}}}), cancelled)?;
            if initialized["capabilities"]["diagnosticProvider"].is_null() {
                return Err("Luau LSP does not support pull diagnostics".into());
            }
            client.notify("initialized", json!({}))?;
            Ok(client)
        })();
        if started.is_err() {
            let _ = std::fs::remove_dir_all(&directory);
        }
        started
    }

    pub fn check(
        &mut self,
        source: &ropey::Rope,
        revision: u64,
        query: Query,
        cancelled: impl Fn() -> bool,
    ) -> Result<Option<Checked>, String> {
        if cancelled() {
            return Ok(None);
        }
        if self.revision != revision {
            self.version = self
                .version
                .checked_add(1)
                .ok_or("LSP version counter exhausted")?;
            let text = source.to_string();
            self.directive = analysis::directive_diagnostic(&text);
            if self.revision == 0 {
                self.notify("textDocument/didOpen", json!({"textDocument":{"uri":self.uri,"languageId":"luau","version":self.version,"text":text}}))?;
            } else {
                self.notify("textDocument/didChange", json!({"textDocument":{"uri":self.uri,"version":self.version},"contentChanges":[{"text":text}]}))?;
            }
            self.revision = revision;
        }
        let response = match self.request(
            "textDocument/diagnostic",
            json!({"textDocument":{"uri":self.uri}}),
            &cancelled,
        ) {
            Ok(response) => response,
            Err(_) if cancelled() => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut report = analysis::Report::default();
        if let Some(diagnostic) = &self.directive {
            report.diagnostics.push(diagnostic.clone());
        }
        if let Some(entries) = response["items"].as_array() {
            let remaining = 200 - report.diagnostics.len();
            report.truncated = entries.len() > remaining;
            for entry in entries.iter().take(remaining) {
                let start = protocol_position(&entry["range"]["start"])?;
                let end = protocol_position(&entry["range"]["end"])?;
                let start_index = from_position(source, start.0, start.1);
                let end_index = from_position(source, end.0, end.1);
                let start_line = source.char_to_line(start_index);
                let end_line = source.char_to_line(end_index);
                report.diagnostics.push(analysis::Diagnostic {
                    span: Some(analysis::Span {
                        start: analysis::Position {
                            line: start_line + 1,
                            column: source.char_to_byte(start_index)
                                - source.line_to_byte(start_line)
                                + 1,
                        },
                        end: analysis::Position {
                            line: end_line + 1,
                            column: source.char_to_byte(end_index) - source.line_to_byte(end_line),
                        },
                    }),
                    severity: if entry["severity"].as_u64().unwrap_or(1) == 1 {
                        analysis::Severity::Error
                    } else {
                        analysis::Severity::Warning
                    },
                    code: match &entry["code"] {
                        Value::String(code) => code.clone(),
                        code if !code.is_null() => code.to_string(),
                        _ => "Luau".into(),
                    },
                    message: entry["message"]
                        .as_str()
                        .unwrap_or("Language server diagnostic")
                        .chars()
                        .take(8192)
                        .collect(),
                });
            }
        }
        let insight = match query {
            Query::Check => None,
            Query::Complete(cursor) | Query::Hover(cursor) | Query::Definition(cursor) => {
                let cursor = cursor.min(source.len_chars());
                let (line, character) = position(source, cursor);
                let method = match query {
                    Query::Complete(_) => "textDocument/completion",
                    Query::Hover(_) => "textDocument/hover",
                    _ => "textDocument/definition",
                };
                let response = match self.request(method, json!({"textDocument":{"uri":self.uri},"position":{"line":line,"character":character}}), &cancelled) {
                    Ok(response) => response,
                    Err(_) if cancelled() => return Ok(None),
                    Err(error) => return Err(error),
                };
                Some(match query {
                    Query::Complete(_) => {
                        let entries = response.as_array().or_else(|| response["items"].as_array());
                        let mut prefix = cursor;
                        while prefix > 0
                            && (source.char(prefix - 1).is_alphanumeric()
                                || source.char(prefix - 1) == '_')
                        {
                            prefix -= 1;
                        }
                        let mut items = Vec::new();
                        for entry in entries.into_iter().flatten().take(100) {
                            if entry["insertTextFormat"].as_u64() == Some(2) {
                                continue;
                            }
                            if entry["additionalTextEdits"]
                                .as_array()
                                .is_some_and(|edits| !edits.is_empty())
                            {
                                continue;
                            }
                            let label = entry["label"]
                                .as_str()
                                .unwrap_or("")
                                .chars()
                                .take(256)
                                .collect::<String>();
                            let text = entry["textEdit"]["newText"]
                                .as_str()
                                .or_else(|| entry["insertText"].as_str())
                                .unwrap_or(&label)
                                .chars()
                                .take(8192)
                                .collect();
                            let range = entry["textEdit"]
                                .get("range")
                                .or_else(|| entry["textEdit"].get("insert"));
                            let replacement = if let Some(range) = range {
                                range_from_value(source, range)?
                            } else {
                                prefix..cursor
                            };
                            items.push(Completion {
                                label,
                                detail: entry["detail"]
                                    .as_str()
                                    .unwrap_or("")
                                    .chars()
                                    .take(1024)
                                    .collect(),
                                replacement,
                                text,
                            });
                        }
                        items.sort_by(|first, second| first.label.cmp(&second.label));
                        Insight::Completions(items)
                    }
                    Query::Hover(_) => {
                        let contents = &response["contents"];
                        let text = if let Some(array) = contents.as_array() {
                            array
                                .iter()
                                .map(markup_text)
                                .collect::<Vec<_>>()
                                .join("\n\n")
                        } else {
                            markup_text(contents)
                        };
                        Insight::Hover(text.chars().take(16384).collect())
                    }
                    _ => {
                        let entries: Vec<&Value> = match response.as_array() {
                            Some(array) => array.iter().take(100).collect(),
                            None if response.is_object() => vec![&response],
                            _ => Vec::new(),
                        };
                        let mut locations = Vec::new();
                        for entry in entries {
                            let uri = entry["uri"]
                                .as_str()
                                .or_else(|| entry["targetUri"].as_str())
                                .ok_or("Missing definition URI")?;
                            let path = url::Url::parse(uri)
                                .map_err(|error| error.to_string())?
                                .to_file_path()
                                .map_err(|_| "Definition is not a local file")?;
                            let range = entry
                                .get("range")
                                .or_else(|| entry.get("targetSelectionRange"))
                                .ok_or("Missing definition range")?;
                            locations.push(Location {
                                path,
                                start: protocol_position(&range["start"])?,
                                end: protocol_position(&range["end"])?,
                            });
                        }
                        if locations.is_empty()
                            || locations
                                .iter()
                                .any(|location| location.path.starts_with(&self.directory))
                        {
                            let response = self.request("textDocument/hover", json!({"textDocument":{"uri":self.uri},"position":{"line":line,"character":character}}), &cancelled)?;
                            Insight::Hover(
                                markup_text(&response["contents"])
                                    .chars()
                                    .take(16384)
                                    .collect(),
                            )
                        } else {
                            Insight::Definitions(locations)
                        }
                    }
                })
            }
        };
        if cancelled() {
            return Ok(None);
        }
        Ok(Some(Checked { report, insight }))
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.send(json!({"jsonrpc":"2.0","method":method,"params":params}))
    }
    fn send(&mut self, message: Value) -> Result<(), String> {
        let bytes = serde_json::to_vec(&message).map_err(|error| error.to_string())?;
        if bytes.len() > MESSAGE_SIZE_BYTES_MAX {
            return Err("LSP message exceeds 64 MiB".into());
        }
        write!(self.input, "Content-Length: {}\r\n\r\n", bytes.len())
            .map_err(|error| error.to_string())?;
        self.input
            .write_all(&bytes)
            .and_then(|()| self.input.flush())
            .map_err(|error| error.to_string())
    }
    fn request(
        &mut self,
        method: &str,
        params: Value,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Value, String> {
        self.request_id = self
            .request_id
            .checked_add(1)
            .ok_or("LSP request counter exhausted")?;
        let id = self.request_id;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if cancelled() {
                self.notify("$/cancelRequest", json!({"id":id}))?;
                return Err("LSP request superseded".into());
            }
            if Instant::now() >= deadline {
                self.notify("$/cancelRequest", json!({"id":id}))?;
                return Err(format!("LSP {method} exceeded 5 s"));
            }
            match self.messages.recv_timeout(Duration::from_millis(10)) {
                Ok(Ok(message)) => {
                    if let Some(method) = message["method"].as_str() {
                        if let Some(id) = message.get("id") {
                            let result = match method {
                                "workspace/configuration" => Value::Array(
                                    message["params"]["items"]
                                        .as_array()
                                        .map(|items| {
                                            items
                                                .iter()
                                                .map(|_| self.configuration.clone())
                                                .collect()
                                        })
                                        .unwrap_or_default(),
                                ),
                                "client/registerCapability" | "window/workDoneProgress/create" => {
                                    Value::Null
                                }
                                _ => {
                                    self.send(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Unsupported client request"}}))?;
                                    continue;
                                }
                            };
                            self.send(json!({"jsonrpc":"2.0","id":id,"result":result}))?;
                        }
                    } else if message["id"].as_u64() == Some(u64::from(id)) {
                        if let Some(error) = message.get("error") {
                            return Err(format!("LSP: {error}"));
                        }
                        return Ok(message["result"].clone());
                    }
                }
                Ok(Err(error)) => return Err(error),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("Language server disconnected".into());
                }
            }
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.lock().expect("LSP process").take() {
            let _ = child.kill();
            if let Err(error) = child.wait() {
                eprintln!("LSP: cannot reap server: {error}");
            }
        }
        while self.messages.try_recv().is_ok() {}
        if let Some(reader) = self.reader.take() {
            // Dropping the receiver before joining also releases a full reader queue.
            let (_, empty) = mpsc::channel();
            let receiver = std::mem::replace(&mut self.messages, empty);
            drop(receiver);
            if reader.join().is_err() {
                eprintln!("LSP: reader stopped unexpectedly");
            }
        }
        if let Err(error) = std::fs::remove_dir_all(&self.directory) {
            eprintln!("LSP: cannot remove temporary definitions: {error}");
        }
    }
}

fn read_message(reader: &mut impl BufRead) -> Result<Value, String> {
    let mut length = None;
    for _ in 0..32 {
        let mut line = String::new();
        let count = reader
            .take(8193)
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("Language server closed its output".into());
        }
        if count > 8192 {
            return Err("LSP header exceeds 8 KiB".into());
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|error| error.to_string())?,
            );
        }
    }
    let length = length.ok_or("Missing LSP content length")?;
    if length > MESSAGE_SIZE_BYTES_MAX {
        return Err("LSP response exceeds 64 MiB".into());
    }
    let mut bytes = vec![0; length];
    reader
        .read_exact(&mut bytes)
        .map_err(|error| error.to_string())?;
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}
fn protocol_position(value: &Value) -> Result<(usize, usize), String> {
    Ok((
        value["line"].as_u64().ok_or("Missing LSP line")? as usize,
        value["character"].as_u64().ok_or("Missing LSP character")? as usize,
    ))
}
fn from_position(source: &ropey::Rope, line: usize, character: usize) -> usize {
    let line = line.min(source.len_lines() - 1);
    let start = source.line_to_char(line);
    let end = start + source.line(line).len_chars();
    source.utf16_cu_to_char(
        (source.char_to_utf16_cu(start) + character).min(source.char_to_utf16_cu(end)),
    )
}
fn position(source: &ropey::Rope, cursor: usize) -> (usize, usize) {
    let line = source.char_to_line(cursor);
    (
        line,
        source.char_to_utf16_cu(cursor) - source.char_to_utf16_cu(source.line_to_char(line)),
    )
}
fn range_from_value(source: &ropey::Rope, value: &Value) -> Result<std::ops::Range<usize>, String> {
    let start = protocol_position(&value["start"])?;
    let end = protocol_position(&value["end"])?;
    let start = from_position(source, start.0, start.1);
    Ok(start..from_position(source, end.0, end.1).max(start))
}
fn markup_text(value: &Value) -> String {
    value
        .as_str()
        .or_else(|| value["value"].as_str())
        .unwrap_or("")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_buffer::Buffer;
    #[test]
    fn framing_and_utf16_positions_preserve_unicode() -> Result<(), String> {
        let source = ropey::Rope::from_str("local text = '😀λ'\nnext");
        for cursor in 0..=source.len_chars() {
            let (line, column) = position(&source, cursor);
            assert_eq!(from_position(&source, line, column), cursor);
        }
        assert!(
            read_message(&mut std::io::Cursor::new(
                b"Content-Length: 999999999\r\n\r\n"
            ))
            .is_err()
        );
        let value = json!({"id":1,"result":"λ"});
        let text = value.to_string();
        let input = format!("Content-Length: {}\r\n\r\n{text}", text.len());
        assert_eq!(read_message(&mut std::io::Cursor::new(input))?, value);
        Ok(())
    }
    #[test]
    fn server_provides_cad_completion_hover_and_versioned_diagnostics() -> Result<(), String> {
        let path = std::env::temp_dir().join("spice-cad-lsp-test.luau");
        let mut client = Client::new(&path, &|| false, Arc::new(Mutex::new(None)))?;
        let source = Buffer::from("--!strict\ncad.\n");
        let checked = client
            .check(&source.rope, source.revision, Query::Complete(14), || false)?
            .expect("Not cancelled");
        let Some(Insight::Completions(items)) = checked.insight else {
            panic!("Completion result");
        };
        assert!(items.iter().any(|item| item.label == "box"), "{items:?}");
        assert!(
            items.iter().any(|item| item.label == "polygon"),
            "{items:?}"
        );
        assert!(
            items.iter().any(|item| item.label == "extrude"),
            "{items:?}"
        );
        assert!(items.iter().any(|item| item.label == "edge"), "{items:?}");
        assert!(
            items.iter().any(|item| item.label == "chamfer"),
            "{items:?}"
        );
        let source = Buffer::from(
            "--!strict\nlocal width: number = 'bad'\ncad.show('plate',cad.box({width=10,depth=10,height=10}))",
        );
        let checked = client
            .check(&source.rope, source.revision, Query::Hover(17), || false)?
            .expect("Not cancelled");
        assert!(checked.report.error().is_some());
        assert!(matches!(checked.insight, Some(Insight::Hover(text)) if !text.is_empty()));
        let source = Buffer::from(
            "--!strict\nlocal width: number = 10\ncad.show('plate',cad.box({width=width,depth=10,height=10}))",
        );
        let cursor = source.to_string().find("width=width").expect("Width use") + 7;
        let checked = client
            .check(
                &source.rope,
                source.revision,
                Query::Definition(cursor),
                || false,
            )?
            .expect("Not cancelled");
        let Some(Insight::Definitions(locations)) = checked.insight else {
            panic!("Definition result");
        };
        assert_eq!(locations.len(), 1);
        assert_eq!(
            source
                .rope
                .slice(locations[0].range(&source.rope))
                .to_string(),
            "width"
        );
        let cursor = source.to_string().find("cad.box").expect("Box use") + 5;
        let checked = client
            .check(
                &source.rope,
                source.revision,
                Query::Definition(cursor),
                || false,
            )?
            .expect("Not cancelled");
        assert!(
            matches!(&checked.insight, Some(Insight::Hover(text)) if !text.is_empty()),
            "{:?}",
            checked.insight
        );
        let source = Buffer::from("--!nocheck\nlocal width = 'unchecked'\n");
        let checked = client
            .check(&source.rope, source.revision, Query::Check, || false)?
            .expect("Not cancelled");
        assert!(checked.report.error().is_some());
        Ok(())
    }
}
