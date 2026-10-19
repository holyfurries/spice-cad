use crate::text_buffer::Buffer;
use eframe::egui::{self, Color32, FontId};
use std::{collections::BTreeMap, sync::Arc};

const COLUMN_GUIDE: usize = 80;
const INDENT_COLUMNS: usize = 4;
const SCROLL_MARGIN_LINES: usize = 8;

#[derive(Default, Clone, Copy)]
enum FocusRequest {
    #[default]
    None,
    Deferred,
    Ready,
}

pub fn preferred_width(ui: &mut egui::Ui, source: &Buffer) -> f32 {
    let character_width = ui
        .fonts_mut(|fonts| fonts.glyph_width(&FontId::monospace(14.0), 'M'))
        .max(1.0);
    let gutter_columns = (source.rope.len_lines().ilog10() + 1) as usize;
    (COLUMN_GUIDE + gutter_columns) as f32 * character_width + 48.0
}

#[derive(Default)]
pub struct State {
    pub outputs: crate::inline_output::State,
    pub modal: crate::modal::State,
    pub action: Option<crate::modal::Action>,
    search: crate::search::State,
    syntax: crate::syntax::State,
    syntax_changed: bool,
    cursor: usize,
    anchor: usize,
    revision: u64,
    reveal: bool,
    center_cursor: bool,
    focus_request: FocusRequest,
    width: f32,
    pixels_per_point: f32,
    composition: String,
    pub error: Option<&'static str>,
    lines: BTreeMap<(usize, usize, usize), Arc<egui::Galley>>,
    advances: BTreeMap<usize, Vec<(u32, f32)>>,
}

impl State {
    pub fn parameters(
        &mut self,
        source: &Buffer,
        context: &egui::Context,
    ) -> Option<&[crate::parameters::Parameter]> {
        self.syntax_changed |= self.syntax.sync(source, context);
        self.syntax.parameters(source.revision)
    }

    pub fn replace_parameter(
        &mut self,
        source: &mut Buffer,
        edit: &crate::parameters::Edit,
    ) -> Result<(), &'static str> {
        let range = edit.apply(source, self.cursor)?;
        self.cursor = range.end;
        self.anchor = range.end;
        self.reveal = true;
        Ok(())
    }

    pub fn focus(&mut self) {
        self.focus_request = FocusRequest::Deferred;
    }

    pub fn open_search(&mut self, backward: bool, replacing: bool) {
        self.focus_request = FocusRequest::None;
        if self.modal.enabled {
            self.modal.mode = crate::modal::Mode::Normal;
        }
        self.search.open(backward, replacing, self.cursor);
    }
    pub fn search_next(&mut self, source: &Buffer, opposite: bool) {
        if let Some(range) = self.search.next(source.revision, self.cursor, opposite) {
            self.jump_to(range);
            self.center_cursor = true;
        }
    }
    fn newline(&mut self, source: &mut Buffer) {
        let cursor = self.selection(source).start;
        let start = source.rope.line_to_char(source.rope.char_to_line(cursor));
        let mut indentation = 0;
        for character in source.rope.slice(start..cursor).chars().take(256) {
            match character {
                ' ' => indentation += 1,
                '\t' => indentation += INDENT_COLUMNS - indentation % INDENT_COLUMNS,
                _ => break,
            }
        }
        let tail = source
            .rope
            .slice(cursor.saturating_sub(256).max(start)..cursor)
            .to_string();
        let tail = tail.trim();
        if tail.ends_with(" then")
            || tail.ends_with(" do")
            || tail == "else"
            || tail == "repeat"
            || tail.ends_with('{')
            || tail.starts_with("function ")
            || tail.starts_with("local function ")
        {
            indentation += INDENT_COLUMNS;
        }
        self.insert(source, &format!("\n{}", " ".repeat(indentation.min(256))));
    }
    pub fn jump_to(&mut self, range: std::ops::Range<usize>) {
        assert!(range.start <= range.end);
        self.anchor = range.start;
        self.cursor = range.end;
        self.reveal = true;
        self.focus_request = FocusRequest::Ready;
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn replace_selection(
        &mut self,
        source: &mut Buffer,
        range: std::ops::Range<usize>,
        text: &str,
    ) {
        if self.modal.enabled {
            self.modal.mode = crate::modal::Mode::Insert;
        }
        self.jump_to(range);
        self.insert(source, text);
    }

    fn selection(&self, source: &Buffer) -> std::ops::Range<usize> {
        self.modal.selection(source, self.cursor, self.anchor)
    }

    fn insert(&mut self, source: &mut Buffer, text: &str) {
        let selection = self.selection(source);
        let removed =
            source.rope.char_to_byte(selection.end) - source.rope.char_to_byte(selection.start);
        if source.rope.len_bytes() - removed + text.len() > crate::text_buffer::SIZE_BYTES_MAX {
            self.error = Some("Edit exceeds the 16 MiB document limit; the text was preserved.");
            return;
        }
        let cursor = selection.start + text.chars().count();
        if source.replace(selection, text, self.cursor) {
            self.error = None;
            self.cursor = cursor;
            self.anchor = cursor;
            self.reveal = true;
        }
    }
}

