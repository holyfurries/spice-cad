use crate::text_buffer::{Buffer, SIZE_BYTES_MAX};
use eframe::egui;
use ropey::{Rope, RopeBuilder};
use std::{
    ops::Range,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

const QUERY_CHARACTERS_MAX: usize = 4096;

struct Matches {
    revision: u64,
    query: String,
    length: usize,
    starts: Arc<Vec<u32>>,
}

fn find(
    source: &Rope,
    revision: u64,
    query: &str,
    cancelled: impl Fn() -> bool,
) -> Option<Matches> {
    assert!(source.len_bytes() <= SIZE_BYTES_MAX);
    let needle: Vec<char> = query.chars().take(QUERY_CHARACTERS_MAX + 1).collect();
    assert!(needle.len() <= QUERY_CHARACTERS_MAX);
    let mut prefix = vec![0; needle.len()];
    let mut matched = 0;
    for index in 1..needle.len() {
        while matched > 0 && needle[index] != needle[matched] {
            matched = prefix[matched - 1];
        }
        if needle[index] == needle[matched] {
            matched += 1;
        }
        prefix[index] = matched;
    }
    let mut starts = Vec::new();
    if !needle.is_empty() {
        matched = 0;
        for (index, character) in source.chars().enumerate() {
            if index % 4096 == 0 && cancelled() {
                return None;
            }
            while matched > 0 && character != needle[matched] {
                matched = prefix[matched - 1];
            }
            if character == needle[matched] {
                matched += 1;
            }
            if matched == needle.len() {
                starts.push(u32::try_from(index + 1 - matched).expect("Source bounded to 16 MiB"));
                matched = 0;
            }
        }
    }
    if cancelled() {
        return None;
    }
    Some(Matches {
        revision,
        query: query.into(),
        length: needle.len(),
        starts: Arc::new(starts),
    })
}

fn replace(
    source: &Rope,
    matches: &Matches,
    replacement: &str,
    cancelled: impl Fn() -> bool,
) -> Result<Option<Rope>, String> {
    assert!(!matches.query.is_empty());
    assert!(source.len_bytes() <= SIZE_BYTES_MAX);
    let removed = matches
        .starts
        .len()
        .checked_mul(matches.query.len())
        .ok_or("Replacement size overflow")?;
    let inserted = matches
        .starts
        .len()
        .checked_mul(replacement.len())
        .ok_or("Replacement size overflow")?;
    let size = source
        .len_bytes()
        .checked_sub(removed)
        .and_then(|size| size.checked_add(inserted))
        .ok_or("Replacement size overflow")?;
    if size > SIZE_BYTES_MAX {
        return Err("Replacement exceeds the 16 MiB document limit; source preserved".into());
    }
    let mut builder = RopeBuilder::new();
    let mut previous = 0;
    for (index, start) in matches.starts.iter().enumerate() {
        if index % 4096 == 0 && cancelled() {
            return Ok(None);
        }
        let start = *start as usize;
        assert!(previous <= start);
        for chunk in source.slice(previous..start).chunks() {
            builder.append(chunk);
        }
        builder.append(replacement);
        previous = start + matches.length;
    }
    for chunk in source.slice(previous..).chunks() {
        builder.append(chunk);
    }
    if cancelled() {
        return Ok(None);
    }
    let result = builder.finish();
    assert_eq!(result.len_bytes(), size);
    Ok(Some(result))
}

enum Job {
    Find {
        source: Rope,
        revision: u64,
        query: String,
    },
    Replace {
        source: Rope,
        matches: Matches,
        replacement: String,
    },
}
enum Completed {
    Found(Matches),
    Replaced {
        revision: u64,
        source: Rope,
        cursor: usize,
        count: usize,
    },
}
#[derive(Default)]
struct Slots {
    pending: Option<(u64, Job)>,
    completed: Option<Result<Completed, String>>,
    stopped: bool,
}
struct Worker {
    slots: Arc<(Mutex<Slots>, Condvar)>,
    sequence: Arc<AtomicU64>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Worker {
    fn new(context: egui::Context) -> Result<Self, String> {
        let slots = Arc::new((Mutex::new(Slots::default()), Condvar::new()));
        let sequence = Arc::new(AtomicU64::new(0));
        let shared = Arc::clone(&slots);
        let desired = Arc::clone(&sequence);
        let thread = std::thread::Builder::new()
            .name("cad-search".into())
            .spawn(move || {
                loop {
                    let (sequence, job) = {
                        let mut slots = shared.0.lock().expect("Search slots");
                        while slots.pending.is_none() && !slots.stopped {
                            slots = shared.1.wait(slots).expect("Search wake");
                        }
                        if slots.stopped {
                            break;
                        }
                        slots.pending.take().expect("Search job")
                    };
                    let cancelled = || desired.load(Ordering::Acquire) != sequence;
                    let result = match job {
                        Job::Find {
                            source,
                            revision,
                            query,
                        } => find(&source, revision, &query, cancelled)
                            .map(|matches| Ok(Completed::Found(matches))),
                        Job::Replace {
                            source,
                            matches,
                            replacement,
                        } => match replace(&source, &matches, &replacement, cancelled) {
                            Ok(Some(source)) => Some(Ok(Completed::Replaced {
                                revision: matches.revision,
                                cursor: (matches.starts[0] as usize + replacement.chars().count())
                                    .min(source.len_chars()),
                                count: matches.starts.len(),
                                source,
                            })),
                            Ok(None) => None,
                            Err(error) => Some(Err(error)),
                        },
                    };
                    let mut slots = shared.0.lock().expect("Search slots");
                    if !cancelled() {
                        slots.completed = result;
                        context.request_repaint();
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            slots,
            sequence,
            thread: Some(thread),
        })
    }
    fn cancel(&self) {
        let mut slots = self.slots.0.lock().expect("Search slots");
        self.sequence.fetch_add(1, Ordering::AcqRel);
        slots.pending = None;
        slots.completed = None;
    }
    fn submit(&self, job: Job) {
        let mut slots = self.slots.0.lock().expect("Search slots");
        let sequence = self.sequence.fetch_add(1, Ordering::AcqRel) + 1;
        slots.pending = Some((sequence, job));
        slots.completed = None;
        self.slots.1.notify_one();
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel();
        self.slots.0.lock().expect("Search slots").stopped = true;
        self.slots.1.notify_one();
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                eprintln!("Search: worker stopped unexpectedly");
            }
        }
    }
}

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Finding,
    Ready(Matches),
    Replacing,
    Failed(String),
}

pub enum Command {
    Select(Range<usize>),
    Replace {
        range: Range<usize>,
        text: String,
    },
    Apply {
        revision: u64,
        source: Rope,
        cursor: usize,
    },
    Focus,
}
#[derive(Default)]
pub struct State {
    open: bool,
    replacing: bool,
    query: String,
    replacement: String,
    backward: bool,
    focus: bool,
    phase: Phase,
    requested: Option<(u64, String)>,
    worker: Option<Worker>,
    select_pending: Option<bool>,
    origin: usize,
    status: String,
}
impl State {
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn open(&mut self, backward: bool, replacing: bool, cursor: usize) {
        self.open = true;
        self.replacing = replacing;
        self.backward = backward;
        self.focus = true;
        self.origin = cursor;
        self.select_pending = if self.query.is_empty() {
            None
        } else {
            Some(backward)
        };
        self.requested = None;
    }
    pub fn next(&mut self, revision: u64, cursor: usize, opposite: bool) -> Option<Range<usize>> {
        self.origin = cursor;
        let backward = self.backward != opposite;
        match &self.phase {
            Phase::Ready(matches)
                if matches.revision == revision && matches.query == self.query =>
            {
                select(matches, cursor, backward)
            }
            _ => {
                self.select_pending = Some(backward);
                None
            }
        }
    }
    pub fn ranges(
        &self,
        revision: u64,
        range: Range<usize>,
    ) -> impl Iterator<Item = Range<usize>> + '_ {
        let (starts, length) = match &self.phase {
            Phase::Ready(matches) if self.open && matches.revision == revision => {
                (matches.starts.as_slice(), matches.length)
            }
            _ => (&[][..], 0),
        };
        let first =
            starts.partition_point(|start| (*start as usize).saturating_add(length) <= range.start);
        starts[first..]
            .iter()
            .take_while(move |start| (**start as usize) < range.end)
            .take(4096)
            .map(move |start| *start as usize..*start as usize + length)
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        source: &Buffer,
        cursor: usize,
        selection: Range<usize>,
    ) -> Option<Command> {
        let mut command = None;
        if self.open {
            let escape =
                ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            let mut navigate = false;
            let mut accept = false;
            let mut previous = false;
            let mut replace_one = false;
            let mut replace_all = false;
            let mut changed = false;
            ui.horizontal_wrapped(|ui| {
                ui.add_sized(
                    egui::vec2(44.0, crate::interface::CONTROL_HEIGHT),
                    egui::Label::new("Find"),
                )
                .on_hover_text(if self.backward {
                    "Search backward"
                } else {
                    "Search forward"
                });
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .id(egui::Id::unique("search_query"))
                        .char_limit(QUERY_CHARACTERS_MAX)
                        .desired_width(180.0)
                        .margin(egui::vec2(8.0, 6.0))
                        .hint_text("Literal text"),
                );
                if self.focus {
                    response.request_focus();
                    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                egui::text::CCursor::new(self.query.chars().count()),
                            )));
                        state.store(ui.ctx(), response.id);
                    }
                    self.focus = false;
                }
                changed = response.changed();
                accept =
                    response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                previous |= ui
                    .button("Previous")
                    .on_hover_text("Previous match")
                    .clicked();
                navigate |= ui.button("Next").on_hover_text("Next match").clicked();
                ui.toggle_value(&mut self.replacing, "Replace");
                if ui
                    .button("Close")
                    .on_hover_text("Esc · Return to code")
                    .clicked()
                    || escape
                {
                    self.open = false;
                    self.select_pending = None;
                    if matches!(self.phase, Phase::Replacing) {
                        self.worker.as_ref().expect("Search worker").cancel();
                        self.phase = Phase::Idle;
                        self.requested = None;
                    }
                    command = Some(Command::Focus);
                }
            });
            if self.replacing {
                ui.horizontal_wrapped(|ui| {
                    ui.add_sized(egui::vec2(44.0, crate::interface::CONTROL_HEIGHT), egui::Label::new("With"));
                    ui.add(egui::TextEdit::singleline(&mut self.replacement).id(egui::Id::unique("search_replacement")).char_limit(QUERY_CHARACTERS_MAX).desired_width(180.0).margin(egui::vec2(8.0, 6.0)));
                    let ready = matches!(&self.phase, Phase::Ready(matches) if matches.revision == source.revision && matches.query == self.query && !matches.starts.is_empty());
                    replace_one = ui.add_enabled(ready, egui::Button::new("Replace match")).clicked();
                    replace_all = ui.add_enabled(ready, egui::Button::new("Replace all")).clicked();
                });
            }
            if changed {
                self.select_pending = Some(self.backward);
            }
            if previous || navigate {
                if let Some(range) = self.next(source.revision, cursor, previous) {
                    command = Some(Command::Select(range));
                }
            }
            if accept {
                if matches!(&self.phase, Phase::Ready(matches) if matches.starts.is_empty()) {
                    self.focus = true;
                    self.status = "No matches".into();
                } else {
                    self.open = false;
                    command = Some(Command::Focus);
                }
            }
            if replace_one {
                if self.replacement == self.query {
                    self.status = "Replacement is unchanged".into();
                } else if let Phase::Ready(matches) = &self.phase {
                    if let Some(range) = replacement_range(matches, cursor, selection) {
                        self.status.clear();
                        command = Some(Command::Replace {
                            range,
                            text: self.replacement.clone(),
                        });
                    }
                }
            }
            if replace_all {
                if self.replacement == self.query {
                    self.status = "Replacement is unchanged".into();
                } else if let Phase::Ready(matches) =
                    std::mem::replace(&mut self.phase, Phase::Replacing)
                {
                    self.worker
                        .as_ref()
                        .expect("Search worker")
                        .submit(Job::Replace {
                            source: source.rope.clone(),
                            matches,
                            replacement: self.replacement.clone(),
                        });
                }
            }
        }
        let changed = !self
            .requested
            .as_ref()
            .is_some_and(|(revision, query)| *revision == source.revision && query == &self.query);
        if changed {
            if self
                .requested
                .as_ref()
                .is_some_and(|(_, query)| query != &self.query)
            {
                self.status.clear();
            }
            if self
                .requested
                .as_ref()
                .is_some_and(|(revision, _)| *revision != source.revision)
            {
                self.select_pending = None;
                self.origin = cursor;
            }
            self.requested = Some((source.revision, self.query.clone()));
            if self.query.is_empty() {
                if let Some(worker) = &self.worker {
                    worker.cancel();
                }
                self.phase = Phase::Idle;
            } else {
                if self.worker.is_none() {
                    match Worker::new(ui.ctx().clone()) {
                        Ok(worker) => self.worker = Some(worker),
                        Err(error) => self.phase = Phase::Failed(error),
                    }
                }
                if let Some(worker) = &self.worker {
                    worker.submit(Job::Find {
                        source: source.rope.clone(),
                        revision: source.revision,
                        query: self.query.clone(),
                    });
                    self.phase = Phase::Finding;
                }
            }
        }
        if let Some(worker) = &self.worker {
            let completed = worker
                .slots
                .0
                .lock()
                .expect("Search slots")
                .completed
                .take();
            if let Some(completed) = completed {
                match completed {
                    Ok(Completed::Found(matches))
                        if matches.revision == source.revision && matches.query == self.query =>
                    {
                        if let Some(backward) = self.select_pending.take() {
                            if let Some(range) = select(&matches, self.origin, backward) {
                                command = Some(Command::Select(range));
                            }
                        }
                        self.phase = Phase::Ready(matches);
                    }
                    Ok(Completed::Replaced {
                        revision,
                        source: replacement,
                        cursor,
                        count,
                    }) if revision == source.revision => {
                        self.status = format!("Replaced {count} matches · one undo");
                        command = Some(Command::Apply {
                            revision,
                            source: replacement,
                            cursor,
                        });
                        self.phase = Phase::Idle;
                    }
                    Err(error) => {
                        self.status = error;
                        self.phase = Phase::Idle;
                        self.requested = None;
                    }
                    _ => {}
                }
            }
        }
        if self.open {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(match &self.phase {
                        Phase::Idle => "Literal, case-sensitive search".into(),
                        Phase::Finding => "Searching…".into(),
                        Phase::Replacing => "Preparing replacements…".into(),
                        Phase::Failed(error) => error.clone(),
                        Phase::Ready(matches) => format!(
                            "{} matches · Enter to jump · Esc to return to code",
                            matches.starts.len()
                        ),
                    })
                    .small()
                    .color(crate::interface::MUTED),
                );
                if !self.status.is_empty() {
                    ui.small(&self.status);
                }
            });
            ui.separator();
        }
        command
    }
}

