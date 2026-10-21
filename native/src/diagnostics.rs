use crate::{analysis, lsp, text_buffer::Buffer};
use eframe::egui;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

const DEBOUNCE: Duration = Duration::from_millis(350);

struct Job {
    sequence: u64,
    query: lsp::Query,
    revision: u64,
    source: ropey::Rope,
}

struct Completion {
    revision: u64,
    result: Result<lsp::Checked, String>,
}

#[derive(Default)]
struct Slots {
    pending: Option<Job>,
    completed: Option<Completion>,
    stopped: bool,
}

struct Worker {
    sequence: Arc<AtomicU64>,
    process: Arc<Mutex<Option<std::process::Child>>>,
    slots: Arc<(Mutex<Slots>, Condvar)>,
    desired_revision: Arc<AtomicU64>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    fn new(path: std::path::PathBuf, context: egui::Context) -> Result<Self, String> {
        let slots = Arc::new((Mutex::new(Slots::default()), Condvar::new()));
        let desired_revision = Arc::new(AtomicU64::new(0));
        let sequence = Arc::new(AtomicU64::new(0));
        let desired_sequence = Arc::clone(&sequence);
        let shared = Arc::clone(&slots);
        let process = Arc::new(Mutex::new(None));
        let child = Arc::clone(&process);
        let desired = Arc::clone(&desired_revision);
        let thread = std::thread::Builder::new()
            .name("cad-analysis".into())
            .spawn(move || {
                let mut client = None;
                let mut dependencies = None;
                loop {
                    let job = {
                        let (mutex, wake) = &*shared;
                        let mut slots = mutex.lock().expect("Analysis slots");
                        while slots.pending.is_none() && !slots.stopped {
                            slots = wake.wait(slots).expect("Analysis wake");
                        }
                        if slots.stopped {
                            break;
                        }
                        slots.pending.take().expect("Pending analysis")
                    };
                    let cancelled = || {
                        desired.load(Ordering::Acquire) != job.revision
                            || desired_sequence.load(Ordering::Acquire) != job.sequence
                    };
                    let result = (|| {
                        let current_dependencies = crate::project::Project::load_if_imported(
                            &path,
                            &job.source.to_string(),
                        )
                        .ok()
                        .flatten()
                        .map(|project| project.dependency_fingerprint());
                        if dependencies != current_dependencies {
                            // Luau LSP 1.69 retains stale imported completions after file notifications.
                            client = None;
                            dependencies = current_dependencies;
                        }
                        if client.is_none() {
                            client = Some(lsp::Client::new(&path, &cancelled, Arc::clone(&child))?);
                        }
                        client.as_mut().expect("Language server").check(
                            &job.source,
                            job.revision,
                            job.query,
                            cancelled,
                        )
                    })();
                    if result.is_err() {
                        client = None;
                    }
                    if cancelled() {
                        continue;
                    }
                    let result = match result {
                        Ok(Some(report)) => Ok(report),
                        Ok(None) => continue,
                        Err(error) => Err(error),
                    };
                    let mut slots = shared.0.lock().expect("Analysis slots");
                    if cancelled() {
                        continue;
                    }
                    slots.completed = Some(Completion {
                        revision: job.revision,
                        result,
                    });
                    drop(slots);
                    context.request_repaint();
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            sequence,
            process,
            slots,
            desired_revision,
            thread: Some(thread),
        })
    }

    fn invalidate(&self, revision: u64) {
        assert!(revision > 0);
        self.desired_revision.store(revision, Ordering::Release);
        let mut slots = self.slots.0.lock().expect("Analysis slots");
        slots.pending = None;
        slots.completed = None;
    }

    fn submit(&self, buffer: &Buffer, query: lsp::Query) {
        assert_eq!(
            self.desired_revision.load(Ordering::Acquire),
            buffer.revision
        );
        let mut slots = self.slots.0.lock().expect("Analysis slots");
        let sequence = self.sequence.fetch_add(1, Ordering::AcqRel) + 1;
        slots.completed = None;
        slots.pending = Some(Job {
            sequence,
            query,
            revision: buffer.revision,
            source: buffer.rope.clone(),
        });
        self.slots.1.notify_one();
    }

    fn take(&self) -> Option<Completion> {
        self.slots
            .0
            .lock()
            .expect("Analysis slots")
            .completed
            .take()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.desired_revision.store(0, Ordering::Release);
        self.slots.0.lock().expect("Analysis slots").stopped = true;
        self.slots.1.notify_one();
        if let Some(child) = self.process.lock().expect("LSP process").as_mut() {
            let _ = child.kill();
        }
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                eprintln!("Analysis: worker panicked during shutdown");
            }
        }
    }
}

enum Phase {
    Waiting(Instant),
    Checking,
    Ready(Result<analysis::Report, String>),
}

pub struct Session {
    insight: Option<(u64, lsp::Insight)>,
    worker: Worker,
    revision: u64,
    phase: Phase,
    run_error: Option<analysis::Diagnostic>,
}

