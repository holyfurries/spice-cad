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
    insert_undo: Option<(u64, u64)>,
}
impl State {
    pub fn break_insert_undo(&mut self) {
        self.insert_undo = None;
    }

    pub fn prepare_insert_undo(&mut self, source: &Buffer) {
        if self.enabled && self.mode == Mode::Insert {
            if self
                .insert_undo
                .is_none_or(|(_, latest)| latest != source.revision)
            {
                self.insert_undo = Some((source.revision, source.revision));
            }
        }
    }

    pub fn merge_insert_undo(&mut self, source: &mut Buffer) {
        if let Some((first, _)) = self
            .insert_undo
            .filter(|_| self.enabled && self.mode == Mode::Insert)
        {
            source.merge_undo(first);
            self.insert_undo = Some((first, source.revision));
        }
    }

    pub fn redo(&mut self, source: &mut Buffer, cursor: &mut usize, anchor: &mut usize) {
        for _ in 0..self.take_count(1) {
            let Some(position) = source.redo_change() else {
                break;
            };
            *cursor = normal_cursor(source, position);
            *anchor = *cursor;
        }
    }
    pub fn take_count(&mut self, default: usize) -> usize {
        assert!(default > 0);
        self.pending = Pending::None;
        match std::mem::take(&mut self.count) {
            0 => default,
            count => count,
        }
    }

    pub fn change_number(
        &mut self,
        source: &mut Buffer,
        cursor: &mut usize,
        anchor: &mut usize,
        subtract: bool,
    ) {
        self.error = None;
        if matches!(self.pending, Pending::Go) {
            self.take_count(1);
            self.error = Some("Sequential number changes are not implemented");
            return;
        }
        let count = self.take_count(1) as i128;
        let visual = matches!(self.mode, Mode::Visual | Mode::VisualLine);
        let selection = if visual {
            self.selection(source, *cursor, *anchor)
        } else {
            let (start, end) = line_bounds(source, *cursor);
            start..end
        };
        let first = source.rope.char_to_line(selection.start);
        let last = source
            .rope
            .char_to_line(selection.end.saturating_sub(1).max(selection.start));
        if last - first >= 10_000 {
            self.error = Some("Number changes are limited to 10,000 lines");
            return;
        }
        let revision = source.revision;
        for line in (first..=last).rev() {
            let (start, end) = line_bounds(source, source.rope.line_to_char(line));
            let start = start.max(selection.start);
            let end = end.min(selection.end);
            let search = if visual { start } else { *cursor };
            let Some((range, replacement)) = change_number(
                source.rope.slice(start..end),
                start,
                search,
                if subtract { -count } else { count },
            ) else {
                continue;
            };
            let target = range.start + replacement.len() - 1;
            if source.replace(range, &replacement, *cursor) {
                source.merge_undo(revision);
                *cursor = target;
                *anchor = target;
            } else {
                self.error = Some("Number change exceeds the document limit");
                break;
            }
        }
        if visual {
            self.mode = Mode::Normal;
            *cursor = selection.start.min(source.rope.len_chars());
            *anchor = *cursor;
        }
    }

