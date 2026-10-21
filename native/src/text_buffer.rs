use ropey::Rope;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};

pub const SIZE_BYTES_MAX: usize = 16 * 1024 * 1024;
#[derive(Clone, Copy)]
pub struct Change {
    pub previous_revision: u64,
    pub start_byte: usize,
    pub old_end_byte: usize,
    pub new_end_byte: usize,
    pub start_point: (usize, usize),
    pub old_end_point: (usize, usize),
    pub new_end_point: (usize, usize),
}
const HISTORY_SIZE_BYTES_MAX: usize = 2 * SIZE_BYTES_MAX;
const HISTORY_COUNT_MAX: usize = 1024;

struct Edit {
    before: Rope,
    after: Rope,
    before_revision: u64,
    after_revision: u64,
    before_cursor: usize,
    after_cursor: usize,
    size_bytes: usize,
}

pub struct Buffer {
    pub change: Option<Change>,
    pub rope: Rope,
    pub revision: u64,
    history: std::collections::VecDeque<Edit>,
    history_index: usize,
    history_size_bytes: usize,
}

pub(crate) fn next_revision() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl From<&str> for Buffer {
    fn from(source: &str) -> Self {
        assert!(source.len() <= SIZE_BYTES_MAX);
        Self {
            change: None,
            rope: Rope::from_str(source),
            revision: next_revision(),
            history: Default::default(),
            history_index: 0,
            history_size_bytes: 0,
        }
    }
}

impl From<String> for Buffer {
    fn from(source: String) -> Self {
        Self::from(source.as_str())
    }
}

impl std::fmt::Display for Buffer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.rope.fmt(formatter)
    }
}

impl Buffer {
    pub fn replace_snapshot(
        &mut self,
        revision: u64,
        replacement: Rope,
        before_cursor: usize,
        after_cursor: usize,
    ) -> bool {
        assert!(replacement.len_bytes() <= SIZE_BYTES_MAX);
        assert!(after_cursor <= replacement.len_chars());
        if self.revision != revision {
            return false;
        }
        let size_bytes = self.rope.len_bytes() + replacement.len_bytes();
        let before = std::mem::replace(&mut self.rope, replacement);
        let before_revision = self.revision;
        self.revision = next_revision();
        self.change = None;
        self.record(
            before,
            before_revision,
            before_cursor,
            after_cursor,
            size_bytes,
        );
        true
    }

    pub fn is_empty(&self) -> bool {
        self.rope.len_bytes() == 0
    }

    pub fn replace(&mut self, range: Range<usize>, inserted: &str, cursor: usize) -> bool {
        assert!(range.start <= range.end);
        assert!(range.end <= self.rope.len_chars());
        let removed_size = self.rope.char_to_byte(range.end) - self.rope.char_to_byte(range.start);
        let size_bytes = removed_size + inserted.len();
        if self.rope.len_bytes() - removed_size + inserted.len() > SIZE_BYTES_MAX {
            return false;
        }
        if range.is_empty() && inserted.is_empty() {
            return false;
        }
        let before = self.rope.clone();
        let point = |source: &Rope, index: usize| {
            let line = source.char_to_line(index);
            (line, source.char_to_byte(index) - source.line_to_byte(line))
        };
        let start_byte = self.rope.char_to_byte(range.start);
        let old_end_byte = self.rope.char_to_byte(range.end);
        let start_point = point(&self.rope, range.start);
        let old_end_point = point(&self.rope, range.end);
        self.rope.remove(range.clone());
        self.rope.insert(range.start, inserted);
        let before_revision = self.revision;
        self.revision = next_revision();
        self.change = Some(Change {
            previous_revision: before_revision,
            start_byte,
            old_end_byte,
            new_end_byte: start_byte + inserted.len(),
            start_point,
            old_end_point,
            new_end_point: point(&self.rope, range.start + inserted.chars().count()),
        });
        self.record(
            before,
            before_revision,
            cursor,
            range.start + inserted.chars().count(),
            size_bytes,
        );
        true
    }

    pub fn merge_undo(&mut self, revision: u64) {
        if self.history_index != self.history.len() || self.history.len() < 2 {
            return;
        }
        let previous = &self.history[self.history.len() - 2];
        let latest = self.history.back().expect("Latest edit");
        if previous.before_revision != revision || previous.after_revision != latest.before_revision
        {
            return;
        }
        let latest = self.history.pop_back().expect("Latest edit");
        let previous = self.history.back_mut().expect("Previous edit");
        previous.after = latest.after;
        previous.after_revision = latest.after_revision;
        previous.after_cursor = latest.after_cursor;
        previous.size_bytes += latest.size_bytes;
        self.history_index = self.history.len();
    }