impl Session {
    pub fn new(path: std::path::PathBuf, context: egui::Context) -> Result<Self, String> {
        Ok(Self {
            worker: Worker::new(path, context)?,
            insight: None,
            revision: 0,
            phase: Phase::Waiting(Instant::now()),
            run_error: None,
        })
    }

    pub fn sync(&mut self, buffer: &Buffer, context: &egui::Context) {
        if self.revision != buffer.revision {
            self.revision = buffer.revision;
            self.worker.invalidate(buffer.revision);
            self.phase = Phase::Waiting(Instant::now());
            self.run_error = None;
            self.insight = None;
            context.request_repaint();
        }
        if let Some(completion) = self.worker.take() {
            if completion.revision == self.revision {
                self.phase = Phase::Ready(completion.result.map(|checked| {
                    self.insight = checked
                        .insight
                        .map(|insight| (completion.revision, insight));
                    checked.report
                }));
            }
        }
        if let Phase::Waiting(since) = self.phase {
            if since.elapsed() >= DEBOUNCE {
                self.worker.submit(buffer, lsp::Query::Check);
                self.phase = Phase::Checking;
            } else {
                context.request_repaint_after(DEBOUNCE.saturating_sub(since.elapsed()));
            }
        }
    }

    pub fn request(&mut self, buffer: &Buffer, query: lsp::Query, context: &egui::Context) {
        self.sync(buffer, context);
        self.worker.submit(buffer, query);
        self.insight = None;
        self.phase = Phase::Checking;
    }

    pub fn take_insight(&mut self) -> Option<(u64, lsp::Insight)> {
        self.insight.take()
    }

    pub fn navigate(&self, source: &Buffer, cursor: usize, next: bool) -> Option<analysis::Span> {
        let mut spans: Vec<_> = match &self.phase {
            Phase::Ready(Ok(report)) => report
                .diagnostics
                .iter()
                .filter_map(|diagnostic| diagnostic.span)
                .collect(),
            _ => Vec::new(),
        };
        if let Some(diagnostic) = &self.run_error {
            if let Some(span) = diagnostic.span {
                spans.push(span);
            }
        }
        spans.sort_by_key(|span| span.range(source).start);
        if next {
            spans
                .iter()
                .find(|span| span.range(source).start > cursor)
                .or_else(|| spans.first())
                .copied()
        } else {
            spans
                .iter()
                .rev()
                .find(|span| span.range(source).end < cursor)
                .or_else(|| spans.last())
                .copied()
        }
    }

    pub fn run_started(&mut self) {
        self.run_error = None;
    }

    pub fn run_failed(&mut self, revision: u64, message: &str) {
        if revision == self.revision && !message.starts_with("Luau type check failed:") {
            self.run_error = Some(analysis::Diagnostic::build_error(message));
        }
    }

    pub fn show(&self, ui: &mut egui::Ui) -> Option<analysis::Span> {
        let mut jump = None;
        ui.horizontal(|ui| {
            crate::interface::panel_title(ui, "DIAGNOSTICS");
            ui.small(match self.phase {
                Phase::Waiting(_) => "Waiting for typing pause…",
                Phase::Checking => "Checking types…",
                Phase::Ready(_) => "Current document",
            });
            ui.separator();
            ui.small("Type checking only · Run builds geometry");
        });
        egui::ScrollArea::vertical()
            .id_salt("diagnostics_list")
            .max_height(ui.available_height().max(0.0))
            .show(ui, |ui| {
                match &self.phase {
                    Phase::Ready(Ok(report)) => {
                        if report.diagnostics.is_empty() {
                            ui.colored_label(
                                crate::interface::ACCENT,
                                "No type errors or warnings",
                            );
                        }
                        for entry in &report.diagnostics {
                            if diagnostic_row(ui, entry) {
                                jump = entry.span;
                            }
                        }
                        if report.truncated {
                            ui.colored_label(
                                crate::interface::WARNING,
                                "Diagnostic list truncated; fix these issues and check again.",
                            );
                        }
                    }
                    Phase::Ready(Err(error)) => {
                        ui.colored_label(crate::interface::WARNING, error);
                    }
                    _ => {}
                }
                if let Some(entry) = &self.run_error {
                    if diagnostic_row(ui, entry) {
                        jump = entry.span;
                    }
                }
            });
        jump
    }

    pub fn status(&self) -> (String, egui::Color32) {
        let mut errors = usize::from(self.run_error.is_some());
        let mut warnings = 0;
        match &self.phase {
            Phase::Waiting(_) => return ("Check pending".into(), crate::interface::MUTED),
            Phase::Checking => return ("Checking types".into(), crate::interface::MUTED),
            Phase::Ready(Err(_)) => {
                return ("Checker unavailable".into(), crate::interface::WARNING);
            }
            Phase::Ready(Ok(report)) => {
                for diagnostic in &report.diagnostics {
                    match diagnostic.severity {
                        analysis::Severity::Error => errors += 1,
                        analysis::Severity::Warning => warnings += 1,
                    }
                }
            }
        }
        if errors > 0 {
            return (
                format!("{errors} errors · {warnings} warnings"),
                crate::interface::ERROR,
            );
        }
        if warnings > 0 {
            return (format!("{warnings} warnings"), crate::interface::WARNING);
        }
        ("No issues".into(), crate::interface::ACCENT)
    }
}

