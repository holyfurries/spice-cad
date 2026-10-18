use crate::text_buffer::{Buffer, Change};
use eframe::egui::{self, Color32, FontId, TextFormat, text::LayoutJob};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicU64, Ordering},
};
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator, Tree};

struct Job {
    revision: u64,
    source: ropey::Rope,
    change: Option<Change>,
}
struct Snapshot {
    revision: u64,
    source: ropey::Rope,
    tree: Tree,
    parameters: Vec<crate::parameters::Parameter>,
}
#[derive(Default)]
struct Slots {
    pending: Option<Job>,
    completed: Option<Snapshot>,
    stopped: bool,
}
struct Worker {
    slots: Arc<(Mutex<Slots>, Condvar)>,
    desired: Arc<AtomicU64>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    fn new(context: egui::Context) -> Result<Self, String> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_luau::LANGUAGE.into())
            .map_err(|error| error.to_string())?;
        let slots = Arc::new((Mutex::new(Slots::default()), Condvar::new()));
        let desired = Arc::new(AtomicU64::new(0));
        let shared = Arc::clone(&slots);
        let current = Arc::clone(&desired);
        let thread = std::thread::Builder::new()
            .name("cad-syntax".into())
            .spawn(move || {
                let mut previous: Option<Snapshot> = None;
                loop {
                    let job = {
                        let mut slots = shared.0.lock().expect("Syntax slots");
                        while slots.pending.is_none() && !slots.stopped {
                            slots = shared.1.wait(slots).expect("Syntax wake");
                        }
                        if slots.stopped {
                            break;
                        }
                        slots.pending.take().expect("Syntax job")
                    };
                    let mut old_tree = None;
                    if let (Some(previous), Some(change)) = (&previous, job.change) {
                        if previous.revision == change.previous_revision {
                            let mut tree = previous.tree.clone();
                            let point =
                                |point: (usize, usize)| tree_sitter::Point::new(point.0, point.1);
                            tree.edit(&tree_sitter::InputEdit {
                                start_byte: change.start_byte,
                                old_end_byte: change.old_end_byte,
                                new_end_byte: change.new_end_byte,
                                start_position: point(change.start_point),
                                old_end_position: point(change.old_end_point),
                                new_end_position: point(change.new_end_point),
                            });
                            old_tree = Some(tree);
                        }
                    }
                    let mut cancelled = |_: &tree_sitter::ParseState| {
                        current.load(Ordering::Acquire) != job.revision
                    };
                    let tree = parser.parse_with_options(
                        &mut |byte, _| {
                            if byte >= job.source.len_bytes() {
                                return &[][..];
                            }
                            let (chunk, start, _, _) = job.source.chunk_at_byte(byte);
                            &chunk.as_bytes()[byte - start..]
                        },
                        old_tree.as_ref(),
                        Some(tree_sitter::ParseOptions::new().progress_callback(&mut cancelled)),
                    );
                    let Some(tree) = tree else {
                        parser.reset();
                        continue;
                    };
                    if current.load(Ordering::Acquire) != job.revision {
                        continue;
                    }
                    previous = Some(Snapshot {
                        revision: job.revision,
                        source: job.source.clone(),
                        tree: tree.clone(),
                        parameters: Vec::new(),
                    });
                    let parameters = crate::parameters::extract(&job.source, &tree);
                    let mut slots = shared.0.lock().expect("Syntax slots");
                    if current.load(Ordering::Acquire) != job.revision {
                        continue;
                    }
                    slots.completed = Some(Snapshot {
                        revision: job.revision,
                        source: job.source,
                        tree,
                        parameters,
                    });
                    drop(slots);
                    context.request_repaint();
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            slots,
            desired,
            thread: Some(thread),
        })
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.desired.store(0, Ordering::Release);
        self.slots.0.lock().expect("Syntax slots").stopped = true;
        self.slots.1.notify_one();
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                eprintln!("Syntax: worker stopped unexpectedly");
            }
        }
    }
}

