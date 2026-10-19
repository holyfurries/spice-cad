use crate::model;
use std::{
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, SystemTime},
};

pub struct Document {
    pub path: PathBuf,
    pub buffer: crate::text_buffer::Buffer,
    saved_revision: Option<u64>,
    pub source: Option<String>,
    pub conflict: Option<String>,
    pub read_error: Option<String>,
    pub saved_at: Option<SystemTime>,
}

pub struct Watcher {
    pub events: mpsc::Receiver<()>,
    stop: mpsc::SyncSender<()>,
}

impl Watcher {
    pub fn new(path: PathBuf, context: eframe::egui::Context) -> Result<Self, String> {
        let (updates, events) = mpsc::sync_channel(1);
        let (stop, cancelled) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("cad-file-watch".into())
            .spawn(move || {
                let mut previous = None;
                let mut previous_metadata = None;
                loop {
                    let metadata = std::fs::metadata(&path)
                        .and_then(|metadata| Ok((metadata.len(), metadata.modified()?)))
                        .map_err(|error| error.to_string());
                    if previous_metadata.as_ref() != Some(&metadata) {
                        let current = model::read_source(&path);
                        if previous.as_ref() != Some(&current) {
                            match updates.try_send(()) {
                                Ok(()) => {
                                    previous = Some(current);
                                    previous_metadata = Some(metadata);
                                    context.request_repaint();
                                }
                                Err(mpsc::TrySendError::Full(())) => {}
                                Err(mpsc::TrySendError::Disconnected(())) => break,
                            }
                        } else {
                            previous_metadata = Some(metadata);
                        }
                    }
                    match cancelled.recv_timeout(Duration::from_millis(250)) {
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        _ => break,
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self { events, stop })
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        let _ = self.stop.try_send(());
    }
}

#[derive(Clone, Copy)]
pub enum Example {
    RackParts,
    RocketEngine,
    RevolvedBushing,
    EdgeChamfers,
    EdgeFillets,
    ExtrudedBracket,
    MeasuredBlock,
    Rack,
    MountingBracket,
}

pub fn example_model(example: Example) -> Result<PathBuf, String> {
    let directory = match std::env::var_os("XDG_DATA_HOME") {
        Some(directory) if PathBuf::from(&directory).is_absolute() => PathBuf::from(directory),
        _ => PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not configured")?)
            .join(".local/share"),
    };
    let (name, source) = match example {
        Example::RackParts => (
            "spice-rack.luau",
            include_str!("../../models/rack-parts.luau"),
        ),
        Example::RocketEngine => (
            "rocket-engine.luau",
            include_str!("../../models/rocket-engine.luau"),
        ),
        Example::RevolvedBushing => (
            "revolved-bushing.luau",
            include_str!("../../models/revolved-bushing.luau"),
        ),
        Example::EdgeChamfers => (
            "edge-chamfers.luau",
            include_str!("../../models/edge-chamfers.luau"),
        ),
        Example::EdgeFillets => (
            "edge-fillets.luau",
            include_str!("../../models/edge-fillets.luau"),
        ),
        Example::ExtrudedBracket => (
            "extruded-bracket.luau",
            include_str!("../../models/extruded-bracket.luau"),
        ),
        Example::MeasuredBlock => (
            "measured-block.luau",
            include_str!("../../models/measured-block.luau"),
        ),
        Example::Rack => ("rack.luau", include_str!("../../models/rack.luau")),
        Example::MountingBracket => (
            "mounting-bracket.luau",
            include_str!("../../models/mounting-bracket.luau"),
        ),
    };
    create_example_model(directory.join("spice-cad/models").join(name), source)
}

fn create_example_model(path: PathBuf, source: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(path.parent().ok_or("Model file needs a directory")?)
        .map_err(|error| format!("Cannot create model directory: {error}"))?;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => {
            if let Err(error) = file.write_all(source.as_bytes()) {
                let cleanup = std::fs::remove_file(&path);
                return Err(format!(
                    "Cannot create example model: {error}; cleanup: {cleanup:?}"
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(format!("Cannot create example model: {error}")),
    }
    Ok(path)
}

impl Document {
    pub fn draft(path: PathBuf) -> Self {
        let mut document = Self::new(path);
        document.buffer = "--!strict\nlocal width: number = cad.number(20, {label = \"Width\", unit = \"mm\"})\nlocal depth: number = cad.number(20, {label = \"Depth\", unit = \"mm\"})\nlocal height: number = cad.number(20, {label = \"Height\", unit = \"mm\"})\ncad.show(\"new model\", cad.box({ width = width, depth = depth, height = height }))\n".into();
        document.saved_revision = None;
        document
    }

    pub fn loaded(path: PathBuf, source: String) -> Self {
        let mut document = Self::new(path);
        document.buffer = source.as_str().into();
        document.saved_revision = Some(document.buffer.revision);
        document.source = Some(source);
        document
    }

    pub fn new(path: PathBuf) -> Self {
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        Self {
            path,
            buffer: "".into(),
            saved_revision: Some(0),
            source: None,
            conflict: None,
            read_error: None,
            saved_at: None,
        }
    }

    pub fn edited(&self) -> bool {
        if self.saved_revision.is_none() {
            return true;
        }
        if self.source.is_none() {
            return !self.buffer.is_empty();
        }
        Some(self.buffer.revision) != self.saved_revision
    }

    pub fn source_ready(&self) -> bool {
        self.source.is_some() || self.edited()
    }

    pub fn poll(&mut self) -> bool {
        if self.saved_revision.is_none() {
            match self.path.try_exists() {
                Ok(false) => {
                    self.read_error = None;
                    return false;
                }
                Ok(true) => {}
                Err(error) => {
                    self.read_error = Some(error.to_string());
                    return false;
                }
            }
        }
        match model::read_source(&self.path) {
            Ok(source) => {
                let recovered = self.read_error.take().is_some();
                if self.source.as_ref() == Some(&source) {
                    self.conflict = None;
                    return recovered && !self.edited();
                }
                if self.edited() {
                    self.conflict = Some(source);
                    return false;
                }
                self.saved_at = if self.source.is_some() {
                    std::fs::metadata(&self.path)
                        .and_then(|metadata| metadata.modified())
                        .ok()
                } else {
                    None
                };
                self.buffer = source.as_str().into();
                self.saved_revision = Some(self.buffer.revision);
                self.source = Some(source);
                self.conflict = None;
                true
            }
            Err(error) => {
                self.read_error = Some(error);
                false
            }
        }
    }

    pub fn reload(&mut self) -> Result<(), String> {
        let source = model::read_source(&self.path)?;
        self.buffer = source.as_str().into();
        self.saved_revision = Some(self.buffer.revision);
        self.source = Some(source);
        self.conflict = None;
        self.read_error = None;
        Ok(())
    }

    pub fn keep_editor(&mut self) {
        if let Some(source) = self.conflict.take() {
            self.source = Some(source);
            self.saved_revision = Some(0);
        }
    }

    pub fn save(&mut self) -> Result<(), String> {
        if self.path.exists() {
            let current = model::read_source(&self.path)?;
            if self.source.as_ref() != Some(&current) {
                self.conflict = Some(current);
                return Err(
                    "File changed on disk. Choose Reload disk or Keep editor before saving.".into(),
                );
            }
        }
        let source = write_source(&self.path, &self.buffer.rope, self.source.as_deref())?;
        self.accept_save(self.path.clone(), source, self.buffer.revision);
        Ok(())
    }

    pub fn accept_save(&mut self, path: PathBuf, source: String, revision: u64) {
        self.path = path;
        self.source = Some(source);
        self.saved_revision = Some(revision);
        self.conflict = None;
        self.read_error = None;
        self.saved_at = Some(SystemTime::now());
    }
}

pub fn write_source(
    path: &std::path::Path,
    source: &ropey::Rope,
    expected: Option<&str>,
) -> Result<String, String> {
    assert!(source.len_bytes() <= crate::text_buffer::SIZE_BYTES_MAX);
    let replacing = path.try_exists().map_err(|error| error.to_string())?;
    if replacing {
        let current = model::read_source(path)?;
        if expected != Some(current.as_str()) {
            return Err("Target changed on disk; review the destination again".into());
        }
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let file_name = path
        .file_name()
        .ok_or("Model file needs a name")?
        .to_string_lossy();
    let temporary = path.with_file_name(format!(
        ".{file_name}.spice-cad-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let saved = (|| -> std::io::Result<()> {
        for chunk in source.chunks() {
            file.write_all(chunk.as_bytes())?;
        }
        file.sync_all()?;
        if replacing {
            let current = model::read_source(path).map_err(std::io::Error::other)?;
            if expected != Some(current.as_str()) {
                return Err(std::io::Error::other("Target changed during save"));
            }
            std::fs::rename(&temporary, path)
        } else {
            std::fs::hard_link(&temporary, path)?;
            std::fs::remove_file(&temporary)
        }
    })();
    if let Err(error) = saved {
        if let Err(cleanup) = std::fs::remove_file(&temporary) {
            return Err(format!(
                "Save failed: {error}; temporary file cleanup failed: {cleanup}"
            ));
        }
        return Err(format!("Save failed: {error}"));
    }
    Ok(source.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_drafts_are_unsaved_until_save_and_never_overwrite_an_existing_file() -> Result<(), String>
    {
        let folder = std::env::temp_dir().join(format!("spice-cad-draft-{}", std::process::id()));
        std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
        let path = folder.join("new.luau");
        let mut draft = Document::draft(path.clone());
        assert!(draft.edited());
        assert!(draft.source_ready());
        assert!(!draft.poll());
        assert!(draft.read_error.is_none());
        assert!(!path.exists());
        draft.save()?;
        assert!(!draft.edited());
        assert_eq!(model::read_source(&path)?, draft.buffer.to_string());
        let mut empty = Document::draft(folder.join("empty.luau"));
        assert!(
            empty
                .buffer
                .replace(0..empty.buffer.rope.len_chars(), "", 0)
        );
        assert!(empty.buffer.is_empty());
        assert!(empty.source_ready());
        assert!(empty.edited());
        assert!(!Document::new(folder.join("missing.luau")).source_ready());
        let mut collision = Document::draft(path.clone());
        assert!(collision.save().is_err());
        assert!(collision.edited());
        assert_eq!(model::read_source(&path)?, draft.buffer.to_string());
        std::fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn save_as_preserves_undo_and_routes_future_saves_to_the_new_path() -> Result<(), String> {
        let folder = std::env::temp_dir().join(format!("spice-cad-copy-{}", std::process::id()));
        std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
        let original = folder.join("original.luau");
        let copy = folder.join("copy.luau");
        std::fs::write(&original, "local width = 20").map_err(|error| error.to_string())?;
        let mut document = Document::loaded(original.clone(), "local width = 20".into());
        assert!(document.buffer.replace(14..16, "30", 14));
        let revision = document.buffer.revision;
        let source = write_source(&copy, &document.buffer.rope, None)?;
        document.accept_save(copy.clone(), source, revision);
        assert!(!document.edited());
        assert_eq!(document.buffer.undo(false), Some(14));
        assert!(document.edited());
        document.save()?;
        assert_eq!(model::read_source(&original)?, "local width = 20");
        assert_eq!(model::read_source(&copy)?, "local width = 20");
        assert!(document.buffer.undo(true).is_some());
        assert!(document.edited());
        document.save()?;
        assert_eq!(model::read_source(&copy)?, "local width = 30");
        assert_eq!(model::read_source(&original)?, "local width = 20");
        std::fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn watcher_is_quiet_until_source_changes_and_recovers_after_deletion() -> Result<(), String> {
        let folder = std::env::temp_dir().join(format!("spice-cad-watch-{}", std::process::id()));
        std::fs::create_dir(&folder).map_err(|error| error.to_string())?;
        let path = folder.join("model.luau");
        std::fs::write(&path, "local width = 10").map_err(|error| error.to_string())?;
        let watcher = Watcher::new(path.clone(), eframe::egui::Context::default())?;
        let mut document = Document::new(path.clone());
        watcher
            .events
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        assert!(document.poll());
        assert_eq!(
            watcher.events.recv_timeout(Duration::from_millis(300)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        std::fs::write(&path, "local width = 20").map_err(|error| error.to_string())?;
        watcher
            .events
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        assert!(document.poll());
        assert_eq!(document.buffer.to_string(), "local width = 20");
        std::fs::remove_file(&path).map_err(|error| error.to_string())?;
        watcher
            .events
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        assert!(!document.poll());
        assert!(document.read_error.is_some());
        std::fs::write(&path, "local width = 30").map_err(|error| error.to_string())?;
        watcher
            .events
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        assert!(document.poll());
        assert!(document.read_error.is_none());
        assert_eq!(document.buffer.to_string(), "local width = 30");
        drop(watcher);
        std::fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn installed_example_is_embedded_and_never_overwrites_user_edits() -> Result<(), String> {
        let folder = std::env::temp_dir().join(format!("spice-cad-example-{}", std::process::id()));
        let path = folder.join("models/example.luau");
        let source = include_str!("../../models/rack.luau");
        assert_eq!(create_example_model(path.clone(), source)?, path);
        assert_eq!(model::read_source(&path)?, source);
        std::fs::write(&path, "user edits").map_err(|error| error.to_string())?;
        create_example_model(path.clone(), source)?;
        assert_eq!(model::read_source(&path)?, "user edits");
        std::fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn editor_preserves_local_and_external_edits_and_saves_atomically() -> Result<(), String> {
        let folder =
            std::env::temp_dir().join(format!("spice-cad-document-{}", std::process::id()));
        std::fs::create_dir(&folder).map_err(|error| error.to_string())?;
        let path = folder.join("model.luau");
        std::fs::write(&path, "local width = 10").map_err(|error| error.to_string())?;
        let mut document = Document::new(path.clone());
        assert!(document.poll());
        document.buffer = "local width = 20".into();
        std::fs::write(&path, "local width = 30").map_err(|error| error.to_string())?;
        assert!(!document.poll());
        assert_eq!(document.buffer.to_string(), "local width = 20");
        assert!(document.save().is_err());
        assert_eq!(model::read_source(&path)?, "local width = 30");
        document.keep_editor();
        document.save()?;
        assert_eq!(model::read_source(&path)?, "local width = 20");
        assert!(!document.edited());
        document.buffer = "local width = 40".into();
        document.reload()?;
        assert_eq!(document.buffer.to_string(), "local width = 20");
        assert_eq!(
            std::fs::read_dir(&folder)
                .map_err(|error| error.to_string())?
                .count(),
            1
        );
        std::fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        Ok(())
    }
}