    fn record(
        &mut self,
        before: Rope,
        before_revision: u64,
        before_cursor: usize,
        after_cursor: usize,
        size_bytes: usize,
    ) {
        while self.history.len() > self.history_index {
            self.history_size_bytes -= self.history.pop_back().expect("Redo entry").size_bytes;
        }
        self.history.push_back(Edit {
            before,
            after: self.rope.clone(),
            before_revision,
            after_revision: self.revision,
            before_cursor,
            after_cursor,
            size_bytes,
        });
        self.history_size_bytes += size_bytes;
        while self.history.len() > HISTORY_COUNT_MAX
            || self.history_size_bytes > HISTORY_SIZE_BYTES_MAX
        {
            self.history_size_bytes -= self.history.pop_front().expect("History entry").size_bytes;
        }
        self.history_index = self.history.len();
    }

    pub fn redo_change(&mut self) -> Option<usize> {
        let cursor = self.history.get(self.history_index)?.before_cursor;
        self.undo(true)?;
        Some(cursor.min(self.rope.len_chars()))
    }

    pub fn undo(&mut self, redo: bool) -> Option<usize> {
        self.change = None;
        let edit = if redo {
            let edit = self.history.get(self.history_index)?;
            self.history_index += 1;
            self.rope = edit.after.clone();
            self.revision = edit.after_revision;
            return Some(edit.after_cursor);
        } else {
            self.history_index.checked_sub(1)?
        };
        self.history_index = edit;
        let edit = &self.history[edit];
        self.rope = edit.before.clone();
        self.revision = edit.before_revision;
        Some(edit.before_cursor.min(self.rope.len_chars()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maximum_size_snapshot_replacement_retains_one_undo() {
        let mut buffer = Buffer::from("a".repeat(SIZE_BYTES_MAX));
        let revision = buffer.revision;
        let replacement = Rope::from_str(&"b".repeat(SIZE_BYTES_MAX));
        assert!(buffer.replace_snapshot(revision, replacement, 0, 0));
        assert_eq!(buffer.history.len(), 1);
        assert!(buffer.history_size_bytes <= HISTORY_SIZE_BYTES_MAX);
        assert_eq!(buffer.rope.char(0), 'b');
        assert_eq!(buffer.undo(false), Some(0));
        assert_eq!(buffer.rope.char(0), 'a');
        assert_eq!(buffer.revision, revision);
        assert!(buffer.undo(false).is_none());
        assert_eq!(buffer.undo(true), Some(0));
        assert_eq!(buffer.rope.char(SIZE_BYTES_MAX - 1), 'b');
    }

    #[test]
    fn edits_preserve_unicode_and_undo_identity_without_copying_document() {
        let mut buffer = Buffer::from("héllo\n世界\n");
        let revision = buffer.revision;
        assert!(buffer.replace(1..2, "λ", 2));
        assert_eq!(buffer.to_string(), "hλllo\n世界\n");
        assert_eq!(buffer.undo(false), Some(2));
        assert_eq!(buffer.revision, revision);
        assert_eq!(buffer.to_string(), "héllo\n世界\n");
        assert_eq!(buffer.undo(true), Some(2));
        assert_eq!(buffer.to_string(), "hλllo\n世界\n");
        assert!(buffer.replace(0..1, "H", 0));
        assert!(buffer.undo(true).is_none());
        assert!(buffer.undo(false).is_some());
        assert!(buffer.replace(0..0, "new branch", 0));
        assert!(buffer.undo(true).is_none());
    }

    #[test]
    fn rejected_edits_preserve_source_revision_and_history() {
        let mut buffer = Buffer::from("kept");
        let revision = buffer.revision;
        assert!(!buffer.replace(0..0, &"x".repeat(SIZE_BYTES_MAX), 0));
        assert_eq!(buffer.to_string(), "kept");
        assert_eq!(buffer.revision, revision);
        assert!(buffer.undo(false).is_none());
    }

    #[test]
    fn large_document_random_edits_and_history_stay_bounded() {
        let source = "local width = 100 -- λ\n".repeat(100_000);
        let started = std::time::Instant::now();
        let mut buffer = Buffer::from(source.as_str());
        let load = started.elapsed();
        let started = std::time::Instant::now();
        for index in 0..2000 {
            let position = index * 997 % buffer.rope.len_chars();
            assert!(buffer.replace(position..position, "x", position));
        }
        eprintln!(
            "100k lines: load {load:?}, 2000 distributed edits {:?}",
            started.elapsed()
        );
        assert_eq!(buffer.rope.len_lines(), 100_001);
        assert!(buffer.history.len() <= HISTORY_COUNT_MAX);
        assert!(buffer.history_size_bytes <= HISTORY_SIZE_BYTES_MAX);
        for _ in 0..HISTORY_COUNT_MAX {
            assert!(buffer.undo(false).is_some());
        }
        assert!(buffer.undo(false).is_none());
    }
}