#[derive(Default)]
pub struct State {
    worker: Option<Worker>,
    snapshot: Option<Snapshot>,
    query: Option<Query>,
    requested: u64,
    pub error: Option<String>,
}
impl State {
    pub fn parameters(&self, revision: u64) -> Option<&[crate::parameters::Parameter]> {
        self.snapshot
            .as_ref()
            .filter(|snapshot| snapshot.revision == revision)
            .map(|snapshot| snapshot.parameters.as_slice())
    }
    pub fn sync(&mut self, source: &Buffer, context: &egui::Context) -> bool {
        if self.error.is_some() {
            return false;
        }
        if self.worker.is_none() {
            let initialized = (|| {
                self.query = Some(
                    Query::new(
                        &tree_sitter_luau::LANGUAGE.into(),
                        tree_sitter_luau::HIGHLIGHTS_QUERY,
                    )
                    .map_err(|error| error.to_string())?,
                );
                self.worker = Some(Worker::new(context.clone())?);
                Ok::<(), String>(())
            })();
            if let Err(error) = initialized {
                self.error = Some(error);
                return false;
            }
        }
        let worker = self.worker.as_ref().expect("Syntax worker");
        if self.requested != source.revision {
            worker.desired.store(source.revision, Ordering::Release);
            worker.slots.0.lock().expect("Syntax slots").pending = Some(Job {
                revision: source.revision,
                source: source.rope.clone(),
                change: source.change,
            });
            worker.slots.1.notify_one();
            self.requested = source.revision;
        }
        let completed = worker
            .slots
            .0
            .lock()
            .expect("Syntax slots")
            .completed
            .take();
        if let Some(snapshot) = completed {
            if snapshot.revision == source.revision {
                self.snapshot = Some(snapshot);
                return true;
            }
        }
        false
    }

    pub fn layout(
        &self,
        source: &Buffer,
        range: std::ops::Range<usize>,
        font: FontId,
    ) -> LayoutJob {
        let text = source.rope.byte_slice(range.clone()).to_string();
        let mut colors = vec![Color32::from_rgb(208, 211, 220); text.len()];
        if let (Some(snapshot), Some(query)) = (&self.snapshot, &self.query) {
            if snapshot.revision == source.revision {
                let mut cursor = QueryCursor::new();
                cursor.set_byte_range(range.clone());
                cursor.set_match_limit(2048);
                let mut captures = cursor.captures(
                    query,
                    snapshot.tree.root_node(),
                    |node: tree_sitter::Node<'_>| {
                        snapshot
                            .source
                            .byte_slice(node.byte_range())
                            .chunks()
                            .map(str::as_bytes)
                    },
                );
                let mut count = 0;
                while let Some((matched, index)) = captures.next() {
                    count += 1;
                    if count > 8192 {
                        break;
                    }
                    let capture = matched.captures[*index];
                    let name = query.capture_names()[capture.index as usize];
                    let color = if name.starts_with("comment") {
                        Color32::from_rgb(109, 137, 111)
                    } else if name.starts_with("string") {
                        Color32::from_rgb(206, 173, 132)
                    } else if name.starts_with("keyword") {
                        Color32::from_rgb(170, 153, 218)
                    } else if name.starts_with("number")
                        || name.starts_with("boolean")
                        || name.starts_with("constant")
                    {
                        Color32::from_rgb(147, 195, 200)
                    } else if name.starts_with("type") || name.starts_with("function") {
                        Color32::from_rgb(136, 179, 218)
                    } else {
                        continue;
                    };
                    let start = capture.node.start_byte().max(range.start) - range.start;
                    let end = capture
                        .node
                        .end_byte()
                        .min(range.end)
                        .saturating_sub(range.start);
                    if start < end && end <= colors.len() {
                        colors[start..end].fill(color);
                    }
                }
            }
        }
        let mut job = LayoutJob::default();
        let mut start = 0;
        for (offset, _) in text.char_indices().skip(1) {
            if colors[offset] != colors[start] {
                job.append(
                    &text[start..offset],
                    0.0,
                    TextFormat {
                        font_id: font.clone(),
                        color: colors[start],
                        ..Default::default()
                    },
                );
                start = offset;
            }
        }
        if !text.is_empty() {
            job.append(
                &text[start..],
                0.0,
                TextFormat {
                    font_id: font,
                    color: colors[start],
                    ..Default::default()
                },
            );
        }
        job
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parser_keeps_multiline_comments_and_incremental_edits_correct() {
        let mut buffer = Buffer::from("--[=[\nlocal not_code = 1\n]=]\nlocal value = 2\n");
        let mut state = State::default();
        let context = egui::Context::default();
        let mut wait = |buffer: &Buffer| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            loop {
                if state.sync(buffer, &context) {
                    break;
                }
                assert!(state.error.is_none(), "{:?}", state.error);
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            state.layout(
                buffer,
                buffer.rope.line_to_byte(1)..buffer.rope.line_to_byte(2),
                FontId::monospace(14.0),
            )
        };
        let job = wait(&buffer);
        assert!(
            job.sections
                .iter()
                .all(|section| section.format.color == Color32::from_rgb(109, 137, 111))
        );
        assert!(buffer.replace(0..2, "", 0));
        let job = wait(&buffer);
        assert_ne!(
            job.sections[0].format.color,
            Color32::from_rgb(109, 137, 111)
        );
    }
}
