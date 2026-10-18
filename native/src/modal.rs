use crate::text_buffer::{Buffer, SIZE_BYTES_MAX};
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Default)]
pub enum Mode {
    Normal,
    #[default]
    Insert,
    Visual,
    VisualLine,
    Command,
}
#[derive(Clone, Copy)]
enum Operator {
    Delete,
    Change,
    Yank,
}
#[derive(Clone, Copy, Default)]
enum Pending {
    #[default]
    None,
    Go,
    Replace,
    Operator(Operator, usize),
    Leader,
    LeaderFiles,
    LeaderDiagnostics,
    LeaderTrouble,
}
#[derive(Debug)]
pub enum Action {
    Save,
    Close,
    SaveClose,
    Open(Option<String>),
    QuickOpen,
    Run,
    Complete,
    Hover,
    Definition,
    ToggleDiagnostics,
    NextDiagnostic,
    PreviousDiagnostic,
    Find { backward: bool, replace: bool },
    FindNext(bool),
}
#[derive(Default)]
pub struct State {
    pub enabled: bool,
    pub mode: Mode,
    pending: Pending,
    count: usize,
    register: String,
    linewise: bool,
    pub command: String,
    pub error: Option<&'static str>,
}
impl State {
    pub fn enable(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.mode = if enabled { Mode::Normal } else { Mode::Insert };
        self.pending = Pending::None;
        self.count = 0;
        self.command.clear();
    }
    pub fn label(&self) -> &'static str {
        if !self.enabled {
            return "STANDARD";
        }
        match self.mode {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Visual => "VISUAL",
            Mode::VisualLine => "V-LINE",
            Mode::Command => "COMMAND",
        }
    }
    pub fn selection(&self, source: &Buffer, cursor: usize, anchor: usize) -> Range<usize> {
        match self.mode {
            Mode::Visual if self.enabled => {
                cursor.min(anchor)..(cursor.max(anchor) + 1).min(source.rope.len_chars())
            }
            Mode::VisualLine if self.enabled => {
                let first = source.rope.char_to_line(cursor.min(anchor));
                let last = source.rope.char_to_line(cursor.max(anchor));
                source.rope.line_to_char(first)..if last + 1 < source.rope.len_lines() {
                    source.rope.line_to_char(last + 1)
                } else {
                    source.rope.len_chars()
                }
            }
            _ => cursor.min(anchor)..cursor.max(anchor),
        }
    }
    pub fn escape(&mut self, source: &Buffer, cursor: &mut usize, anchor: &mut usize) {
        if self.mode == Mode::Insert
            && *cursor > source.rope.line_to_char(source.rope.char_to_line(*cursor))
        {
            *cursor -= 1;
        }
        self.mode = Mode::Normal;
        *anchor = *cursor;
        self.pending = Pending::None;
        self.count = 0;
        self.command.clear();
    }
    pub fn command_enter(&mut self) -> Option<Action> {
        let command = self.command.trim();
        let action = match command {
            "w" => Some(Action::Save),
            "q" => Some(Action::Close),
            "wq" | "x" => Some(Action::SaveClose),
            "run" => Some(Action::Run),
            "e" => Some(Action::Open(None)),
            command if command.starts_with("e ") => {
                Some(Action::Open(Some(command[2..].trim().to_owned())))
            }
            _ => {
                self.error = Some("Supported commands: :w :q :wq :e [path] :run");
                None
            }
        };
        self.command.clear();
        self.mode = Mode::Normal;
        action
    }
    pub fn text(
        &mut self,
        text: &str,
        source: &mut Buffer,
        cursor: &mut usize,
        anchor: &mut usize,
    ) -> Option<Action> {
        for (offset, character) in text.char_indices().take(4096) {
            if self.mode == Mode::Command {
                if self.command.len() < 4096 {
                    self.command.push(character);
                }
                continue;
            }
            if self.mode == Mode::Insert {
                let text = &text[offset..];
                let range = (*cursor).min(*anchor)..(*cursor).max(*anchor);
                if source.replace(range.clone(), text, *cursor) {
                    *cursor = range.start + text.chars().count();
                    *anchor = *cursor;
                } else {
                    self.error = Some("Edit exceeds the document limit");
                }
                return None;
            }
            self.error = None;
            if matches!(self.pending, Pending::Replace) {
                self.pending = Pending::None;
                let visual = matches!(self.mode, Mode::Visual | Mode::VisualLine);
                let range = if visual {
                    self.selection(source, *cursor, *anchor)
                } else {
                    let (_, end) = line_bounds(source, *cursor);
                    let count = self.count.max(1);
                    if count > end.saturating_sub(*cursor) {
                        self.count = 0;
                        continue;
                    }
                    *cursor..*cursor + count
                };
                self.count = 0;
                let mut replacement = String::with_capacity(range.len().min(SIZE_BYTES_MAX));
                for original in source.rope.slice(range.clone()).chars() {
                    replacement.push(if matches!(original, '\n' | '\r') {
                        original
                    } else {
                        character
                    });
                    if replacement.len() > SIZE_BYTES_MAX {
                        break;
                    }
                }
                if replacement.len() <= SIZE_BYTES_MAX
                    && source.replace(range.clone(), &replacement, *cursor)
                {
                    *cursor = range.start;
                    *anchor = *cursor;
                    self.mode = Mode::Normal;
                } else {
                    self.error = Some("Replacement exceeds the document limit");
                }
                continue;
            }
            if character.is_ascii_digit() && (character != '0' || self.count > 0) {
                self.count =
                    (self.count * 10 + character.to_digit(10).expect("Digit") as usize).min(10_000);
                continue;
            }
            let count = self.count.max(1);
            let counted = self.count > 0;
            self.count = 0;
            let pending = std::mem::take(&mut self.pending);
            match pending {
                Pending::Leader => {
                    match character {
                        ' ' => return Some(Action::Run),
                        'p' => self.pending = Pending::LeaderFiles,
                        'v' => self.pending = Pending::LeaderDiagnostics,
                        'x' => self.pending = Pending::LeaderTrouble,
                        _ => {}
                    }
                    continue;
                }
                Pending::LeaderFiles => {
                    if character == 'f' {
                        return Some(Action::QuickOpen);
                    }
                    continue;
                }
                Pending::LeaderDiagnostics => {
                    if character == 'd' {
                        return Some(Action::ToggleDiagnostics);
                    }
                    continue;
                }
                Pending::LeaderTrouble => {
                    if character == 'x' {
                        return Some(Action::ToggleDiagnostics);
                    }
                    continue;
                }
                _ => {}
            }
            if let Pending::Go = pending {
                if character == 'd' {
                    return Some(Action::Definition);
                }
                if character == 'g' {
                    *cursor = source
                        .rope
                        .line_to_char((count - 1).min(source.rope.len_lines() - 1));
                    if !matches!(self.mode, Mode::Visual | Mode::VisualLine) {
                        *anchor = *cursor;
                    }
                }
                continue;
            }
            if let Pending::Operator(operator, leading) = pending {
                let count = leading.saturating_mul(count).min(10_000);
                if matches!(operator, Operator::Change)
                    && character == 'w'
                    && *cursor < source.rope.len_chars()
                    && !source.rope.char(*cursor).is_whitespace()
                {
                    let mut end = *cursor;
                    for index in 0..count {
                        if index > 0 {
                            end += 1;
                            while end < source.rope.len_chars()
                                && source.rope.char(end).is_whitespace()
                            {
                                end += 1;
                            }
                        }
                        if end >= source.rope.len_chars() {
                            break;
                        }
                        let category = class(source.rope.char(end));
                        while end + 1 < source.rope.len_chars()
                            && class(source.rope.char(end + 1)) == category
                        {
                            end += 1;
                        }
                    }
                    self.apply(
                        operator,
                        *cursor..(end + 1).min(source.rope.len_chars()),
                        false,
                        source,
                        cursor,
                        anchor,
                    );
                    continue;
                }
                if matches!(
                    (operator, character),
                    (Operator::Delete, 'd') | (Operator::Change, 'c') | (Operator::Yank, 'y')
                ) {
                    let line = source.rope.char_to_line(*cursor);
                    let end = (line + count).min(source.rope.len_lines());
                    let range = source.rope.line_to_char(line)..if end == source.rope.len_lines() {
                        source.rope.len_chars()
                    } else {
                        source.rope.line_to_char(end)
                    };
                    self.apply(operator, range, true, source, cursor, anchor);
                } else if let Some((target, inclusive, linewise)) =
                    motion(source, *cursor, character, count, counted)
                {
                    let range = if linewise {
                        let first = source.rope.char_to_line((*cursor).min(target));
                        let last = source.rope.char_to_line((*cursor).max(target));
                        source.rope.line_to_char(first)..if last + 1 == source.rope.len_lines() {
                            source.rope.len_chars()
                        } else {
                            source.rope.line_to_char(last + 1)
                        }
                    } else if target >= *cursor {
                        *cursor..(target + usize::from(inclusive)).min(source.rope.len_chars())
                    } else {
                        target..*cursor
                    };
                    self.apply(operator, range, linewise, source, cursor, anchor);
                }
                continue;
            }
            match character {
                'r' => {
                    self.pending = Pending::Replace;
                    self.count = count;
                }
                '/' | '?' => {
                    return Some(Action::Find {
                        backward: character == '?',
                        replace: false,
                    });
                }
                'n' | 'N' => return Some(Action::FindNext(character == 'N')),
                ' ' => self.pending = Pending::Leader,
                '-' => return Some(Action::Open(None)),
                ':' => {
                    self.mode = Mode::Command;
                    self.command.clear();
                }
                'g' => {
                    self.pending = Pending::Go;
                    self.count = if counted { count } else { 0 };
                }
                'i' | 'a' | 'I' | 'A' => {
                    let (start, end) = line_bounds(source, *cursor);
                    *cursor = match character {
                        'a' => (*cursor + 1).min(end),
                        'I' => first_nonblank(source, start, end),
                        'A' => end,
                        _ => *cursor,
                    };
                    *anchor = *cursor;
                    self.mode = Mode::Insert;
                }
                'o' | 'O' => {
                    let (start, end) = line_bounds(source, *cursor);
                    let insertion = if character == 'o' { end } else { start };
                    if source.replace(insertion..insertion, "\n", *cursor) {
                        *cursor = insertion + usize::from(character == 'o');
                        *anchor = *cursor;
                        self.mode = Mode::Insert;
                    }
                }
                'v' | 'V' => {
                    let mode = if character == 'v' {
                        Mode::Visual
                    } else {
                        Mode::VisualLine
                    };
                    self.mode = if self.mode == mode {
                        Mode::Normal
                    } else {
                        mode
                    };
                    *anchor = *cursor;
                }
                'd' | 'c' | 'y' => {
                    let operator = match character {
                        'd' => Operator::Delete,
                        'c' => Operator::Change,
                        _ => Operator::Yank,
                    };
                    if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
                        self.apply(
                            operator,
                            self.selection(source, *cursor, *anchor),
                            self.mode == Mode::VisualLine,
                            source,
                            cursor,
                            anchor,
                        );
                    } else {
                        self.pending = Pending::Operator(operator, count);
                    }
                }
                'x' | 'X' | 's' => {
                    let (start, end) = line_bounds(source, *cursor);
                    let range = if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
                        self.selection(source, *cursor, *anchor)
                    } else if character == 'X' {
                        (*cursor).saturating_sub(count).max(start)..*cursor
                    } else {
                        *cursor..(*cursor + count).min(end)
                    };
                    self.apply(
                        if character == 's' {
                            Operator::Change
                        } else {
                            Operator::Delete
                        },
                        range,
                        false,
                        source,
                        cursor,
                        anchor,
                    );
                }
                'D' | 'C' | 'Y' => {
                    let (start, end) = line_bounds(source, *cursor);
                    self.apply(
                        match character {
                            'C' => Operator::Change,
                            'Y' => Operator::Yank,
                            _ => Operator::Delete,
                        },
                        if character == 'Y' {
                            start
                                ..(end + usize::from(end < source.rope.len_chars()))
                                    .min(source.rope.len_chars())
                        } else {
                            *cursor..end
                        },
                        character == 'Y',
                        source,
                        cursor,
                        anchor,
                    );
                }
                'p' | 'P' => {
                    if self.register.is_empty() {
                        continue;
                    }
                    if self.register.len().saturating_mul(count) > SIZE_BYTES_MAX {
                        self.error = Some("Paste exceeds 16 MiB");
                        continue;
                    }
                    let (start, end) = line_bounds(source, *cursor);
                    let insertion = if self.linewise {
                        if character == 'p' {
                            (end + usize::from(end < source.rope.len_chars()))
                                .min(source.rope.len_chars())
                        } else {
                            start
                        }
                    } else if character == 'p' {
                        (*cursor + 1).min(end)
                    } else {
                        *cursor
                    };
                    let mut text = self.register.repeat(count);
                    if self.linewise
                        && insertion == source.rope.len_chars()
                        && insertion > 0
                        && source.rope.char(insertion - 1) != '\n'
                    {
                        text.insert(0, '\n');
                    }
                    if source.replace(insertion..insertion, &text, *cursor) {
                        *cursor = insertion;
                        *anchor = *cursor;
                    }
                }
                'u' => {
                    for _ in 0..count {
                        let Some(position) = source.undo(false) else {
                            break;
                        };
                        *cursor = position;
                        *anchor = position;
                    }
                }
                'K' => return Some(Action::Hover),
                _ => {
                    if let Some((target, _, _)) = motion(source, *cursor, character, count, counted)
                    {
                        *cursor = target;
                        if !matches!(self.mode, Mode::Visual | Mode::VisualLine) {
                            *anchor = *cursor;
                        }
                    }
                }
            }
        }
        None
    }
    fn apply(
        &mut self,
        operator: Operator,
        range: Range<usize>,
        linewise: bool,
        source: &mut Buffer,
        cursor: &mut usize,
        anchor: &mut usize,
    ) {
        assert!(range.start <= range.end);
        assert!(range.end <= source.rope.len_chars());
        self.register = source.rope.slice(range.clone()).to_string();
        self.linewise = linewise;
        if !matches!(operator, Operator::Yank) {
            let replacement = if linewise
                && matches!(operator, Operator::Change)
                && self.register.ends_with('\n')
            {
                "\n"
            } else {
                ""
            };
            if source.replace(range.clone(), replacement, *cursor) {
                *cursor = range.start;
            }
        } else {
            *cursor = range.start;
        }
        *cursor = (*cursor).min(source.rope.len_chars());
        *anchor = *cursor;
        self.mode = if matches!(operator, Operator::Change) {
            Mode::Insert
        } else {
            Mode::Normal
        };
    }
}

