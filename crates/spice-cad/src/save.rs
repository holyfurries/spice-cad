use crate::{document, interface, model};
use eframe::egui;
use std::{path::PathBuf, sync::mpsc};

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    NewModel,
    SaveAs,
}

struct Target {
    path: PathBuf,
    expected: Option<String>,
}
enum Request {
    Inspect(PathBuf),
    Write {
        target: Target,
        source: ropey::Rope,
        revision: u64,
    },
}
enum Response {
    Inspected(Target),
    Written(Saved),
}
pub struct Saved {
    pub path: PathBuf,
    pub source: String,
    pub revision: u64,
}
pub enum Selection {
    New(PathBuf),
    Saved(Saved),
}
enum Phase {
    Editing,
    Checking,
    Confirm(Target),
    Writing,
}

pub struct Dialog {
    pub origin: PathBuf,
    mode: Mode,
    input: String,
    phase: Phase,
    focus: bool,
    error: Option<String>,
    requests: mpsc::SyncSender<Request>,
    responses: mpsc::Receiver<Result<Response, String>>,
}

fn inspect(path: PathBuf) -> Result<Target, String> {
    if !path
        .extension()
        .is_some_and(|extension| extension == "luau" || extension == "lua")
    {
        return Err("Use a .luau or .lua filename".into());
    }
    let parent = std::fs::canonicalize(path.parent().ok_or("Choose a directory and filename")?)
        .map_err(|error| format!("Cannot open destination directory: {error}"))?;
    let path = parent.join(path.file_name().ok_or("Choose a filename")?);
    let expected = if path.try_exists().map_err(|error| error.to_string())? {
        Some(model::read_source(&path)?)
    } else {
        None
    };
    Ok(Target { path, expected })
}

