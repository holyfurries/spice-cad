use crate::interface;
use eframe::egui;
use std::{
    collections::VecDeque,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

const FILE_COUNT_MAX: usize = 4096;
const ENTRY_COUNT_MAX: usize = 16_384;
const RECENT_COUNT_MAX: usize = 20;
const HISTORY_BYTES_MAX: u64 = 128 * 1024;
const RESULT_COUNT_MAX: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    FitGauge,
    Open,
    Run,
    Save,
    NewModel,
    SaveAs,
    AutoPreview,
    Export,
    ExportGame,
    Find,
    Replace,
    Code,
    Split,
    Model,
    AssemblyLayout,
    PrintLayout,
    Parameters,
    Diagnostics,
    Display,
    Edges,
    Vim,
    Performance,
    Frame,
    Front,
    Top,
    Rack,
    RackParts,
    Bracket,
    ExtrudedBracket,
    EdgeFillets,
    EdgeChamfers,
    RevolvedBushing,
    RocketEngine,
}

pub const COMMANDS: &[(Command, &str, &str)] = &[
    (Command::FitGauge, "Examples: print fit gauge", ""),
    (Command::NewModel, "File: new model", "Ctrl+Shift+N"),
    (Command::SaveAs, "File: save model as", "Ctrl+Shift+S"),
    (Command::AutoPreview, "Model: toggle live preview", ""),
    (Command::Open, "File: browse models", "Ctrl+Shift+O"),
    (Command::Run, "Model: run source", "Ctrl+Enter"),
    (Command::Save, "File: save model", "Ctrl+S"),
    (Command::Export, "File: export STEP and STL", ""),
    (Command::ExportGame, "File: export game asset GLB", ""),
    (Command::Find, "Edit: find", "Ctrl+Shift+F"),
    (Command::Replace, "Edit: replace", "Ctrl+Shift+H"),
    (Command::Code, "View: focus code", "Ctrl+1"),
    (Command::Split, "View: split code and model", "Ctrl+2"),
    (Command::Model, "View: focus model", "Ctrl+3"),
    (Command::AssemblyLayout, "Model: assembly layout", ""),
    (Command::PrintLayout, "Model: print layout", ""),
    (Command::Parameters, "View: parameters and properties", ""),
    (
        Command::Diagnostics,
        "View: toggle diagnostics",
        "Ctrl+Shift+J",
    ),
    (Command::Display, "View: appearance settings", ""),
    (Command::Edges, "View: toggle surface edges", ""),
    (Command::Vim, "Edit: toggle Vim-style keys", ""),
    (Command::Performance, "View: toggle performance", ""),
    (Command::Frame, "Camera: frame model", "F"),
    (Command::Front, "Camera: front view", ""),
    (Command::Top, "Camera: top view", ""),
    (Command::Rack, "Examples: spice rack", ""),
    (Command::RackParts, "Examples: spice rack four parts", ""),
    (Command::Bracket, "Examples: mounting bracket", ""),
    (Command::ExtrudedBracket, "Examples: extruded bracket", ""),
    (Command::EdgeFillets, "Examples: selected edge fillets", ""),
    (
        Command::EdgeChamfers,
        "Examples: selected edge chamfers",
        "",
    ),
    (Command::RevolvedBushing, "Examples: revolved bushing", ""),
    (
        Command::RocketEngine,
        "Examples: rocket engine assembly",
        "",
    ),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Files,
    Commands,
}

pub enum Selection {
    File(PathBuf),
    Command(Command),
    Cancel,
}

pub struct Availability {
    pub source: bool,
    pub run: bool,
    pub export: bool,
    pub model: bool,
}

impl Availability {
    fn allows(&self, command: Command) -> bool {
        match command {
            Command::Run => self.run,
            Command::Save
            | Command::SaveAs
            | Command::AutoPreview
            | Command::Find
            | Command::Replace
            | Command::Code
            | Command::Split
            | Command::Vim => self.source,
            Command::Export => self.export,
            Command::ExportGame => self.source && self.export,
            Command::Frame => self.model,
            _ => true,
        }
    }
}