fn diagnostic_row(ui: &mut egui::Ui, entry: &analysis::Diagnostic) -> bool {
    let color = match entry.severity {
        analysis::Severity::Error => crate::interface::ERROR,
        analysis::Severity::Warning => crate::interface::WARNING,
    };
    let mut clicked = false;
    ui.horizontal_wrapped(|ui| {
        if let Some(span) = entry.span {
            clicked = ui
                .link(format!("{}:{}", span.start.line, span.start.column))
                .on_hover_text("Jump to source")
                .clicked();
        }
        ui.colored_label(color, format!("{}: {}", entry.code, entry.message));
    });
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imported_completion_refreshes_after_a_dependency_edit() -> Result<(), String> {
        let folder =
            std::env::temp_dir().join(format!("spice-module-completion-{}", std::process::id()));
        std::fs::create_dir(&folder).map_err(|error| error.to_string())?;
        let result = (|| {
            let component = folder.join("component.luau");
            std::fs::write(
                &component,
                "return {build=function(width: number): number return width end}",
            )
            .map_err(|error| error.to_string())?;
            let worker = Worker::new(folder.join("main.luau"), egui::Context::default())?;
            let complete = || -> Result<Vec<String>, String> {
                let source = "local component=require('./component')\ncomponent.";
                let buffer = Buffer::from(source);
                worker.invalidate(buffer.revision);
                worker.submit(&buffer, lsp::Query::Complete(source.len()));
                let deadline = Instant::now() + Duration::from_secs(3);
                for _ in 0..300 {
                    if let Some(completion) = worker.take() {
                        let Some(lsp::Insight::Completions(entries)) = completion.result?.insight
                        else {
                            return Err("Missing imported completions".into());
                        };
                        return Ok(entries.into_iter().map(|entry| entry.label).collect());
                    }
                    if Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err("Module completion timed out".into())
            };
            assert!(complete()?.iter().any(|label| label == "build"));
            std::fs::write(
                &component,
                "return {updated=function(width: number): number return width end}",
            )
            .map_err(|error| error.to_string())?;
            let labels = complete()?;
            assert!(labels.iter().any(|label| label == "updated"));
            assert!(!labels.iter().any(|label| label == "build"));
            Ok(())
        })();
        std::fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        result
    }

    #[test]
    fn diagnostic_navigation_skips_the_current_selection_and_wraps() -> Result<(), String> {
        let source = Buffer::from("--!strict\nfirst error\nsecond error\n");
        let mut session = Session::new(
            std::env::temp_dir().join("navigation.luau"),
            egui::Context::default(),
        )?;
        session.phase = Phase::Ready(Ok(analysis::Report {
            diagnostics: [3, 2]
                .map(|line| {
                    analysis::Diagnostic::build_error(&format!("model.luau:{line}: test error"))
                })
                .into(),
            truncated: false,
        }));
        let first = session.navigate(&source, 0, true).expect("First issue");
        assert_eq!(first.start.line, 2);
        let second = session
            .navigate(&source, first.range(&source).end, true)
            .expect("Next issue");
        assert_eq!(second.start.line, 3);
        assert_eq!(
            session
                .navigate(&source, second.range(&source).end, false)
                .expect("Previous issue")
                .start
                .line,
            2
        );
        assert_eq!(
            session
                .navigate(&source, first.range(&source).end, false)
                .expect("Wrap backward")
                .start
                .line,
            3
        );
        assert_eq!(
            session
                .navigate(&source, second.range(&source).end, true)
                .expect("Wrap forward")
                .start
                .line,
            2
        );
        Ok(())
    }

    #[test]
    fn latest_revision_replaces_pending_checks_and_cancels_running_work() -> Result<(), String> {
        let worker = Worker::new(
            std::env::temp_dir().join("cad-cancellation.luau"),
            egui::Context::default(),
        )?;
        let old = Buffer::from("local a: number = 'wrong'\n".repeat(100_000));
        worker.invalidate(old.revision);
        worker.submit(&old, lsp::Query::Check);
        std::thread::sleep(Duration::from_millis(15));
        let latest = Buffer::from("local value: number = 1");
        worker.invalidate(latest.revision);
        worker.submit(&latest, lsp::Query::Check);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(completion) = worker.take() {
                assert_eq!(completion.revision, latest.revision);
                assert!(completion.result?.report.error().is_none());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Latest check blocked behind stale work"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(worker.slots.0.lock().expect("Slots").pending.is_none());
        Ok(())
    }
}