fn replacement_range(
    matches: &Matches,
    cursor: usize,
    selection: Range<usize>,
) -> Option<Range<usize>> {
    if selection.end - selection.start == matches.length {
        if matches
            .starts
            .binary_search(&(selection.start as u32))
            .is_ok()
        {
            return Some(selection);
        }
    }
    select(matches, cursor, false)
}

fn select(matches: &Matches, cursor: usize, backward: bool) -> Option<Range<usize>> {
    let index = if backward {
        matches
            .starts
            .partition_point(|start| (*start as usize + matches.length) < cursor)
            .checked_sub(1)
            .unwrap_or(matches.starts.len().saturating_sub(1))
    } else {
        let index = matches
            .starts
            .partition_point(|start| (*start as usize) < cursor);
        if index == matches.starts.len() {
            0
        } else {
            index
        }
    };
    matches
        .starts
        .get(index)
        .map(|start| *start as usize..*start as usize + matches.length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_search_preserves_unicode_across_chunks_and_newlines() {
        let text = "λ世界 width\n".repeat(2000);
        let source = Rope::from_str(&text);
        let boundary = source.byte_to_char(source.chunk_at_byte(0).0.len());
        let crossing = source.slice(boundary - 3..boundary + 6).to_string();
        for query in [
            "λ世",
            "width\nλ",
            "世界 w",
            "missing",
            "",
            crossing.as_str(),
        ] {
            let found = find(&source, 1, query, || false).expect("Not cancelled");
            let expected: Vec<u32> = if query.is_empty() {
                Vec::new()
            } else {
                text.match_indices(query)
                    .map(|(byte, _)| source.byte_to_char(byte) as u32)
                    .collect()
            };
            assert_eq!(*found.starts, expected, "{query:?}");
            for start in found.starts.iter() {
                assert_eq!(
                    source.slice(*start as usize..*start as usize + found.length),
                    query
                );
            }
        }
    }

    #[test]
    fn replace_all_is_one_undo_and_rejects_stale_results() -> Result<(), String> {
        let mut source = Buffer::from("λ aa aa\naa end");
        let revision = source.revision;
        let matches = find(&source.rope, revision, "aa", || false).expect("Search");
        let after = replace(&source.rope, &matches, "世界", || false)?.expect("Prepared");
        assert!(source.replace_snapshot(revision, after, 1, 4));
        assert_eq!(source.to_string(), "λ 世界 世界\n世界 end");
        assert_eq!(source.undo(false), Some(1));
        assert_eq!(source.revision, revision);
        assert_eq!(source.to_string(), "λ aa aa\naa end");
        assert!(source.undo(false).is_none());
        assert_eq!(source.undo(true), Some(4));
        assert_eq!(source.to_string(), "λ 世界 世界\n世界 end");
        let revision = source.revision;
        assert!(source.replace(0..1, "edited", 0));
        let current_revision = source.revision;
        assert!(!source.replace_snapshot(revision, Rope::from_str("obsolete"), 0, 0));
        assert_eq!(source.revision, current_revision);
        assert_eq!(source.to_string(), "edited 世界 世界\n世界 end");
        Ok(())
    }

    #[test]
    fn navigation_wraps_and_replacement_matches_do_not_overlap() -> Result<(), String> {
        let source = Rope::from_str("aaaaa λ aa");
        let matches = find(&source, 1, "aa", || false).expect("Search");
        assert_eq!(*matches.starts, vec![0, 2, 8]);
        assert_eq!(select(&matches, 2, false), Some(2..4));
        assert_eq!(select(&matches, 10, false), Some(0..2));
        assert_eq!(select(&matches, 0, true), Some(8..10));
        assert_eq!(select(&matches, 4, true), Some(0..2));
        assert_eq!(
            replace(&source, &matches, "", || false)?
                .expect("Prepared")
                .to_string(),
            "a λ "
        );
        assert!(find(&source, 1, "aa", || true).is_none());
        assert!(replace(&source, &matches, "b", || true)?.is_none());
        Ok(())
    }

    #[test]
    fn oversized_replacement_is_rejected_without_changing_source() {
        let source = Rope::from_str(&"a".repeat(1_000_000));
        let matches = find(&source, 1, "a", || false).expect("Search");
        assert!(replace(&source, &matches, "bbbbbbbbbbbbbbbbb", || false).is_err());
        assert_eq!(source.len_chars(), 1_000_000);
        assert_eq!(source.char(0), 'a');
    }

    #[test]
    fn replace_one_advances_past_inserted_matches() {
        let mut source = Buffer::from("a a a");
        let matches = find(&source.rope, source.revision, "a", || false).expect("Search");
        let range = replacement_range(&matches, 1, 0..1).expect("Current match");
        assert_eq!(range, 0..1);
        assert!(source.replace(range, "aa", 1));
        let matches = find(&source.rope, source.revision, "a", || false).expect("Updated search");
        assert_eq!(replacement_range(&matches, 2, 2..2), Some(3..4));
        assert_eq!(replacement_range(&matches, 4, 3..4), Some(3..4));
    }

    #[test]
    fn worker_publishes_only_the_latest_search() -> Result<(), String> {
        let worker = Worker::new(egui::Context::default())?;
        worker.submit(Job::Find {
            source: Rope::from_str(&"old ".repeat(500_000)),
            revision: 1,
            query: "old".into(),
        });
        worker.submit(Job::Find {
            source: Rope::from_str("new λ new"),
            revision: 2,
            query: "new".into(),
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            if let Some(result) = worker
                .slots
                .0
                .lock()
                .expect("Search slots")
                .completed
                .take()
            {
                let Completed::Found(matches) = result? else {
                    panic!("Find response");
                };
                assert_eq!(matches.revision, 2);
                assert_eq!(*matches.starts, vec![0, 6]);
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Search worker blocked"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok(())
    }
}