impl Dialog {
    pub fn new(mode: Mode, origin: PathBuf, context: egui::Context) -> Result<Self, String> {
        let directory = origin.parent().unwrap_or(std::path::Path::new("."));
        let path = match mode {
            Mode::NewModel => directory.join("untitled.luau"),
            Mode::SaveAs => directory.join(format!(
                "{}-copy.luau",
                origin.file_stem().unwrap_or_default().to_string_lossy()
            )),
        };
        let (requests, incoming) = mpsc::sync_channel(1);
        let (completed, responses) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("cad-save-as".into())
            .spawn(move || {
                while let Ok(request) = incoming.recv() {
                    let result = match request {
                        Request::Inspect(path) => inspect(path).map(Response::Inspected),
                        Request::Write {
                            target,
                            source,
                            revision,
                        } => document::write_source(
                            &target.path,
                            &source,
                            target.expected.as_deref(),
                        )
                        .map(|source| {
                            Response::Written(Saved {
                                path: target.path,
                                source,
                                revision,
                            })
                        }),
                    };
                    if completed.send(result).is_err() {
                        break;
                    }
                    context.request_repaint();
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            origin,
            mode,
            input: path.display().to_string(),
            phase: Phase::Editing,
            focus: true,
            error: None,
            requests,
            responses,
        })
    }

    pub fn writing(&self) -> bool {
        matches!(self.phase, Phase::Writing)
    }

    fn write(&mut self, target: Target, source: Option<(&ropey::Rope, u64)>) {
        let Some((source, revision)) = source else {
            self.error = Some("No model is open".into());
            self.phase = Phase::Editing;
            return;
        };
        match self.requests.try_send(Request::Write {
            target,
            source: source.clone(),
            revision,
        }) {
            Ok(()) => self.phase = Phase::Writing,
            Err(error) => {
                self.error = Some(error.to_string());
                self.phase = Phase::Editing;
            }
        }
    }

    pub fn show(
        &mut self,
        context: &egui::Context,
        source: Option<(&ropey::Rope, u64)>,
    ) -> (bool, Option<Selection>) {
        match self.responses.try_recv() {
            Ok(Ok(Response::Inspected(target))) => {
                if self.mode == Mode::NewModel {
                    if target.expected.is_none() {
                        return (true, Some(Selection::New(target.path)));
                    }
                    self.error = Some("That file already exists. Choose a new filename.".into());
                    self.phase = Phase::Editing;
                } else if target.expected.is_some() {
                    self.phase = Phase::Confirm(target);
                } else {
                    self.write(target, source);
                }
            }
            Ok(Ok(Response::Written(saved))) => return (true, Some(Selection::Saved(saved))),
            Ok(Err(error)) => {
                self.error = Some(error);
                self.phase = Phase::Editing;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.error = Some("File worker stopped; close and reopen this dialog".into());
                self.phase = Phase::Editing;
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        let enter = context
            .memory(|memory| memory.has_focus(egui::Id::unique("save_model_path_input")))
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let busy = matches!(self.phase, Phase::Checking | Phase::Writing);
        let mut cancel = false;
        let mut submit = false;
        let mut replace = false;
        let response = egui::Modal::new(egui::Id::unique("save_model_path")).show(context, |ui| {
            ui.set_width((context.content_rect().width() - 48.0).clamp(240.0, 600.0));
            ui.heading(if self.mode == Mode::NewModel {
                "New model"
            } else {
                "Save model as"
            });
            if let Phase::Confirm(target) = &self.phase {
                ui.label("Replace the existing model at this path?");
                ui.add(egui::Label::new(target.path.display().to_string()).truncate())
                    .on_hover_text(target.path.display().to_string());
                ui.horizontal(|ui| {
                    replace = ui.button("Replace file").clicked();
                    if ui.button("Choose another path").clicked() {
                        self.phase = Phase::Editing;
                        self.focus = true;
                    }
                    cancel = ui.button("Cancel").clicked();
                });
            } else {
                ui.add_enabled_ui(!busy, |ui| {
                    let input = ui.add(
                        egui::TextEdit::singleline(&mut self.input)
                            .id(egui::Id::unique("save_model_path_input"))
                            .desired_width(f32::INFINITY)
                            .char_limit(4096)
                            .margin(egui::vec2(8.0, 6.0)),
                    );
                    if self.focus {
                        input.request_focus();
                        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), input.id) {
                            state
                                .cursor
                                .set_char_range(Some(egui::text::CCursorRange::two(
                                    egui::text::CCursor::new(0),
                                    egui::text::CCursor::new(self.input.chars().count()),
                                )));
                            state.store(ui.ctx(), input.id);
                        }
                        self.focus = false;
                    }
                    ui.label(
                        egui::RichText::new(if self.mode == Mode::NewModel {
                            "Choose a path for the draft. Save writes it to disk."
                        } else {
                            "The current source and undo history are preserved."
                        })
                        .small()
                        .color(interface::MUTED),
                    );
                    ui.horizontal(|ui| {
                        submit = ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(if self.mode == Mode::NewModel {
                                        "Create draft"
                                    } else {
                                        "Save as"
                                    })
                                    .color(interface::ON_ACCENT),
                                )
                                .fill(interface::ACCENT),
                            )
                            .clicked()
                            || enter;
                        cancel = ui.button("Cancel").clicked();
                    });
                });
                if busy {
                    ui.label(if self.writing() {
                        "Saving model…"
                    } else {
                        "Checking destination…"
                    });
                    if matches!(self.phase, Phase::Checking) {
                        cancel = ui.button("Cancel").clicked();
                    }
                }
            }
            if let Some(error) = &self.error {
                ui.colored_label(interface::WARNING, error);
            }
        });
        if submit && !busy {
            let path = PathBuf::from(self.input.trim());
            let path = if path.is_absolute() {
                path
            } else {
                self.origin
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .join(path)
            };
            self.error = None;
            match self.requests.try_send(Request::Inspect(path)) {
                Ok(()) => self.phase = Phase::Checking,
                Err(error) => self.error = Some(error.to_string()),
            }
        }
        if replace {
            if let Phase::Confirm(target) = std::mem::replace(&mut self.phase, Phase::Editing) {
                self.write(target, source);
            }
        }
        let closed = !self.writing() && (cancel || response.should_close());
        if closed {
            context.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(event, egui::Event::Key { .. } | egui::Event::Text(_))
                })
            });
        }
        (closed, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_checks_and_atomic_writes_preserve_conflicting_files() -> Result<(), String> {
        let folder = std::env::temp_dir().join(format!("spice-cad-save-as-{}", std::process::id()));
        std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
        let path = folder.join("part.luau");
        let target = inspect(path.clone())?;
        assert!(target.expected.is_none());
        let source = ropey::Rope::from_str("local width = 20");
        document::write_source(&path, &source, None)?;
        assert!(document::write_source(&path, &source, None).is_err());
        let target = inspect(path.clone())?;
        std::fs::write(&path, "external edit").map_err(|error| error.to_string())?;
        assert!(document::write_source(&path, &source, target.expected.as_deref()).is_err());
        assert_eq!(model::read_source(&path)?, "external edit");
        assert!(inspect(folder.join("part.stl")).is_err());
        assert!(inspect(folder.join("missing/part.luau")).is_err());
        let target = inspect(path.clone())?;
        document::write_source(&path, &source, target.expected.as_deref())?;
        assert_eq!(model::read_source(&path)?, "local width = 20");
        std::fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        Ok(())
    }
}
