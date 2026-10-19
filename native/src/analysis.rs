use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

const TYPE_CONTEXT: &str = include_str!("../types/cad.luau");
const OUTPUT_SIZE_BYTES_MAX: u64 = 65536;
const DIAGNOSTIC_COUNT_MAX: usize = 200;

#[derive(Default)]
pub struct Cache {
    successful: Option<String>,
}

impl Cache {
    pub fn validate(&mut self, source: &str) -> Result<bool, String> {
        let key = crate::parameters::analysis_key(source);
        if key.is_some() && key == self.successful {
            return Ok(true);
        }
        let report = check(source, || false)?.ok_or("Analysis cancelled")?;
        if let Some(error) = report.error() {
            return Err(error);
        }
        self.successful = key;
        Ok(false)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub start: Position,
    pub end: Position,
}

impl Span {
    pub fn range(&self, buffer: &crate::text_buffer::Buffer) -> std::ops::Range<usize> {
        let position = |position: Position, end: bool| {
            let line_index = position
                .line
                .saturating_sub(1)
                .min(buffer.rope.len_lines() - 1);
            let line = buffer.rope.line(line_index);
            let byte = if end {
                position.column
            } else {
                position.column.saturating_sub(1)
            }
            .min(line.len_bytes());
            buffer.rope.line_to_char(line_index) + line.byte_to_char(byte)
        };
        let start = position(self.start, false);
        start..position(self.end, true).max(start)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub span: Option<Span>,
    pub severity: Severity,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct Report {
    pub diagnostics: Vec<Diagnostic>,
    pub truncated: bool,
}

impl Report {
    pub fn error(&self) -> Option<String> {
        let errors: Vec<_> = self
            .diagnostics
            .iter()
            .filter(|entry| entry.severity == Severity::Error)
            .map(Diagnostic::description)
            .collect();
        if errors.is_empty() {
            return None;
        }
        Some(format!("Luau type check failed:\n{}", errors.join("\n")))
    }
}

impl Diagnostic {
    pub fn description(&self) -> String {
        let location = self.span.map_or(String::new(), |span| {
            format!(
                "model.luau:{}.{}-{}.{}: ",
                span.start.line, span.start.column, span.end.line, span.end.column
            )
        });
        format!("{location}{}: {}", self.code, self.message)
    }

    pub fn build_error(message: &str) -> Self {
        let line = message
            .split("model.luau:")
            .nth(1)
            .and_then(|rest| rest.split(':').next())
            .and_then(|line| line.parse::<usize>().ok())
            .filter(|line| *line > 0);
        Self {
            span: line.map(|line| Span {
                start: Position { line, column: 1 },
                end: Position { line, column: 1 },
            }),
            severity: Severity::Error,
            code: "Build".into(),
            message: message.chars().take(8192).collect(),
        }
    }
}

struct AnalysisFiles(PathBuf);
impl Drop for AnalysisFiles {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("Analysis: could not remove {}: {error}", self.0.display());
        }
    }
}

struct Analyzer(Child);
impl Drop for Analyzer {
    fn drop(&mut self) {
        match self.0.try_wait() {
            Ok(Some(_)) => {}
            _ => {
                if let Err(error) = self.0.kill() {
                    eprintln!("Analysis: cannot stop analyzer: {error}");
                }
                if let Err(error) = self.0.wait() {
                    eprintln!("Analysis: cannot reap analyzer: {error}");
                }
            }
        }
    }
}

pub fn directive_diagnostic(source: &str) -> Option<Diagnostic> {
    for (index, line) in source.lines().enumerate() {
        if line.trim().starts_with("--!") && line.trim() != "--!strict" {
            return Some(Diagnostic {
                span: Some(Span {
                    start: Position {
                        line: index + 1,
                        column: 1,
                    },
                    end: Position {
                        line: index + 1,
                        column: line.len().max(1),
                    },
                }),
                severity: Severity::Error,
                code: "StrictMode".into(),
                message: "Models must use strict type checking".into(),
            });
        }
    }
    None
}

pub fn check(source: &str, cancelled: impl Fn() -> bool) -> Result<Option<Report>, String> {
    if source.len() > crate::text_buffer::SIZE_BYTES_MAX {
        return Err("Model source exceeds 16 MiB".into());
    }
    if cancelled() {
        return Ok(None);
    }
    if let Some(diagnostic) = directive_diagnostic(source) {
        return Ok(Some(Report {
            diagnostics: vec![diagnostic],
            truncated: false,
        }));
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "spice-cad-analysis-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&path).map_err(|error| error.to_string())?;
    let files = AnalysisFiles(path);
    let diagnostics_path = files.0.join("diagnostics");
    let diagnostics =
        std::fs::File::create(&diagnostics_path).map_err(|error| error.to_string())?;
    let mut input = std::io::BufWriter::new(
        std::fs::File::create(files.0.join("input.luau")).map_err(|error| error.to_string())?,
    );
    let prefix = format!("--!strict\n{TYPE_CONTEXT}\n");
    input
        .write_all(prefix.as_bytes())
        .map_err(|error| error.to_string())?;
    for line in source.split_inclusive('\n') {
        if cancelled() {
            return Ok(None);
        }
        let line = if line.trim() == "--!strict" {
            if line.ends_with('\n') { "\n" } else { "" }
        } else {
            line
        };
        input
            .write_all(line.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    input.flush().map_err(|error| error.to_string())?;
    let mut command = Command::new("luau-analyze");
    command
        .args(["--mode=strict", "--formatter=gnu", "-j1", "input.luau"])
        .current_dir(&files.0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(diagnostics);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                let limit = libc::rlimit {
                    rlim_cur: 512 * 1024 * 1024,
                    rlim_max: 512 * 1024 * 1024,
                };
                if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut child = Analyzer(command.spawn().map_err(|error| {
        format!("Cannot start luau-analyze: {error}. Launch through nix develop.")
    })?);
    let deadline = Instant::now() + Duration::from_secs(5);
    let (success, exceeded) = loop {
        if cancelled() {
            return Ok(None);
        }
        if let Some(status) = child.0.try_wait().map_err(|error| error.to_string())? {
            break (status.success(), false);
        }
        let size = std::fs::metadata(&diagnostics_path)
            .map_err(|error| error.to_string())?
            .len();
        if size > OUTPUT_SIZE_BYTES_MAX {
            break (false, true);
        }
        if Instant::now() >= deadline {
            return Err("Luau analyzer exceeded its 5 s budget".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    drop(child);
    let mut bytes = Vec::new();
    std::fs::File::open(&diagnostics_path)
        .map_err(|error| error.to_string())?
        .take(OUTPUT_SIZE_BYTES_MAX)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let mut report = parse(&String::from_utf8_lossy(&bytes), prefix.lines().count());
    report.truncated |= exceeded || bytes.len() as u64 == OUTPUT_SIZE_BYTES_MAX;
    if !success && report.error().is_none() {
        report.diagnostics.push(Diagnostic {
            span: None,
            severity: Severity::Error,
            code: "Analyzer".into(),
            message: if exceeded {
                "Analyzer diagnostic output exceeded 64 KiB".into()
            } else {
                "Analyzer failed or exceeded its memory budget".into()
            },
        });
    }
    Ok(Some(report))
}

fn parse(output: &str, offset: usize) -> Report {
    let mut report = Report::default();
    for line in output.lines() {
        if report.diagnostics.len() >= DIAGNOSTIC_COUNT_MAX {
            report.truncated = true;
            break;
        }
        let Some(rest) = line
            .strip_prefix("./input.luau:")
            .or_else(|| line.strip_prefix("input.luau:"))
        else {
            if let Some(previous) = report.diagnostics.last_mut() {
                previous.message.push('\n');
                previous.message.push_str(line);
            }
            continue;
        };
        let Some((span, details)) = rest.split_once(':') else {
            continue;
        };
        let Some((first, last)) = span.split_once('-') else {
            continue;
        };
        let position = |source: &str| -> Option<Position> {
            let (line, column) = source.split_once('.')?;
            Some(Position {
                line: line.parse::<usize>().ok()?,
                column: column.parse::<usize>().ok()?,
            })
        };
        let (Some(first), Some(last)) = (position(first), position(last)) else {
            continue;
        };
        let Some((code, message)) = details.trim().split_once(':') else {
            continue;
        };
        let severity = if ["TypeError", "SyntaxError"].contains(&code) {
            Severity::Error
        } else {
            Severity::Warning
        };
        if first.line <= offset && severity == Severity::Warning {
            continue;
        }
        let span = if first.line > offset && last.line > offset {
            Some(Span {
                start: Position {
                    line: first.line - offset,
                    column: first.column.max(1),
                },
                end: Position {
                    line: last.line - offset,
                    column: last.column,
                },
            })
        } else {
            None
        };
        report.diagnostics.push(Diagnostic {
            span,
            severity,
            code: code.into(),
            message: message.trim().into(),
        });
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_checks_reuse_only_numeric_property_literals() -> Result<(), String> {
        let source = "-- λ\nlocal movement = cad.number(10, {min=-100,max=100})\nlocal mode = cad.toggle(false,{})\nlocal ordinary: number = 10\nassert(movement > 0)\ncad.show('part',cad.box({width=10,depth=10,height=10}))";
        let mut cache = Cache::default();
        assert!(!cache.validate(source)?);
        for literal in ["11", "-12.5", "1e2"] {
            let edited = source.replacen("number(10,", &format!("number({literal},"), 1);
            assert!(cache.validate(&edited)?);
            assert!(
                check(&edited, || false)?
                    .expect("Checked")
                    .error()
                    .is_none()
            );
        }
        for edited in [
            source.replace("false", "true"),
            source.replace("max=100", "max=101"),
            source.replace("number = 10", "number = 11"),
            source.replace("movement > 0", "movement > 1"),
            source.replace("number(10,", "number(5+5,"),
        ] {
            assert_ne!(
                crate::parameters::analysis_key(source),
                crate::parameters::analysis_key(&edited)
            );
        }
        let invalid = source.replace("ordinary: number = 10", "ordinary: number = 'bad'");
        assert!(cache.validate(&invalid).is_err());
        assert!(cache.validate(source)?);
        assert!(
            crate::model::compile_cached(&source.replace("number(10,", "number(-1,"), &mut cache)
                .is_err()
        );
        assert!(cache.validate(source)?);
        assert!(crate::model::compile_cached(source, &mut cache).is_ok());
        assert!(cache.validate("local bad = cad.number(").is_err());
        let rack = include_str!("../../models/rack-parts.luau");
        let mut cache = Cache::default();
        assert!(!cache.validate(rack)?);
        assert!(cache.validate(&rack.replacen("number(12,", "number(25,", 1))?);
        Ok(())
    }

    #[test]
    fn positions_are_model_relative_and_warnings_do_not_block() -> Result<(), String> {
        let report = check(
            "--!strict\nlocal text = \"λ\"; local value: number = \"bad\"",
            || false,
        )?
        .expect("Not cancelled");
        let error = report
            .diagnostics
            .iter()
            .find(|entry| entry.severity == Severity::Error)
            .expect("Type error");
        assert_eq!(
            error.span.expect("Source span").start,
            Position {
                line: 2,
                column: 42
            }
        );
        assert!(report.error().is_some());
        let report = check("local unused: number = 42", || false)?.expect("Not cancelled");
        assert!(report.error().is_none());
        assert!(
            report
                .diagnostics
                .iter()
                .any(|entry| entry.severity == Severity::Warning)
        );
        assert!(check("invalid", || true)?.is_none());
        Ok(())
    }

    #[test]
    fn malformed_output_and_prefix_diagnostics_are_not_false_locations() {
        let report = parse(
            "input.luau:1.1-1.2: LocalUnused: host warning\ninput.luau:9.2-9.5: SyntaxError: expected expression\n  continuation\ninput.luau:broken: bad",
            8,
        );
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].span.expect("Model span").start.line,
            1
        );
        assert!(report.diagnostics[0].message.contains("continuation"));
    }

    #[test]
    fn runtime_trace_points_to_the_model_call_site() -> Result<(), String> {
        let error = crate::model::compile("--!strict\nlocal radius: number = -1\ncad.show('bad', cad.cylinder({radius=radius,height=10}))").err().expect("Invalid radius");
        let diagnostic = Diagnostic::build_error(&error);
        assert_eq!(diagnostic.span.expect("Runtime location").start.line, 3);
        assert_eq!(diagnostic.severity, Severity::Error);
        assert!(diagnostic.message.contains("radius must"));
        Ok(())
    }

    #[test]
    fn inclusive_byte_columns_select_unicode_tokens_and_multiline_ranges() {
        let buffer =
            crate::text_buffer::Buffer::from("local a = 'λ'; local b = 'bad'\nsecond line\n");
        let span = Span {
            start: Position {
                line: 1,
                column: 12,
            },
            end: Position {
                line: 1,
                column: 13,
            },
        };
        assert_eq!(buffer.rope.slice(span.range(&buffer)).to_string(), "λ");
        let span = Span {
            start: Position {
                line: 1,
                column: 27,
            },
            end: Position { line: 2, column: 6 },
        };
        assert_eq!(
            buffer.rope.slice(span.range(&buffer)).to_string(),
            "'bad'\nsecond"
        );
    }
}
