use crate::{document::Document, model};
use eframe::egui;
use std::{path::PathBuf, sync::mpsc};

const ENTRY_COUNT_MAX: usize = 1024;

enum Request {
    Directory(PathBuf),
    Model(PathBuf),
    Example(crate::document::Example),
}
enum Response {
    Directory(PathBuf, Result<Vec<(PathBuf, bool)>, String>),
    Model(Result<Box<Document>, String>),
}

pub struct Picker {
    pub open: bool,
    visible: bool,
    path: String,
    directory: PathBuf,
    entries: Vec<(PathBuf, bool)>,
    pending: Option<Request>,
    requests: mpsc::SyncSender<Request>,
    responses: mpsc::Receiver<Response>,
    error: Option<String>,
    loading: bool,
    focus_path: bool,
}

impl Picker {
    pub fn open_example(&mut self, example: crate::document::Example) {
        self.open = true;
        self.visible = false;
        self.pending = Some(Request::Example(example));
    }
    pub fn open_path(&mut self, path: PathBuf) {
        self.open = true;
        self.visible = false;
        self.pending = Some(Request::Model(path));
    }
    pub fn new(path: PathBuf, context: egui::Context) -> Result<Self, String> {
        let (requests, incoming) = mpsc::sync_channel(1);
        let (completed, responses) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("cad-files".into())
            .spawn(move || {
                while let Ok(request) = incoming.recv() {
                    let response = match request {
                        Request::Directory(path) => {
                            let entries = list(&path);
                            Response::Directory(path, entries)
                        }
                        Request::Model(path) if path.is_dir() => {
                            Response::Directory(path.clone(), list(&path))
                        }
                        Request::Model(path) => Response::Model(
                            model::read_source(&path)
                                .map(|source| Box::new(Document::loaded(path, source))),
                        ),
                        Request::Example(example) => Response::Model(
                            crate::document::example_model(example).and_then(|path| {
                                model::read_source(&path)
                                    .map(|source| Box::new(Document::loaded(path, source)))
                            }),
                        ),
                    };
                    if completed.send(response).is_err() {
                        break;
                    }
                    context.request_repaint();
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            open: true,
            visible: true,
            path: path.display().to_string(),
            directory: path.clone(),
            entries: Vec::new(),
            pending: Some(Request::Directory(path)),
            requests,
            responses,
            error: None,
            loading: false,
            focus_path: true,
        })
    }

    pub fn show(&mut self, context: &egui::Context) -> Option<Document> {
        if let Some(request) = self.pending.take() {
            match self.requests.try_send(request) {
                Ok(()) => {
                    self.loading = true;
                    self.error = None;
                }
                Err(mpsc::TrySendError::Full(request)) => {
                    self.pending = Some(request);
                    context.request_repaint_after(std::time::Duration::from_millis(100));
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    self.loading = false;
                    self.visible = true;
                    self.error = Some("File worker stopped; reopen the application".into());
                }
            }
        }
        if let Ok(response) = self.responses.try_recv() {
            self.loading = false;
            match response {
                Response::Model(Ok(document)) if self.open => {
                    self.open = false;
                    return Some(*document);
                }
                Response::Model(Ok(_)) => {}
                Response::Model(Err(error)) => {
                    self.error = Some(error);
                    self.visible = true;
                }
                Response::Directory(path, result) => {
                    self.visible = true;
                    match result {
                        Ok(entries) => {
                            self.path = path.display().to_string();
                            self.directory = path;
                            self.entries = entries;
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
            }
        }
        if !self.open || !self.visible {
            return None;
        }
        let mut open = self.open;
        let mut cancel =
            context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        egui::Window::new("Open Luau model")
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_size([640.0, 420.0])
            .collapsible(false)
            .show(context, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Up").clicked() {
                        if let Some(parent) = self.directory.parent() {
                            self.pending = Some(Request::Directory(parent.to_owned()));
                        }
                    }
                    if ui.button("Home").clicked() {
                        if let Some(home) = std::env::var_os("HOME") {
                            self.pending = Some(Request::Directory(home.into()));
                        }
                    }
                    let path = ui.add(
                        egui::TextEdit::singleline(&mut self.path)
                            .char_limit(4096)
                            .margin(egui::vec2(8.0, 6.0))
                            .desired_width(f32::INFINITY),
                    );
                    if self.focus_path {
                        path.request_focus();
                        self.focus_path = false;
                    }
                    if path.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                        self.pending = Some(Request::Model(self.input_path()));
                    }
                });
                if let Some(error) = &self.error {
                    ui.colored_label(crate::interface::WARNING, error);
                }
                if self.loading {
                    ui.small("Reading files…");
                }
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        for (path, directory) in &self.entries {
                            let name = path.file_name().unwrap_or_default().to_string_lossy();
                            let label = if *directory {
                                format!("{name}/")
                            } else {
                                name.to_string()
                            };
                            let response =
                                ui.selectable_label(self.path == path.display().to_string(), label);
                            if response.clicked() {
                                self.path = path.display().to_string();
                            }
                            if response.double_clicked() && !self.loading {
                                self.pending = Some(if *directory {
                                    Request::Directory(path.clone())
                                } else {
                                    Request::Model(path.clone())
                                });
                            }
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Browse path").clicked() {
                        self.pending = Some(Request::Directory(self.input_path()));
                    }
                    if ui
                        .add_enabled(
                            !self.loading,
                            egui::Button::new(
                                egui::RichText::new("Open file").color(crate::interface::ON_ACCENT),
                            )
                            .fill(crate::interface::ACCENT),
                        )
                        .clicked()
                    {
                        self.pending = Some(Request::Model(self.input_path()));
                    }
                    cancel |= ui.button("Cancel").clicked();
                    ui.small(".luau / .lua · double-click a folder to browse");
                });
                ui.small("Scans up to 1024 directory entries. Enter a path for other files.");
            });
        self.open = open && !cancel;
        if !self.open {
            self.pending = None;
        }
        if self.pending.is_some() {
            context.request_repaint();
        }
        None
    }

    fn input_path(&self) -> PathBuf {
        let path = PathBuf::from(&self.path);
        if path.is_absolute() {
            path
        } else {
            self.directory.join(path)
        }
    }
}

fn list(path: &std::path::Path) -> Result<Vec<(PathBuf, bool)>, String> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path)
        .map_err(|error| error.to_string())?
        .take(ENTRY_COUNT_MAX)
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let directory = path.is_dir();
        if directory
            || path
                .extension()
                .is_some_and(|extension| extension == "luau" || extension == "lua")
        {
            entries.push((path, directory));
        }
    }
    entries.sort_by(|first, second| second.1.cmp(&first.1).then_with(|| first.0.cmp(&second.0)));
    Ok(entries)
}