    pub fn enable(&mut self, enabled: bool) {
        self.break_insert_undo();
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
        self.break_insert_undo();
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
                self.prepare_insert_undo(source);
                let text = &text[offset..];
                let range = (*cursor).min(*anchor)..(*cursor).max(*anchor);
                if source.replace(range.clone(), text, *cursor) {
                    self.merge_insert_undo(source);
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
                    self.insert_undo = Some((source.revision, source.revision));
                }
                'o' | 'O' => {
                    let (start, end) = line_bounds(source, *cursor);
                    let insertion = if character == 'o' { end } else { start };
                    let revision = source.revision;
                    if source.replace(insertion..insertion, "\n", *cursor) {
                        *cursor = insertion + usize::from(character == 'o');
                        *anchor = *cursor;
                        self.mode = Mode::Insert;
                        self.insert_undo = Some((revision, source.revision));
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
                        *cursor = normal_cursor(source, position);
                        *anchor = *cursor;
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
        let revision = source.revision;
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
        self.insert_undo = if self.mode == Mode::Insert {
            Some((revision, source.revision))
        } else {
            None
        };
    }
}

fn normal_cursor(source: &Buffer, position: usize) -> usize {
    let position = position.min(source.rope.len_chars());
    let (start, end) = line_bounds(source, position);
    if position >= end && end > start {
        end - 1
    } else {
        position
    }
}

fn change_number(
    line: ropey::RopeSlice<'_>,
    start: usize,
    cursor: usize,
    delta: i128,
) -> Option<(Range<usize>, String)> {
    assert!(cursor >= start);
    assert!(delta.abs() <= 10_000);
    let mut characters = line.chars().enumerate().peekable();
    while let Some((index, character)) = characters.next() {
        if !character.is_ascii_digit() {
            continue;
        }
        let mut digits_start = index;
        let mut radix = 10;
        let mut prefix = String::new();
        if character == '0' {
            if let Some(&(_, marker)) = characters.peek() {
                radix = match marker {
                    'x' | 'X' => 16,
                    'b' | 'B' => 2,
                    _ => 10,
                };
                if radix != 10 {
                    characters.next();
                    digits_start += 2;
                    prefix.push('0');
                    prefix.push(marker);
                }
            }
        }
        let mut end = if radix == 10 { index + 1 } else { digits_start };
        while let Some(&(position, digit)) = characters.peek() {
            if !digit.is_digit(radix) {
                break;
            }
            end = position + 1;
            characters.next();
        }
        if start + end <= cursor || end == digits_start {
            continue;
        }
        if end - digits_start > 64 {
            return None;
        }
        let digits = line.slice(digits_start..end).to_string();
        let magnitude = u64::from_str_radix(&digits, radix).ok()?;
        let negative = radix == 10 && index > 0 && line.char(index - 1) == '-';
        let number_start = index - usize::from(negative);
        let width = if radix != 10 || digits.starts_with('0') {
            digits.len()
        } else {
            0
        };
        let replacement = if radix == 10 {
            let value = if negative {
                -(magnitude as i128)
            } else {
                magnitude as i128
            } + delta;
            let sign = if value < 0 { "-" } else { "" };
            format!("{sign}{:0width$}", value.unsigned_abs() as u64)
        } else {
            let value = magnitude.wrapping_add(delta as u64);
            if radix == 2 {
                format!("{prefix}{value:0width$b}")
            } else if digits
                .chars()
                .rev()
                .find(char::is_ascii_alphabetic)
                .is_some_and(|character| character.is_ascii_uppercase())
            {
                format!("{prefix}{value:0width$X}")
            } else {
                format!("{prefix}{value:0width$x}")
            }
        };
        return Some((start + number_start..start + end, replacement));
    }
    None
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
    fn number_commands_find_the_current_line_number_and_preserve_its_format() {
        for (text, position, keys, subtract, expected, target) in [
            ("local x = -009", 0, "3", false, "local x = -006", 13),
            ("local x = 0xFF", 12, "", true, "local x = 0xFE", 13),
            ("0b111", 3, "", false, "0b1000", 5),
            ("12.5", 3, "", false, "12.6", 3),
            ("λ = 9", 0, "", false, "λ = 10", 5),
            ("0xff", 2, "255", true, "0x00", 3),
            ("0", 0, "", true, "-1", 1),
            ("no numbers\n42", 0, "", false, "no numbers\n42", 0),
        ] {
            let mut source = Buffer::from(text);
            let mut state = State::default();
            state.enable(true);
            let mut cursor = position;
            let mut anchor = position;
            state.text(keys, &mut source, &mut cursor, &mut anchor);
            state.change_number(&mut source, &mut cursor, &mut anchor, subtract);
            assert_eq!(source.to_string(), expected);
            assert_eq!(cursor, target);
            assert_eq!(anchor, cursor);
        }
    }

    #[test]
    fn visual_number_changes_touch_one_number_per_line_in_one_undo_step() {
        let text = "10 foo20\n21";
        let mut source = Buffer::from(text);
        let mut state = State::default();
        state.enable(true);
        let mut cursor = 0;
        let mut anchor = 0;
        state.text("Vj", &mut source, &mut cursor, &mut anchor);
        state.change_number(&mut source, &mut cursor, &mut anchor, false);
        assert_eq!(source.to_string(), "11 foo20\n22");
        assert!(state.mode == Mode::Normal);
        assert_eq!(cursor, 0);
        assert!(source.undo(false).is_some());
        assert_eq!(source.to_string(), text);
        assert!(source.undo(false).is_none());
    }
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
    fn insert_and_change_commands_undo_as_blocks_and_counted_redo_restores_them() {
        for (command, redo_cursor) in [
            ("i", 0),
            ("a", 1),
            ("I", 0),
            ("A", 8),
            ("cw", 0),
            ("cc", 0),
            ("o", 0),
            ("O", 0),
        ] {
            let mut source = Buffer::from("old text\n");
            let mut state = State::default();
            state.enable(true);
            let (mut cursor, mut anchor) = (0, 0);
            state.text(command, &mut source, &mut cursor, &mut anchor);
            for text in ["n", "é", "w"] {
                state.text(text, &mut source, &mut cursor, &mut anchor);
            }
            let changed = source.to_string();
            let revision = source.revision;
            state.escape(&source, &mut cursor, &mut anchor);
            state.text("u", &mut source, &mut cursor, &mut anchor);
            assert_eq!(source.to_string(), "old text\n", "{command}");
            assert_eq!(cursor, anchor);
            state.redo(&mut source, &mut cursor, &mut anchor);
            assert_eq!(source.to_string(), changed, "{command}");
            assert_eq!(source.revision, revision);
            assert_eq!(cursor, anchor);
            assert_eq!(cursor, redo_cursor, "{command}");
        }
        let mut source = Buffer::from("");
        let mut state = State::default();
        state.enable(true);
        let (mut cursor, mut anchor) = (0, 0);
        for input in ["ione", "Atwo", "Athree"] {
            state.text(input, &mut source, &mut cursor, &mut anchor);
            state.escape(&source, &mut cursor, &mut anchor);
        }
        state.text("2u", &mut source, &mut cursor, &mut anchor);
        assert_eq!(source.to_string(), "one");
        state.text("2", &mut source, &mut cursor, &mut anchor);
        state.redo(&mut source, &mut cursor, &mut anchor);
        assert_eq!(source.to_string(), "onetwothree");
        assert!(cursor < source.rope.len_chars());
    }

    #[test]
    fn insert_blocks_split_at_movements_and_unrelated_source_edits() {
        let mut source = Buffer::from("");
        let mut state = State::default();
        state.enable(true);
        let (mut cursor, mut anchor) = (0, 0);
        state.text("iabc", &mut source, &mut cursor, &mut anchor);
        state.break_insert_undo();
        cursor = 2;
        anchor = 2;
        state.text("de", &mut source, &mut cursor, &mut anchor);
        state.escape(&source, &mut cursor, &mut anchor);
        state.text("u", &mut source, &mut cursor, &mut anchor);
        assert_eq!(source.to_string(), "abc");
        state.text("u", &mut source, &mut cursor, &mut anchor);
        assert!(source.is_empty());
        state.text("ix", &mut source, &mut cursor, &mut anchor);
        source.replace(0..0, "external", 0);
        cursor = source.rope.len_chars();
        anchor = cursor;
        state.text("yz", &mut source, &mut cursor, &mut anchor);
        state.escape(&source, &mut cursor, &mut anchor);
        state.text("u", &mut source, &mut cursor, &mut anchor);
        assert_eq!(source.to_string(), "externalx");
    }

    #[test]
    fn long_insert_sessions_keep_their_start_beyond_the_history_entry_limit() {
        let mut source = Buffer::from("");
        let mut state = State::default();
        state.enable(true);
        let (mut cursor, mut anchor) = (0, 0);
        state.text("i", &mut source, &mut cursor, &mut anchor);
        for _ in 0..2048 {
            state.text("x", &mut source, &mut cursor, &mut anchor);
        }
        state.escape(&source, &mut cursor, &mut anchor);
        state.text("u", &mut source, &mut cursor, &mut anchor);
        assert!(source.is_empty());
        assert!(source.undo(false).is_none());
        state.redo(&mut source, &mut cursor, &mut anchor);
        assert_eq!(source.rope.len_chars(), 2048);
        assert_eq!(cursor, 0);
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