struct File {
    path: PathBuf,
    label: String,
    recent: bool,
}
struct Index {
    files: Vec<File>,
    notice: Option<String>,
}
struct Job {
    generation: u64,
    root: Option<PathBuf>,
    recent: Vec<PathBuf>,
}
#[derive(Default)]
struct Slots {
    pending: Option<Job>,
    completed: Option<(u64, Index)>,
    stopped: bool,
}
struct Worker {
    slots: Arc<(Mutex<Slots>, Condvar)>,
    generation: Arc<AtomicU64>,
}

impl Worker {
    fn new(context: egui::Context) -> Result<Self, String> {
        let slots = Arc::new((Mutex::new(Slots::default()), Condvar::new()));
        let shared = Arc::clone(&slots);
        let generation = Arc::new(AtomicU64::new(0));
        let desired = Arc::clone(&generation);
        std::thread::Builder::new()
            .name("cad-file-index".into())
            .spawn(move || {
                let history = history_path();
                let (mut recent, mut history_error) = match &history {
                    Ok(path) => match read_history(path) {
                        Ok(recent) => (recent, None),
                        Err(error) => (Vec::new(), Some(error)),
                    },
                    Err(error) => (Vec::new(), Some(error.clone())),
                };
                loop {
                    let job = {
                        let mut slots = shared.0.lock().expect("File index slots");
                        while slots.pending.is_none() && !slots.stopped {
                            slots = shared.1.wait(slots).expect("File index wake");
                        }
                        if slots.stopped {
                            break;
                        }
                        slots.pending.take().expect("File index job")
                    };
                    for path in job.recent.into_iter().rev() {
                        remember(&mut recent, path);
                    }
                    if let Ok(path) = &history {
                        if let Err(error) = save_history(path, &recent) {
                            history_error = Some(error);
                        }
                    }
                    let mut index = index(job.root.as_deref(), &recent, || {
                        desired.load(Ordering::Acquire) != job.generation
                    });
                    if index.notice.is_none() {
                        index.notice = history_error.clone();
                    }
                    if desired.load(Ordering::Acquire) == job.generation {
                        shared.0.lock().expect("File index slots").completed =
                            Some((job.generation, index));
                        context.request_repaint();
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self { slots, generation })
    }

    fn submit(&self, root: Option<PathBuf>, recent: Vec<PathBuf>) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let mut slots = self.slots.0.lock().expect("File index slots");
        slots.pending = Some(Job {
            generation,
            root,
            recent,
        });
        slots.completed = None;
        self.slots.1.notify_one();
        generation
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.slots.0.lock().expect("File index slots").stopped = true;
        self.slots.1.notify_one();
    }
}

fn remember(recent: &mut Vec<PathBuf>, path: PathBuf) {
    if path.as_os_str().len() > 4096 {
        return;
    }
    recent.retain(|previous| previous != &path);
    recent.insert(0, path);
    recent.truncate(RECENT_COUNT_MAX);
    assert!(recent.len() <= RECENT_COUNT_MAX);
}

fn history_path() -> Result<PathBuf, String> {
    let directory = match std::env::var_os("XDG_STATE_HOME") {
        Some(path) if Path::new(&path).is_absolute() => PathBuf::from(path),
        _ => PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not configured")?)
            .join(".local/state"),
    };
    Ok(directory.join("spice-cad/recent-models.json"))
}

fn read_history(path: &Path) -> Result<Vec<PathBuf>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Cannot read recent files: {error}")),
    };
    let mut bytes = Vec::new();
    file.take(HISTORY_BYTES_MAX + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > HISTORY_BYTES_MAX {
        return Err("Recent-file history exceeds 128 KiB".into());
    }
    let paths: Vec<PathBuf> = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Cannot read recent files: {error}"))?;
    let mut recent = Vec::new();
    for path in paths.into_iter().take(RECENT_COUNT_MAX).rev() {
        if path.is_absolute() {
            remember(&mut recent, path);
        }
    }
    Ok(recent)
}

fn save_history(path: &Path, recent: &[PathBuf]) -> Result<(), String> {
    assert!(recent.len() <= RECENT_COUNT_MAX);
    let paths: Vec<_> = recent
        .iter()
        .filter(|path| path.to_str().is_some())
        .collect();
    let bytes = serde_json::to_vec(&paths).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > HISTORY_BYTES_MAX {
        return Err("Recent-file history exceeds 128 KiB".into());
    }
    std::fs::create_dir_all(path.parent().ok_or("History needs a directory")?)
        .map_err(|error| error.to_string())?;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temporary = path.with_file_name(format!(
        ".recent-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let written = file
        .write_all(&bytes)
        .and_then(|()| std::fs::rename(&temporary, path));
    if let Err(error) = written {
        let cleanup = std::fs::remove_file(&temporary);
        return Err(format!(
            "Cannot save recent files: {error}; cleanup: {cleanup:?}"
        ));
    }
    Ok(())
}

fn index(root: Option<&Path>, recent: &[PathBuf], cancelled: impl Fn() -> bool) -> Index {
    let mut files = Vec::new();
    for path in recent.iter().take(RECENT_COUNT_MAX) {
        if cancelled() {
            return Index {
                files,
                notice: None,
            };
        }
        if path.is_file() {
            files.push(File {
                path: path.clone(),
                label: root
                    .and_then(|root| path.strip_prefix(root).ok())
                    .unwrap_or(path)
                    .display()
                    .to_string(),
                recent: true,
            });
        }
    }
    let mut notice = None;
    if let Some(root) = root {
        let mut directories = VecDeque::from([(root.to_owned(), 0_u32)]);
        let mut count = 0;
        while let Some((directory, depth)) = directories.pop_front() {
            if cancelled() {
                break;
            }
            let entries = match std::fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) => {
                    notice = Some(format!("Cannot index {}: {error}", directory.display()));
                    continue;
                }
            };
            for entry in entries.take(ENTRY_COUNT_MAX - count) {
                count += 1;
                if cancelled() {
                    break;
                }
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        notice = Some(error.to_string());
                        continue;
                    }
                };
                let kind = match entry.file_type() {
                    Ok(kind) => kind,
                    Err(error) => {
                        notice = Some(error.to_string());
                        continue;
                    }
                };
                let path = entry.path();
                if path.as_os_str().len() > 4096 {
                    continue;
                }
                if kind.is_dir() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if !name.starts_with('.')
                        && !matches!(
                            name.as_ref(),
                            "target" | "output" | "result" | "node_modules" | "vendor"
                        )
                        && depth < 8
                        && directories.len() < 1024
                    {
                        directories.push_back((path, depth + 1));
                    }
                } else if kind.is_file()
                    && path
                        .extension()
                        .is_some_and(|extension| extension == "luau" || extension == "lua")
                {
                    if !files
                        .iter()
                        .take(RECENT_COUNT_MAX)
                        .any(|file| file.path == path)
                    {
                        files.push(File {
                            label: path
                                .strip_prefix(root)
                                .unwrap_or(&path)
                                .display()
                                .to_string(),
                            path,
                            recent: false,
                        });
                    }
                }
                if files.len() == FILE_COUNT_MAX {
                    break;
                }
            }
            if count == ENTRY_COUNT_MAX || files.len() == FILE_COUNT_MAX {
                notice = Some("Index limit reached · use Browse files for other models".into());
                break;
            }
        }
    }
    files.sort_by(|first, second| {
        second.recent.cmp(&first.recent).then_with(|| {
            if first.recent {
                std::cmp::Ordering::Equal
            } else {
                first.label.cmp(&second.label)
            }
        })
    });
    assert!(files.len() <= FILE_COUNT_MAX);
    Index { files, notice }
}