fn line_bounds(source: &Buffer, cursor: usize) -> (usize, usize) {
    let line = source
        .rope
        .char_to_line(cursor.min(source.rope.len_chars()));
    let start = source.rope.line_to_char(line);
    let mut end = start + source.rope.line(line).len_chars();
    if end > start && source.rope.char(end - 1) == '\n' {
        end -= 1;
    }
    if end > start && source.rope.char(end - 1) == '\r' {
        end -= 1;
    }
    (start, end)
}
fn first_nonblank(source: &Buffer, mut start: usize, end: usize) -> usize {
    while start < end && source.rope.char(start).is_whitespace() {
        start += 1;
    }
    start
}
fn class(character: char) -> u8 {
    if character.is_whitespace() {
        0
    } else if character.is_alphanumeric() || character == '_' {
        1
    } else {
        2
    }
}
fn motion(
    source: &Buffer,
    cursor: usize,
    key: char,
    count: usize,
    counted: bool,
) -> Option<(usize, bool, bool)> {
    let cursor = cursor.min(source.rope.len_chars());
    let (start, end) = line_bounds(source, cursor);
    let maximum = end.saturating_sub(1).max(start);
    let mut target = cursor;
    match key {
        'h' => target = cursor.saturating_sub(count).max(start),
        'l' => target = (cursor + count).min(maximum),
        '0' => target = start,
        '^' => target = first_nonblank(source, start, end),
        '$' => target = maximum,
        'j' | 'k' | 'G' => {
            let line = source.rope.char_to_line(cursor);
            let line = match key {
                'j' => (line + count).min(source.rope.len_lines() - 1),
                'k' => line.saturating_sub(count),
                _ if counted => (count - 1).min(source.rope.len_lines() - 1),
                _ => source.rope.len_lines() - 1,
            };
            let target_start = source.rope.line_to_char(line);
            let (_, target_end) = line_bounds(source, target_start);
            target =
                (target_start + cursor - start).min(target_end.saturating_sub(1).max(target_start));
        }
        'w' | 'b' | 'e' => {
            for _ in 0..count {
                if key == 'b' {
                    target = target.saturating_sub(1);
                    while target > 0 && class(source.rope.char(target)) == 0 {
                        target -= 1;
                    }
                    if target > 0 {
                        let category = class(source.rope.char(target));
                        while target > 0 && class(source.rope.char(target - 1)) == category {
                            target -= 1;
                        }
                    }
                } else if key == 'w' {
                    if target < source.rope.len_chars() {
                        let category = class(source.rope.char(target));
                        while target < source.rope.len_chars()
                            && class(source.rope.char(target)) == category
                        {
                            target += 1;
                        }
                    }
                    while target < source.rope.len_chars() && class(source.rope.char(target)) == 0 {
                        target += 1;
                    }
                } else {
                    if target + 1 < source.rope.len_chars() {
                        target += 1;
                    }
                    while target < source.rope.len_chars() && class(source.rope.char(target)) == 0 {
                        target += 1;
                    }
                    if target < source.rope.len_chars() {
                        let category = class(source.rope.char(target));
                        while target + 1 < source.rope.len_chars()
                            && class(source.rope.char(target + 1)) == category
                        {
                            target += 1;
                        }
                    }
                }
            }
        }
        _ => return None,
    }
    Some((
        target.min(source.rope.len_chars()),
        matches!(key, 'e' | '$'),
        matches!(key, 'j' | 'k' | 'G'),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replace_visual_unicode_lines_digits_and_cancel_without_touching_register() {
        let mut source = Buffer::from("abλ\ncd\r\nef");
        let mut state = State::default();
        state.enable(true);
        state.register = "saved yank".into();
        let (mut cursor, mut anchor) = (0, 0);
        state.text("vllr7", &mut source, &mut cursor, &mut anchor);
        assert_eq!(source.to_string(), "777\ncd\r\nef");
        assert!(state.mode == Mode::Normal);
        assert_eq!(state.register, "saved yank");
        state.text("uVjrλ", &mut source, &mut cursor, &mut anchor);
        assert_eq!(source.to_string(), "λλλ\nλλ\r\nef");
        state.text("u", &mut source, &mut cursor, &mut anchor);
        assert_eq!(source.to_string(), "abλ\ncd\r\nef");
        cursor = 2;
        anchor = cursor;
        state.text("vhh", &mut source, &mut cursor, &mut anchor);
        state.text("r", &mut source, &mut cursor, &mut anchor);
        state.text("_", &mut source, &mut cursor, &mut anchor);
        assert_eq!(source.to_string(), "___\ncd\r\nef");
        state.text("u3rX", &mut source, &mut cursor, &mut anchor);
        assert_eq!(source.to_string(), "XXX\ncd\r\nef");
        state.text("vr", &mut source, &mut cursor, &mut anchor);
        state.escape(&source, &mut cursor, &mut anchor);
        state.text("l", &mut source, &mut cursor, &mut anchor);
        assert_eq!(source.to_string(), "XXX\ncd\r\nef");
        assert!(state.mode == Mode::Normal);
    }
    #[test]
    fn leader_keys_dispatch_native_actions_without_editing_source() {
        let mut source = Buffer::from("unchanged\n");
        let mut state = State::default();
        state.enable(true);
        let (mut cursor, mut anchor) = (0, 0);
        for (keys, expected) in [
            (" pf", "quick_open"),
            (" vd", "diagnostics"),
            (" xx", "diagnostics"),
            ("  ", "run"),
            ("-", "open"),
        ] {
            let action = state
                .text(keys, &mut source, &mut cursor, &mut anchor)
                .expect("Native action");
            assert!(match expected {
                "quick_open" => matches!(action, Action::QuickOpen),
                "open" => matches!(action, Action::Open(None)),
                "diagnostics" => matches!(action, Action::ToggleDiagnostics),
                _ => matches!(action, Action::Run),
            });
            assert_eq!(source.to_string(), "unchanged\n");
        }
    }
    #[test]
    fn change_word_preserves_newline_and_batched_insert_text() {
        let mut source = Buffer::from("110\nnext\n");
        let mut state = State::default();
        state.enable(true);
        let (mut cursor, mut anchor) = (0, 0);
        state.text("cw120", &mut source, &mut cursor, &mut anchor);
        assert_eq!(source.to_string(), "120\nnext\n");
        assert!(state.mode == Mode::Insert);
        state.escape(&source, &mut cursor, &mut anchor);
        state.text(":w", &mut source, &mut cursor, &mut anchor);
        assert!(matches!(state.command_enter(), Some(Action::Save)));
    }
    #[test]
    fn operators_counts_visual_selection_and_undo_use_the_rope() {
        let mut source = Buffer::from("one two three\nsecond line\nlast\n");
        let mut state = State::default();
        state.enable(true);
        let (mut cursor, mut anchor) = (0, 0);
        state.text("2dw", &mut source, &mut cursor, &mut anchor);
        assert_eq!(source.to_string(), "three\nsecond line\nlast\n");
        state.text("u", &mut source, &mut cursor, &mut anchor);
        assert_eq!(source.to_string(), "one two three\nsecond line\nlast\n");
        state.text("jdd", &mut source, &mut cursor, &mut anchor);
        assert_eq!(source.to_string(), "one two three\nlast\n");
        state.text("P", &mut source, &mut cursor, &mut anchor);
        assert_eq!(source.to_string(), "one two three\nsecond line\nlast\n");
        state.text("ggvll", &mut source, &mut cursor, &mut anchor);
        assert_eq!(
            source
                .rope
                .slice(state.selection(&source, cursor, anchor))
                .to_string(),
            "one"
        );
        state.text("c", &mut source, &mut cursor, &mut anchor);
        assert!(state.mode == Mode::Insert);
        assert_eq!(source.to_string(), " two three\nsecond line\nlast\n");
    }
}