pub fn show(ui: &mut egui::Ui, source: &mut Buffer, state: &mut State) -> bool {
    let before = source.revision;
    let search_was_open = state.search.is_open();
    let mut search_handoff = false;
    state.cursor = state.cursor.min(source.rope.len_chars());
    state.anchor = state.anchor.min(source.rope.len_chars());
    let selection = state.selection(source);
    if let Some(command) = state.search.show(ui, source, state.cursor, selection) {
        match command {
            crate::search::Command::Select(range) => {
                state.anchor = range.start;
                state.cursor = range.end;
                state.reveal = true;
                state.center_cursor = true;
                if !state.search.is_open() {
                    state.focus_request = FocusRequest::Ready;
                    search_handoff = search_was_open;
                }
            }
            crate::search::Command::Focus => {
                state.focus_request = FocusRequest::Ready;
                search_handoff = true;
            }
            crate::search::Command::Replace { range, text } => {
                if state.modal.enabled {
                    state.modal.mode = crate::modal::Mode::Normal;
                }
                state.anchor = range.start;
                state.cursor = range.end;
                state.insert(source, &text);
            }
            crate::search::Command::Apply {
                revision,
                source: replacement,
                cursor,
            } => {
                if source.replace_snapshot(revision, replacement, state.cursor, cursor) {
                    state.cursor = cursor;
                    state.anchor = cursor;
                    state.reveal = true;
                }
            }
        }
    }
    let id = egui::Id::unique("luau_source");
    let font = FontId::monospace(14.0);
    let row_height = 21.0;
    let char_width = ui.fonts_mut(|fonts| fonts.glyph_width(&font, 'M')).max(1.0);
    let available = ui.available_rect_before_wrap();
    let response = ui.interact(available, id, egui::Sense::click_and_drag());
    let focus_allowed = ui
        .ctx()
        .memory(|memory| memory.is_above_modal_layer(ui.layer_id()));
    if focus_allowed && (response.clicked() || response.drag_started()) {
        response.request_focus();
        state.focus_request = FocusRequest::None;
    } else {
        match state.focus_request {
            FocusRequest::Deferred => {
                state.focus_request = FocusRequest::Ready;
                ui.ctx().request_repaint();
            }
            FocusRequest::Ready if focus_allowed => {
                response.request_focus();
                state.focus_request = FocusRequest::None;
            }
            _ => {}
        }
    }
    state.cursor = state.cursor.min(source.rope.len_chars());
    state.anchor = state.anchor.min(source.rope.len_chars());
    if focus_allowed
        && response.has_focus()
        && !search_handoff
        && !egui::Popup::is_any_open(ui.ctx())
    {
        ui.ctx().memory_mut(|memory| {
            memory.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: state.modal.enabled,
                },
            )
        });
        let events: Vec<_> = ui.input_mut(|input| {
            input
                .events
                .extract_if(.., |event| {
                    matches!(
                        event,
                        egui::Event::Text(_)
                            | egui::Event::Paste(_)
                            | egui::Event::Copy
                            | egui::Event::Cut
                            | egui::Event::Key { .. }
                            | egui::Event::Ime(_)
                    )
                })
                .collect()
        });
        for event in events {
            match event {
                egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                    state.composition = text.chars().take(4096).collect();
                }
                egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                    state.composition.clear();
                    state.insert(source, &text);
                }
                egui::Event::Ime(egui::ImeEvent::DeleteSurrounding {
                    before_chars,
                    after_chars,
                }) => {
                    state.anchor = state.cursor.saturating_sub(before_chars);
                    state.cursor = state
                        .cursor
                        .saturating_add(after_chars)
                        .min(source.rope.len_chars());
                    state.insert(source, "");
                }
                egui::Event::Text(text)
                    if state.modal.enabled && state.modal.mode != crate::modal::Mode::Insert =>
                {
                    state.action =
                        state
                            .modal
                            .text(&text, source, &mut state.cursor, &mut state.anchor);
                    state.reveal = true;
                }
                egui::Event::Text(text) | egui::Event::Paste(text) => state.insert(source, &text),
                egui::Event::Copy
                    if state.modal.enabled && state.modal.mode == crate::modal::Mode::Insert =>
                {
                    state
                        .modal
                        .escape(source, &mut state.cursor, &mut state.anchor);
                    state.reveal = true;
                }
                egui::Event::Copy | egui::Event::Cut => {
                    let selection = state.selection(source);
                    if !selection.is_empty() {
                        ui.ctx().copy_text(source.rope.slice(selection).to_string());
                        if matches!(event, egui::Event::Cut) {
                            state.insert(source, "");
                        }
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if key == egui::Key::F1
                        || key == egui::Key::F12
                        || key == egui::Key::Space && modifiers.command
                    {
                        state.action = Some(match key {
                            egui::Key::F1 => crate::modal::Action::Hover,
                            egui::Key::F12 => crate::modal::Action::Definition,
                            _ => crate::modal::Action::Complete,
                        });
                        continue;
                    }
                    if state.modal.enabled {
                        if key == egui::Key::Escape
                            || key == egui::Key::C
                                && modifiers.ctrl
                                && state.modal.mode == crate::modal::Mode::Insert
                        {
                            state
                                .modal
                                .escape(source, &mut state.cursor, &mut state.anchor);
                            state.reveal = true;
                            continue;
                        }
                        if state.modal.mode == crate::modal::Mode::Normal && modifiers.ctrl {
                            if matches!(key, egui::Key::D | egui::Key::U) {
                                let line = source.rope.char_to_line(state.cursor);
                                let column = state.cursor - source.rope.line_to_char(line);
                                let distance =
                                    (available.height() / row_height / 2.0).max(1.0) as usize;
                                let target = if key == egui::Key::U {
                                    line.saturating_sub(distance)
                                } else {
                                    (line + distance).min(source.rope.len_lines() - 1)
                                };
                                state.cursor = source.rope.line_to_char(target)
                                    + column.min(line_length(source.rope.line(target)));
                                state.anchor = state.cursor;
                                state.reveal = true;
                                state.center_cursor = true;
                                continue;
                            }
                            if matches!(key, egui::Key::J | egui::Key::K) {
                                state.action = Some(match key {
                                    egui::Key::J => crate::modal::Action::PreviousDiagnostic,
                                    _ => crate::modal::Action::NextDiagnostic,
                                });
                                continue;
                            }
                        }
                        if state.modal.mode == crate::modal::Mode::Command {
                            if key == egui::Key::Enter {
                                state.action = state.modal.command_enter();
                            }
                            if key == egui::Key::Backspace {
                                state.modal.command.pop();
                            }
                            continue;
                        }
                        if state.modal.mode != crate::modal::Mode::Insert && !modifiers.command {
                            let command = match key {
                                egui::Key::Enter => Some("j^"),
                                egui::Key::Backspace => Some("h"),
                                egui::Key::Delete => Some("x"),
                                egui::Key::Tab => Some(""),
                                _ => None,
                            };
                            if let Some(command) = command {
                                state.action = state.modal.text(
                                    command,
                                    source,
                                    &mut state.cursor,
                                    &mut state.anchor,
                                );
                                state.reveal = true;
                                continue;
                            }
                        }
                        if modifiers.command && key == egui::Key::R {
                            if let Some(cursor) = source.undo(true) {
                                state.cursor = cursor;
                                state.anchor = cursor;
                                state.reveal = true;
                            }
                            continue;
                        }
                    }
                    if modifiers.command {
                        match key {
                            egui::Key::A => {
                                state.anchor = 0;
                                state.cursor = source.rope.len_chars();
                            }
                            egui::Key::Home | egui::Key::ArrowUp => {
                                state.cursor = 0;
                                if !modifiers.shift {
                                    state.anchor = state.cursor;
                                }
                                state.reveal = true;
                            }
                            egui::Key::End | egui::Key::ArrowDown => {
                                state.cursor = source.rope.len_chars();
                                if !modifiers.shift {
                                    state.anchor = state.cursor;
                                }
                                state.reveal = true;
                            }
                            egui::Key::Z | egui::Key::Y => {
                                if let Some(cursor) =
                                    source.undo(modifiers.shift || key == egui::Key::Y)
                                {
                                    state.cursor = cursor;
                                    state.anchor = cursor;
                                    state.reveal = true;
                                }
                            }
                            _ => {}
                        }
                    } else {
                        let line = source.rope.char_to_line(state.cursor);
                        let start = source.rope.line_to_char(line);
                        let column = state.cursor - start;
                        let end = start + line_length(source.rope.line(line));
                        let mut moved = true;
                        match key {
                            egui::Key::Enter => state.newline(source),
                            egui::Key::Tab => state.insert(
                                source,
                                &" ".repeat(INDENT_COLUMNS - column % INDENT_COLUMNS),
                            ),
                            egui::Key::Backspace | egui::Key::Delete => {
                                if state.selection(source).is_empty() {
                                    if key == egui::Key::Backspace {
                                        state.anchor = state.cursor.saturating_sub(1);
                                    } else {
                                        state.anchor =
                                            (state.cursor + 1).min(source.rope.len_chars());
                                    }
                                }
                                state.insert(source, "");
                            }
                            egui::Key::ArrowLeft => {
                                state.cursor =
                                    if !modifiers.shift && !state.selection(source).is_empty() {
                                        state.selection(source).start
                                    } else {
                                        state.cursor.saturating_sub(1)
                                    }
                            }
                            egui::Key::ArrowRight => {
                                state.cursor =
                                    if !modifiers.shift && !state.selection(source).is_empty() {
                                        state.selection(source).end
                                    } else {
                                        (state.cursor + 1).min(source.rope.len_chars())
                                    }
                            }
                            egui::Key::Home => state.cursor = start,
                            egui::Key::End => state.cursor = end,
                            egui::Key::ArrowUp
                            | egui::Key::ArrowDown
                            | egui::Key::PageUp
                            | egui::Key::PageDown => {
                                let distance =
                                    if matches!(key, egui::Key::PageUp | egui::Key::PageDown) {
                                        (available.height() / row_height).max(1.0) as usize
                                    } else {
                                        1
                                    };
                                let target =
                                    if matches!(key, egui::Key::ArrowUp | egui::Key::PageUp) {
                                        line.saturating_sub(distance)
                                    } else {
                                        (line + distance).min(source.rope.len_lines() - 1)
                                    };
                                let target_start = source.rope.line_to_char(target);
                                let target_line = source.rope.line(target);
                                let length = line_length(target_line);
                                state.cursor = target_start + column.min(length);
                            }
                            _ => moved = false,
                        }
                        if moved {
                            if !modifiers.shift {
                                state.anchor = state.cursor;
                            }
                            state.reveal = true;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    state.syntax_changed |= state.syntax.sync(source, ui.ctx());
    let syntax_changed = std::mem::take(&mut state.syntax_changed);
    if syntax_changed
        || state.revision != source.revision
        || state.pixels_per_point != ui.ctx().pixels_per_point()
    {
        state.lines.clear();
        state.advances.clear();
        if state.revision != source.revision {
            state.width = available.width();
        }
        state.revision = source.revision;
        state.pixels_per_point = ui.ctx().pixels_per_point();
    }
    let line_count = source.rope.len_lines();
    state.outputs.sync(source);
    state
        .outputs
        .resolve(|line| state.syntax.output_end(source.revision, line));
    let output_layout = state.outputs.layout(line_count, row_height);
    let gutter = (line_count.ilog10() + 1) as f32 * char_width + 24.0;
    state.width = state.width.max(available.width());
    egui::ScrollArea::both()
        .id_salt("luau_source_scroll")
        .show_viewport(ui, |ui, viewport| {
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(state.width, output_layout.height()),
                egui::Sense::hover(),
            );
            let first = output_layout.line_index(viewport.min.y);
            let last = output_layout
                .line_index(viewport.max.y)
                .saturating_add(2)
                .min(line_count)
                .min(first.saturating_add(512));
            let columns = ((viewport.width() / char_width).ceil() as usize)
                .saturating_add(8)
                .min(4096);
            let mut visible_keys = Vec::new();
            let mut origins = BTreeMap::new();
            let mut cursor_x = None;
            for line in first..last {
                let slice = source.rope.line(line);
                let length = line_length(slice);
                let advances = state.advances.entry(line).or_insert_with(|| {
                    line_advances(ui, &font, char_width, &state.syntax, source, line)
                });
                let width = |column: usize| prefix_width(column, char_width, advances);
                let column_start = if length <= 256 {
                    0
                } else {
                    let left = (viewport.min.x - gutter).max(0.0);
                    let mut first = 0;
                    let mut last = length;
                    while first < last {
                        let middle = first + (last - first).div_ceil(2);
                        if width(middle) <= left {
                            first = middle;
                        } else {
                            last = middle - 1;
                        }
                    }
                    first
                };
                let column_end = if length <= 256 {
                    length
                } else {
                    (column_start + columns).min(length)
                };
                let origin = width(column_start);
                origins.insert(line, origin);
                state.width = state.width.max(gutter + width(length) + 32.0);
                let key = (line, column_start, column_end);
                visible_keys.push(key);
                state.lines.entry(key).or_insert_with(|| {
                    let start = source.rope.line_to_byte(line) + slice.char_to_byte(column_start);
                    let end = source.rope.line_to_byte(line)
                        + slice.char_to_byte(column_end.min(line_length(slice)));
                    ui.fonts_mut(|fonts| {
                        fonts.layout_job(state.syntax.layout(
                            source,
                            start..end.max(start),
                            font.clone(),
                        ))
                    })
                });
            }
            if response.clicked() || response.dragged() {
                if let Some(pointer) = response.interact_pointer_pos() {
                    if ui.clip_rect().contains(pointer) && rect.contains(pointer) {
                        if let Some(line) = output_layout.text_line(pointer.y - rect.top()) {
                            if let Some(&(line, column_start, column_end)) =
                                visible_keys.iter().find(|key| key.0 == line)
                            {
                                let galley = &state.lines[&(line, column_start, column_end)];
                                let position = rect.min
                                    + egui::vec2(
                                        gutter + origins[&line],
                                        output_layout.line_top(line),
                                    );
                                let column: usize =
                                    galley.cursor_from_pos(pointer - position).index.into();
                                let slice = source.rope.line(line);
                                let length = slice.len_chars();
                                let start = source.rope.line_to_char(line);
                                let cursor = (start + column_start + column).min(
                                    start
                                        + length.saturating_sub(usize::from(
                                            length > 0 && slice.char(length - 1) == '\n',
                                        )),
                                );
                                let anchor = if response.clicked()
                                    && !ui.input(|input| input.modifiers.shift)
                                    || response.drag_started()
                                {
                                    cursor
                                } else {
                                    state.anchor
                                };
                                if state.cursor != cursor || state.anchor != anchor {
                                    state.cursor = cursor;
                                    state.anchor = anchor;
                                    ui.ctx().request_repaint();
                                }
                            }
                        }
                    }
                }
            }
            let cursor_line = source.rope.char_to_line(state.cursor);
            let selection = state.selection(source);
            let mut highlights_remaining = 4096;
            for &(line, column_start, column_end) in &visible_keys {
                let start = source.rope.line_to_char(line);
                let galley = Arc::clone(&state.lines[&(line, column_start, column_end)]);
                let position =
                    rect.min + egui::vec2(gutter + origins[&line], output_layout.line_top(line));
                let row = egui::Rect::from_min_size(
                    egui::pos2(rect.left() + viewport.min.x, position.y),
                    egui::vec2(viewport.width(), row_height),
                );
                if line == cursor_line {
                    ui.painter()
                        .rect_filled(row, 0.0, Color32::from_rgb(33, 38, 43));
                }
                for found in state
                    .search
                    .ranges(source.revision, start + column_start..start + column_end)
                    .take(highlights_remaining)
                {
                    let first = found.start.max(start + column_start) - start - column_start;
                    let last = found.end.min(start + column_end) - start - column_start;
                    let left = galley
                        .pos_from_cursor(egui::text::CCursor::new(first))
                        .left();
                    let right = galley
                        .pos_from_cursor(egui::text::CCursor::new(last))
                        .left();
                    ui.painter().rect_filled(
                        egui::Rect::from_min_max(
                            position + egui::vec2(left, 0.0),
                            position + egui::vec2(right.max(left + char_width), row_height),
                        ),
                        2.0,
                        Color32::from_rgb(87, 70, 36),
                    );
                    highlights_remaining -= 1;
                }
                let selected_start = selection.start.max(start + column_start);
                let selected_end = selection.end.min(start + column_end);
                if selected_start < selected_end {
                    let x_start = galley
                        .pos_from_cursor(egui::text::CCursor::new(
                            selected_start - start - column_start,
                        ))
                        .left();
                    let x_end = galley
                        .pos_from_cursor(egui::text::CCursor::new(
                            selected_end - start - column_start,
                        ))
                        .left();
                    ui.painter().rect_filled(
                        egui::Rect::from_min_max(
                            position + egui::vec2(x_start, 0.0),
                            position + egui::vec2(x_end.max(x_start + char_width), row_height),
                        ),
                        0.0,
                        crate::interface::SELECTION,
                    );
                }
                ui.painter()
                    .galley(position, galley.clone(), Color32::WHITE);
                if response.has_focus() && line == cursor_line {
                    let column = state.cursor - start;
                    if column >= column_start && column <= column_end {
                        let x = galley
                            .pos_from_cursor(egui::text::CCursor::new(column - column_start))
                            .left();
                        cursor_x = Some(origins[&line] + x);
                        let block =
                            state.modal.enabled && state.modal.mode != crate::modal::Mode::Insert;
                        let caret = egui::Rect::from_min_size(
                            position + egui::vec2(x, 1.0),
                            egui::vec2(if block { char_width } else { 1.5 }, row_height - 2.0),
                        );
                        ui.painter().rect_filled(
                            caret,
                            0.0,
                            Color32::from_rgba_unmultiplied(
                                135,
                                219,
                                202,
                                if block { 100 } else { 255 },
                            ),
                        );
                        ui.output_mut(|output| {
                            output.ime = Some(egui::output::IMEOutput {
                                purpose: egui::IMEPurpose::Normal,
                                rect: available,
                                cursor_rect: caret,
                                should_interrupt_composition: false,
                            })
                        });
                        if !state.composition.is_empty() {
                            ui.painter().text(
                                caret.left_top(),
                                egui::Align2::LEFT_TOP,
                                &state.composition,
                                font.clone(),
                                crate::interface::ACCENT,
                            );
                        }
                        if state.reveal {
                            let margin = (SCROLL_MARGIN_LINES as f32 * row_height)
                                .min(viewport.height() / 2.0 - row_height / 2.0)
                                .max(0.0);
                            ui.scroll_to_rect(
                                caret.expand2(egui::vec2(8.0, margin)),
                                if state.center_cursor {
                                    Some(egui::Align::Center)
                                } else {
                                    None
                                },
                            );
                        }
                    }
                }
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(row.min, egui::vec2(gutter - 8.0, row_height)),
                    0.0,
                    crate::interface::PANEL,
                );
                ui.painter().text(
                    egui::pos2(row.left() + gutter - 16.0, position.y + 2.0),
                    egui::Align2::RIGHT_TOP,
                    if line == cursor_line {
                        (line + 1).to_string()
                    } else {
                        line.abs_diff(cursor_line).to_string()
                    },
                    font.clone(),
                    if line == cursor_line {
                        Color32::from_rgb(238, 246, 255)
                    } else {
                        crate::interface::MUTED
                    },
                );
            }
            if state.reveal {
                let column = state.cursor - source.rope.line_to_char(cursor_line);
                let x = cursor_x.unwrap_or_else(|| {
                    let advances = state.advances.entry(cursor_line).or_insert_with(|| {
                        line_advances(ui, &font, char_width, &state.syntax, source, cursor_line)
                    });
                    prefix_width(column, char_width, advances)
                });
                let caret = egui::Rect::from_min_size(
                    rect.min + egui::vec2(gutter + x, output_layout.line_top(cursor_line)),
                    egui::vec2(2.0, row_height),
                );
                let margin = (SCROLL_MARGIN_LINES as f32 * row_height)
                    .min(viewport.height() / 2.0 - row_height / 2.0)
                    .max(0.0);
                ui.scroll_to_rect(
                    caret.expand2(egui::vec2(gutter + 8.0, margin)),
                    if state.center_cursor {
                        Some(egui::Align::Center)
                    } else {
                        None
                    },
                );
                state.reveal = false;
                state.center_cursor = false;
            }
            let guide = rect.left() + gutter + COLUMN_GUIDE as f32 * char_width;
            ui.painter().line_segment(
                [
                    egui::pos2(guide, rect.top() + viewport.top()),
                    egui::pos2(guide, rect.top() + viewport.bottom()),
                ],
                egui::Stroke::new(1.0, crate::interface::BORDER),
            );
            for card in &output_layout.cards {
                if card.top + card.height < viewport.min.y || card.top > viewport.max.y {
                    continue;
                }
                let output_rect = egui::Rect::from_min_size(
                    rect.min + egui::vec2(viewport.min.x + gutter, card.top),
                    egui::vec2(
                        (viewport.width() - gutter - 12.0).clamp(120.0, 560.0),
                        card.height,
                    ),
                );
                state.outputs.paint(ui, card, output_rect);
            }
            state.lines.retain(|key, _| visible_keys.contains(key));
            state
                .advances
                .retain(|line, _| visible_keys.iter().any(|key| key.0 == *line));
        });
    source.revision != before
}
fn prefix_width(column: usize, character_width: f32, advances: &[(u32, f32)]) -> f32 {
    assert!(character_width > 0.0);
    assert!(column <= crate::text_buffer::SIZE_BYTES_MAX);
    let index = advances.partition_point(|(position, _)| (*position as usize) < column);
    column as f32 * character_width + index.checked_sub(1).map_or(0.0, |index| advances[index].1)
}

fn line_advances(
    ui: &mut egui::Ui,
    font: &FontId,
    character_width: f32,
    syntax: &crate::syntax::State,
    source: &Buffer,
    line: usize,
) -> Vec<(u32, f32)> {
    let pending;
    let special = if syntax.ready(source.revision) {
        syntax
            .special_columns(source.revision, line)
            .unwrap_or_default()
    } else {
        pending = crate::syntax::collect_special_columns(source.rope.line(line));
        &pending
    };
    let mut widths = BTreeMap::new();
    let mut extra = 0.0;
    ui.fonts_mut(|fonts| {
        special
            .iter()
            .map(|(column, character)| {
                let width = widths.entry(*character).or_insert_with(|| {
                    let width = fonts.glyph_width(font, *character);
                    if width == 0.0 && !fonts.has_glyph(font, *character) {
                        character_width
                    } else {
                        width
                    }
                });
                extra += *width - character_width;
                (*column, extra)
            })
            .collect()
    })
}
fn line_length(line: ropey::RopeSlice<'_>) -> usize {
    let mut length = line.len_chars();
    if length > 0 && line.char(length - 1) == '\n' {
        length -= 1;
    }
    if length > 0 && line.char(length - 1) == '\r' {
        length -= 1;
    }
    length
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_cards_preserve_text_hit_testing_and_collapse_without_editing_source() {
        let context = egui::Context::default();
        let text = "cad.show('part', part)\nlocal width = 10\n";
        let mut source = Buffer::from(text);
        let mut state = State::default();
        state.outputs.set(
            source.revision,
            Arc::new(vec![crate::inline_output::Entry {
                line: 0,
                name: "part".into(),
                content: crate::inline_output::Content::Part {
                    image: None,
                    dimensions: glam::Vec3::ONE,
                    volume_mm3: 1.0,
                },
            }]),
        );
        state.outputs.resolve(Some);
        let render = |source: &mut Buffer, state: &mut State, events| {
            context
                .run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(600.0, 400.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        show(ui, source, state);
                    },
                )
                .drop_without_applying_deltas();
        };
        let click = |source: &mut Buffer, state: &mut State, position: egui::Pos2| {
            render(
                source,
                state,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            render(
                source,
                state,
                vec![egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        };
        render(&mut source, &mut state, Vec::new());
        click(&mut source, &mut state, egui::pos2(90.0, 165.0));
        assert_eq!(source.rope.char_to_line(state.cursor), 1);
        let cursor = state.cursor;
        click(&mut source, &mut state, egui::pos2(90.0, 80.0));
        assert_eq!(state.cursor, cursor);
        click(&mut source, &mut state, egui::pos2(90.0, 30.0));
        assert_eq!(
            state.outputs.layout(source.rope.len_lines(), 21.0).cards[0].height,
            30.0
        );
        render(&mut source, &mut state, Vec::new());
        click(&mut source, &mut state, egui::pos2(90.0, 60.0));
        assert_eq!(source.rope.char_to_line(state.cursor), 1);
        assert_eq!(source.to_string(), text);
        assert!(source.undo(false).is_none());
    }
    #[test]
    fn horizontal_offsets_preserve_tabs_and_short_line_glyph_positions() {
        assert_eq!(prefix_width(3, 8.0, &[(0, 24.0), (2, 48.0)]), 72.0);
        assert_eq!(prefix_width(2, 8.0, &[(0, 24.0), (2, 48.0)]), 40.0);
        let mut source = Buffer::from("\tfirst\n    second\n\t\tλ third");
        let mut state = State::default();
        let context = egui::Context::default();
        for offset in [0.0, 40.0, 75.0, 0.0] {
            context
                .run_ui(
                    egui::RawInput {
                        events: vec![
                            egui::Event::PointerMoved(egui::pos2(45.0, 60.0)),
                            egui::Event::MouseWheel {
                                unit: egui::MouseWheelUnit::Point,
                                delta: egui::vec2(-offset, 0.0),
                                phase: egui::TouchPhase::Move,
                                source: egui::MouseWheelSource::Unknown,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                        ..Default::default()
                    },
                    |ui| {
                        ui.set_max_size(egui::vec2(80.0, 200.0));
                        show(ui, &mut source, &mut state);
                        for (key, galley) in &state.lines {
                            assert_eq!(key.1, 0);
                            assert_eq!(
                                galley.job.text,
                                source.rope.line(key.0).to_string().trim_end_matches('\n')
                            );
                        }
                    },
                )
                .drop_without_applying_deltas();
        }
    }

    #[test]
    fn virtualized_prefix_offsets_match_shaped_tabs_and_fallback_glyphs() {
        let source = Buffer::from("\t A λ 😀 text ".repeat(400));
        egui::Context::default()
            .run_ui(egui::RawInput::default(), |ui| {
                let font = FontId::monospace(14.0);
                let galley = ui.fonts_mut(|fonts| {
                    fonts.layout_job(crate::syntax::State::default().layout(
                        &source,
                        0..source.rope.len_bytes(),
                        font.clone(),
                    ))
                });
                let width = ui.fonts_mut(|fonts| fonts.glyph_width(&font, 'M'));
                let advances = line_advances(
                    ui,
                    &font,
                    width,
                    &crate::syntax::State::default(),
                    &source,
                    0,
                );
                for column in [12, 80, 1000, 4000] {
                    let expected: f64 = galley.rows[0]
                        .row
                        .glyphs
                        .iter()
                        .take(column)
                        .map(|glyph| f64::from(glyph.advance_width))
                        .sum();
                    let actual = prefix_width(column, width, &advances);
                    assert!(
                        (f64::from(actual) - expected).abs() <= 0.1,
                        "Column {column}: {actual} vs shaped {expected}"
                    );
                }
            })
            .drop_without_applying_deltas();
    }

    #[test]
    fn focus_requests_wait_for_modals_and_restore_keyboard_editing_after_close() {
        let context = egui::Context::default();
        let mut source = Buffer::from("original");
        let mut state = State::default();
        state.modal.enable(true);
        state.focus();
        let mut dialog_text = String::new();
        let mut render = |modal, events| {
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
                        if modal {
                            egui::Modal::new(egui::Id::unique("focus_regression")).show(
                                ui.ctx(),
                                |ui| {
                                    ui.text_edit_singleline(&mut dialog_text).request_focus();
                                },
                            );
                        }
                        show(ui, &mut source, &mut state);
                    },
                )
                .drop_without_applying_deltas();
        };
        render(true, Vec::new());
        render(true, vec![egui::Event::Text("dialog".into())]);
        for _ in 0..3 {
            render(false, Vec::new());
        }
        render(false, vec![egui::Event::Text("i".into())]);
        render(false, vec![egui::Event::Text("x".into())]);
        assert_eq!(source.to_string(), "xoriginal");
        assert_eq!(dialog_text, "dialog");
        assert!(context.memory(|memory| memory.has_focus(egui::Id::unique("luau_source"))));
        assert!(matches!(state.focus_request, FocusRequest::None));
    }

    #[test]
    fn accepting_incremental_search_preserves_match_and_returns_focus() {
        fn render(
            context: &egui::Context,
            source: &mut Buffer,
            state: &mut State,
            events: Vec<egui::Event>,
        ) {
            context
                .run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(740.0, 600.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        show(ui, source, state);
                    },
                )
                .drop_without_applying_deltas();
        }
        let context = egui::Context::default();
        let mut source = Buffer::from("width second width");
        let mut state = State::default();
        state.modal.enable(true);
        state.open_search(false, false);
        render(&context, &mut source, &mut state, Vec::new());
        render(
            &context,
            &mut source,
            &mut state,
            vec![egui::Event::Text("w".into())],
        );
        for _ in 0..1000 {
            render(&context, &mut source, &mut state, Vec::new());
            if state.cursor == 1 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(state.selection(&source), 0..1);
        render(
            &context,
            &mut source,
            &mut state,
            vec![egui::Event::Text("idth".into())],
        );
        for _ in 0..1000 {
            render(&context, &mut source, &mut state, Vec::new());
            if state.cursor == 5 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(state.selection(&source), 0..5);
        render(
            &context,
            &mut source,
            &mut state,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(state.selection(&source), 0..5);
        assert!(!state.search.is_open());
        assert_eq!(
            context.memory(|memory| memory.focused()),
            Some(egui::Id::unique("luau_source"))
        );
        assert_eq!(source.to_string(), "width second width");
    }

    #[test]
    fn focused_editor_leaves_escape_and_text_for_open_menus() {
        let context = egui::Context::default();
        let mut source = Buffer::from("unchanged");
        let mut state = State::default();
        state.modal.enable(true);
        state.focus_request = FocusRequest::Ready;
        context
            .run_ui(egui::RawInput::default(), |ui| {
                show(ui, &mut source, &mut state);
            })
            .drop_without_applying_deltas();
        egui::Popup::open_id(&context, egui::Id::unique("menu"));
        context
            .run_ui(
                egui::RawInput {
                    events: vec![
                        egui::Event::Text("x".into()),
                        egui::Event::Key {
                            key: egui::Key::Escape,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ui| {
                    show(ui, &mut source, &mut state);
                    assert!(ui.input(|input| input.key_pressed(egui::Key::Escape)));
                },
            )
            .drop_without_applying_deltas();
        assert_eq!(source.to_string(), "unchanged");
    }

    #[test]
    fn clicking_above_the_caret_keeps_rendered_cursor_positions_consistent() {
        let context = egui::Context::default();
        let mut buffer = Buffer::from("first line\nsecond line\nthird line\nlast line");
        let mut state = State::default();
        state.jump_to(buffer.rope.line_to_char(3)..buffer.rope.line_to_char(3));
        let mut render = |events| {
            context
                .run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(440.0, 240.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        show(ui, &mut buffer, &mut state);
                    },
                )
                .drop_without_applying_deltas();
        };
        render(Vec::new());
        let pointer = egui::pos2(90.0, 10.0);
        render(vec![
            egui::Event::PointerMoved(pointer),
            egui::Event::PointerButton {
                pos: pointer,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        render(vec![egui::Event::PointerButton {
            pos: pointer,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert_eq!(buffer.rope.char_to_line(state.cursor), 0);
        assert_eq!(state.anchor, state.cursor);
    }

    #[test]
    fn clipboard_selection_undo_and_document_navigation_preserve_text() {
        let context = egui::Context::default();
        let mut buffer = Buffer::from("héllo\n\n");
        let initial_revision = buffer.revision;
        let mut state = State::default();
        let mut render = |events| {
            context
                .run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(440.0, 800.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        show(ui, &mut buffer, &mut state);
                    },
                )
                .drop_without_applying_deltas();
        };
        render(Vec::new());
        context.memory_mut(|memory| memory.request_focus(egui::Id::unique("luau_source")));
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        };
        render(vec![key(egui::Key::End), egui::Event::Paste("世界".into())]);
        render(vec![key(egui::Key::A), egui::Event::Cut]);
        render(vec![key(egui::Key::Z), key(egui::Key::Z)]);
        assert_eq!(buffer.to_string(), "héllo\n\n");
        assert_eq!(buffer.revision, initial_revision);
    }

    #[test]
    fn highlighting_preserves_unicode_and_long_literals() {
        for source in [
            "--[=[λ comment]=]\nlocal name = 'héllo'",
            "local text = [==[a\n'b'\n]==]",
            "",
        ] {
            let buffer = Buffer::from(source);
            let job = crate::syntax::State::default().layout(
                &buffer,
                0..source.len(),
                FontId::monospace(14.0),
            );
            assert_eq!(job.text, source);
            for section in job.sections {
                assert!(source.is_char_boundary(section.byte_range.start.into()));
                assert!(source.is_char_boundary(section.byte_range.end.into()));
            }
        }
    }

    #[test]
    fn layout_is_bounded_by_viewport_for_large_files_and_long_lines() {
        for source in [
            "local width = 100 -- λ\n".repeat(100_000),
            "x".repeat(100_000),
        ] {
            let mut buffer = Buffer::from(source);
            let mut state = State::default();
            egui::__run_test_ui(|ui| {
                ui.set_max_size(egui::vec2(440.0, 800.0));
                show(ui, &mut buffer, &mut state);
                assert!(state.lines.len() <= 42);
                let characters: usize = state.lines.values().map(|line| line.job.text.len()).sum();
                assert!(characters < 10_000);
            });
        }
    }
}
#[test]
fn smart_indent_is_bounded_and_undoable_and_control_c_leaves_insert() {
    let context = egui::Context::default();
    let mut source = Buffer::from("    if ready then");
    let mut state = State::default();
    state.modal.enable(true);
    state.modal.mode = crate::modal::Mode::Insert;
    state.cursor = source.rope.len_chars();
    state.anchor = state.cursor;
    state.newline(&mut source);
    assert_eq!(source.to_string(), "    if ready then\n        ");
    state.focus_request = FocusRequest::Ready;
    context
        .run_ui(egui::RawInput::default(), |ui| {
            show(ui, &mut source, &mut state);
        })
        .drop_without_applying_deltas();
    context
        .run_ui(
            egui::RawInput {
                events: vec![egui::Event::Copy],
                ..Default::default()
            },
            |ui| {
                show(ui, &mut source, &mut state);
            },
        )
        .drop_without_applying_deltas();
    assert!(state.modal.mode == crate::modal::Mode::Normal);
    assert_eq!(source.to_string(), "    if ready then\n        ");
    assert!(source.undo(false).is_some());
    assert_eq!(source.to_string(), "    if ready then");
    let mut source = Buffer::from(" ".repeat(100_000));
    state.cursor = source.rope.len_chars();
    state.anchor = state.cursor;
    state.newline(&mut source);
    assert_eq!(source.rope.len_chars(), 100_257);
}