fn fuzzy_score(query: &str, label: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }
    let mut characters = label.chars().enumerate();
    let mut previous = None;
    let mut score = 0;
    for wanted in query.chars() {
        let (index, _) =
            characters.find(|(_, character)| character.to_lowercase().eq(wanted.to_lowercase()))?;
        score += match previous {
            Some(previous) => (index - previous - 1) as u32 * 8,
            None => index as u32 * 4,
        };
        previous = Some(index);
    }
    Some(score)
}

#[derive(Default)]
pub struct State {
    mode: Option<Mode>,
    query: String,
    focus: bool,
    selected: usize,
    rows: Vec<usize>,
    files: Vec<File>,
    recent: Vec<PathBuf>,
    root: PathBuf,
    worker: Option<Worker>,
    generation: u64,
    loading: bool,
    notice: Option<String>,
}

impl State {
    pub fn is_open(&self) -> bool {
        self.mode.is_some()
    }

    pub fn remember(&mut self, path: PathBuf) {
        remember(&mut self.recent, path);
        if let Some(worker) = &self.worker {
            self.generation = worker.submit(None, self.recent.clone());
        }
    }

    pub fn open(&mut self, mode: Mode, root: PathBuf, context: &egui::Context) {
        self.mode = Some(mode);
        self.query.clear();
        self.focus = true;
        self.selected = 0;
        self.rows.clear();
        if mode == Mode::Files {
            if self.root != root {
                self.files.clear();
            }
            self.root = root;
            self.notice = None;
            if self.worker.is_none() {
                match Worker::new(context.clone()) {
                    Ok(worker) => self.worker = Some(worker),
                    Err(error) => self.notice = Some(error),
                }
            }
            if let Some(worker) = &self.worker {
                self.generation = worker.submit(Some(self.root.clone()), self.recent.clone());
                self.loading = true;
            }
        }
        context.request_repaint();
    }

    pub fn show(
        &mut self,
        context: &egui::Context,
        availability: Availability,
    ) -> Option<Selection> {
        let mode = self.mode?;
        let mut refresh = self.focus || mode == Mode::Commands;
        if let Some(worker) = &self.worker {
            let result = worker
                .slots
                .0
                .lock()
                .expect("File index slots")
                .completed
                .take();
            if let Some((generation, index)) = result {
                if generation == self.generation {
                    self.files = index.files;
                    self.notice = index.notice;
                    self.loading = false;
                    refresh = true;
                }
            }
        }
        let (up, down, accept, cancel) = context.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        let mut selection = None;
        let response = egui::Modal::new(egui::Id::unique("navigation_palette"))
            .area(
                egui::Area::new(egui::Id::unique("navigation_palette"))
                    .order(egui::Order::Foreground)
                    .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 72.0)),
            )
            .show(context, |ui| {
                ui.set_width((context.content_rect().width() - 48.0).clamp(240.0, 640.0));
                ui.horizontal(|ui| {
                    interface::panel_title(
                        ui,
                        if mode == Mode::Files {
                            "QUICK OPEN"
                        } else {
                            "COMMANDS"
                        },
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new("Up/Down · Enter select · Esc close")
                                .small()
                                .color(interface::MUTED),
                        );
                    });
                });
                let input = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .id(egui::Id::unique("palette_query"))
                        .char_limit(256)
                        .desired_width(f32::INFINITY)
                        .margin(egui::vec2(8.0, 6.0))
                        .hint_text(if mode == Mode::Files {
                            "Search model files…"
                        } else {
                            "Search commands…"
                        }),
                );
                if self.focus {
                    input.request_focus();
                    self.focus = false;
                }
                if input.changed() {
                    self.selected = 0;
                    refresh = true;
                }
                if refresh {
                    let mut scored = Vec::new();
                    match mode {
                        Mode::Files => {
                            for (index, file) in self.files.iter().enumerate() {
                                let name = file
                                    .path
                                    .file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or(&file.label);
                                let filename_score = fuzzy_score(&self.query, name);
                                let score = filename_score
                                    .or_else(|| fuzzy_score(&self.query, &file.label));
                                if let Some(score) = score {
                                    scored.push((
                                        filename_score.is_none(),
                                        score,
                                        !file.recent,
                                        index,
                                    ));
                                }
                            }
                        }
                        Mode::Commands => {
                            for (index, &(command, label, _)) in COMMANDS.iter().enumerate() {
                                if availability.allows(command) {
                                    if let Some(score) = fuzzy_score(&self.query, label) {
                                        scored.push((false, score, false, index));
                                    }
                                }
                            }
                        }
                    }
                    scored.sort_unstable();
                    self.rows = scored
                        .into_iter()
                        .take(RESULT_COUNT_MAX)
                        .map(|(_, _, _, index)| index)
                        .collect();
                }
                if down {
                    self.selected = (self.selected + 1).min(self.rows.len().saturating_sub(1));
                }
                if up {
                    self.selected = self.selected.saturating_sub(1);
                }
                self.selected = self.selected.min(self.rows.len().saturating_sub(1));
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("palette_results")
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for (row, &index) in self.rows.iter().enumerate() {
                            let (label, shortcut) = if mode == Mode::Files {
                                (
                                    self.files[index].label.as_str(),
                                    if self.files[index].recent {
                                        "Recent"
                                    } else {
                                        ""
                                    },
                                )
                            } else {
                                (COMMANDS[index].1, COMMANDS[index].2)
                            };
                            let response = ui
                                .add(
                                    egui::Button::selectable(row == self.selected, label)
                                        .shortcut_text(shortcut)
                                        .truncate()
                                        .min_size(egui::vec2(
                                            ui.available_width(),
                                            interface::CONTROL_HEIGHT,
                                        )),
                                )
                                .on_hover_text(label);
                            if (up || down || refresh) && row == self.selected {
                                response.scroll_to_me(None);
                            }
                            if response.clicked() || accept && row == self.selected {
                                selection = Some(if mode == Mode::Files {
                                    Selection::File(self.files[index].path.clone())
                                } else {
                                    Selection::Command(COMMANDS[index].0)
                                });
                            }
                        }
                        if self.rows.is_empty() {
                            ui.label(if self.loading {
                                "Indexing model files…"
                            } else {
                                "No matching results"
                            });
                        }
                    });
                ui.separator();
                if mode == Mode::Files {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!(
                                "{} · {} models{}",
                                self.root.display(),
                                self.files.len(),
                                if self.loading { " · indexing…" } else { "" }
                            ))
                            .small()
                            .color(interface::MUTED),
                        )
                        .truncate(),
                    )
                    .on_hover_text(self.root.display().to_string());
                    if let Some(notice) = &self.notice {
                        ui.add(
                            egui::Label::new(egui::RichText::new(notice).color(interface::WARNING))
                                .truncate(),
                        )
                        .on_hover_text(notice);
                    }
                    if ui.button("Browse files…").clicked() {
                        selection = Some(Selection::Command(Command::Open));
                    }
                } else {
                    ui.label(
                        egui::RichText::new("Commands available in the current document")
                            .small()
                            .color(interface::MUTED),
                    );
                }
            });
        if cancel || response.should_close() {
            selection = Some(Selection::Cancel);
        }
        if selection.is_some() {
            self.mode = None;
            context.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(
                        event,
                        egui::Event::Key { .. }
                            | egui::Event::Text(_)
                            | egui::Event::Copy
                            | egui::Event::Cut
                            | egui::Event::Paste(_)
                    )
                })
            });
        }
        selection
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_keyboard_selection_and_cancel_do_not_edit_source() {
        let context = egui::Context::default();
        let mut state = State::default();
        let mut source = crate::text_buffer::Buffer::from("local width = 10\n");
        let mut editor = crate::code_editor::State::default();
        editor.modal.enable(true);
        let mut render = |state: &mut State, events| {
            let mut selection = None;
            context
                .run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1100.0, 650.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        selection = state.show(
                            ui.ctx(),
                            Availability {
                                source: true,
                                run: true,
                                export: false,
                                model: true,
                            },
                        );
                        crate::code_editor::show(ui, &mut source, &mut editor);
                    },
                )
                .drop_without_applying_deltas();
            selection
        };
        state.open(Mode::Commands, PathBuf::new(), &context);
        assert!(render(&mut state, Vec::new()).is_none());
        assert!(render(&mut state, vec![egui::Event::Text("run source".into())]).is_none());
        assert!(!state.rows.is_empty());
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        assert!(matches!(
            render(&mut state, vec![key(egui::Key::Enter)]),
            Some(Selection::Command(Command::Run))
        ));
        assert!(!state.is_open());
        state.open(Mode::Commands, PathBuf::new(), &context);
        render(&mut state, Vec::new());
        assert!(matches!(
            render(&mut state, vec![key(egui::Key::Escape)]),
            Some(Selection::Cancel)
        ));
        state.mode = Some(Mode::Files);
        state.query.clear();
        state.focus = true;
        state.files = vec![
            File {
                path: "/other/projects/model.luau".into(),
                label: "/other/projects/model.luau".into(),
                recent: true,
            },
            File {
                path: "/parts/other.luau".into(),
                label: "parts/other.luau".into(),
                recent: false,
            },
        ];
        render(&mut state, Vec::new());
        render(&mut state, vec![egui::Event::Text("other".into())]);
        assert_eq!(state.rows[0], 1);
        assert!(
            matches!(render(&mut state, vec![key(egui::Key::Enter)]), Some(Selection::File(path)) if path == Path::new("/parts/other.luau"))
        );
        assert_eq!(source.to_string(), "local width = 10\n");
    }

    #[test]
    fn fuzzy_matching_handles_gaps_unicode_and_missing_characters() {
        assert!(fuzzy_score("sr", "spice-rack.luau").is_some());
        assert!(fuzzy_score("É", "équerre.luau").is_some());
        assert!(fuzzy_score("xx", "export").is_none());
        assert!(fuzzy_score("rack", "rack.luau") < fuzzy_score("rack", "test/rack.luau"));
        assert_eq!(fuzzy_score("", "anything"), Some(0));
    }

    #[test]
    fn file_index_skips_generated_trees_and_preserves_recent_order()
    -> Result<(), Box<dyn std::error::Error>> {
        let folder = std::env::temp_dir().join(format!("spice-cad-palette-{}", std::process::id()));
        std::fs::create_dir_all(folder.join("target"))?;
        std::fs::create_dir_all(folder.join("parts"))?;
        for file in [
            "a.luau",
            "b.lua",
            "parts/c.luau",
            "target/hidden.luau",
            "ignore.txt",
        ] {
            std::fs::write(folder.join(file), "")?;
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&folder, folder.join("parts/loop"))?;
        let recent = vec![folder.join("b.lua"), folder.join("a.luau")];
        let files = index(Some(&folder), &recent, || false).files;
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, recent[0]);
        assert_eq!(files[1].path, recent[1]);
        assert_eq!(files[2].label, "parts/c.luau");
        assert!(index(Some(&folder), &[], || true).files.is_empty());
        let history = folder.join("state/recent.json");
        save_history(&history, &recent)?;
        assert_eq!(read_history(&history)?, recent);
        assert!(read_history(&folder.join("missing.json"))?.is_empty());
        std::fs::write(&history, "[")?;
        assert!(read_history(&history).is_err());
        let mut bounded = Vec::new();
        for index in 0..100 {
            remember(&mut bounded, folder.join(format!("{index}.luau")));
        }
        assert_eq!(bounded.len(), RECENT_COUNT_MAX);
        remember(&mut bounded, folder.join("99.luau"));
        assert_eq!(bounded.len(), RECENT_COUNT_MAX);
        std::fs::remove_dir_all(folder)?;
        Ok(())
    }
}
