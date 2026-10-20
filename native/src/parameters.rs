use crate::text_buffer::Buffer;
use eframe::egui;
use mlua::{Lua, Table, Value};
use ropey::Rope;
use std::ops::Range;

const PARAMETER_COUNT_MAX: usize = 64;
const LITERAL_BYTES_MAX: usize = 128;

#[derive(Clone)]
enum Control {
    Number {
        minimum: Option<f64>,
        maximum: Option<f64>,
        step: f64,
    },
    Toggle,
}

#[derive(Clone)]
struct Property {
    label: Option<String>,
    unit: String,
    control: Control,
}

impl Property {
    fn from_table(toggle: bool, options: &Table) -> mlua::Result<Self> {
        let allowed: &[&str] = if toggle {
            &["label"]
        } else {
            &["label", "unit", "min", "max", "step"]
        };
        for pair in options.pairs::<String, Value>() {
            let (key, _) = pair?;
            if !allowed.contains(&key.as_str()) {
                return Err(mlua::Error::runtime(format!(
                    "Unknown property option: {key}"
                )));
            }
        }
        let label = options.get::<Option<String>>("label")?;
        let unit = options.get::<Option<String>>("unit")?.unwrap_or_default();
        if label.as_ref().is_some_and(|label| {
            label.is_empty() || label.len() > 80 || label.chars().any(char::is_control)
        }) {
            return Err(mlua::Error::runtime(
                "Property label needs 1–80 bytes without control characters",
            ));
        }
        if unit.len() > 16 || unit.chars().any(char::is_control) {
            return Err(mlua::Error::runtime(
                "Property unit needs at most 16 bytes without control characters",
            ));
        }
        let control = if toggle {
            Control::Toggle
        } else {
            let minimum = options.get::<Option<f64>>("min")?;
            let maximum = options.get::<Option<f64>>("max")?;
            let step = options.get::<Option<f64>>("step")?.unwrap_or(0.01);
            if minimum.is_some_and(|value| !value.is_finite())
                || maximum.is_some_and(|value| !value.is_finite())
            {
                return Err(mlua::Error::runtime("Property bounds must be finite"));
            }
            if let (Some(minimum), Some(maximum)) = (minimum, maximum) {
                if minimum >= maximum || !(maximum - minimum).is_finite() {
                    return Err(mlua::Error::runtime(
                        "Property min must be less than max with a finite span",
                    ));
                }
            }
            if !step.is_finite() || step <= 0.0 {
                return Err(mlua::Error::runtime(
                    "Property step must be finite and positive",
                ));
            }
            Control::Number {
                minimum,
                maximum,
                step,
            }
        };
        Ok(Self {
            label,
            unit,
            control,
        })
    }

    fn validate(&self, literal: &str) -> Result<(), &'static str> {
        match self.control {
            Control::Toggle => match literal {
                "true" | "false" => Ok(()),
                _ => Err("Toggle needs true or false"),
            },
            Control::Number {
                minimum, maximum, ..
            } => {
                let value = number(literal)?;
                if minimum.is_some_and(|minimum| value < minimum)
                    || maximum.is_some_and(|maximum| value > maximum)
                {
                    return Err("Value is outside the declared property range");
                }
                Ok(())
            }
        }
    }
}

pub fn register(lua: &Lua, cad: &Table) -> mlua::Result<()> {
    let count = std::rc::Rc::new(std::cell::Cell::new(0_usize));
    for (name, toggle) in [("number", false), ("toggle", true)] {
        let count = std::rc::Rc::clone(&count);
        cad.set(
            name,
            lua.create_function(move |_, (value, options): (Value, Table)| {
                if count.get() >= PARAMETER_COUNT_MAX {
                    return Err(mlua::Error::runtime("Model exceeds 64 properties"));
                }
                let property = Property::from_table(toggle, &options)?;
                let literal = match value {
                    Value::Boolean(value) if toggle => value.to_string(),
                    Value::Number(value) if !toggle => format!("{value:e}"),
                    Value::Integer(value) if !toggle => format!("{:e}", value as f64),
                    _ => return Err(mlua::Error::runtime("Property value has the wrong type")),
                };
                property.validate(&literal).map_err(mlua::Error::runtime)?;
                count.set(count.get() + 1);
                Ok(value)
            })?,
        )?;
    }
    Ok(())
}

#[derive(Clone)]
pub struct Parameter {
    pub name: String,
    literal: String,
    pub range: Range<usize>,
    pub line: u32,
    property: Property,
}

fn parse_source(source: &str) -> Option<(Rope, tree_sitter::Tree)> {
    if source.len() > crate::text_buffer::SIZE_BYTES_MAX {
        return None;
    }
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_luau::LANGUAGE.into())
        .ok()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(50);
    let mut cancelled = |_: &tree_sitter::ParseState| std::time::Instant::now() >= deadline;
    let tree = parser.parse_with_options(
        &mut |offset, _| source.as_bytes().get(offset..).unwrap_or_default(),
        None,
        Some(tree_sitter::ParseOptions::new().progress_callback(&mut cancelled)),
    )?;
    Some((Rope::from_str(source), tree))
}

impl Parameter {
    pub fn is_numeric(&self) -> bool {
        matches!(self.property.control, Control::Number { .. })
    }

    fn edit(&self, revision: u64, literal: String) -> Result<Edit, &'static str> {
        self.property.validate(&literal)?;
        Ok(Edit {
            input_at: None,
            revision,
            range: self.range.clone(),
            literal,
            property: self.property.clone(),
            group_revision: None,
        })
    }
}

pub fn analyze_source(source: &str) -> Option<(String, Vec<Parameter>)> {
    let (rope, tree) = parse_source(source)?;
    let mut key = String::with_capacity(source.len());
    let mut previous = 0;
    let parameters = extract(&rope, &tree);
    for parameter in &parameters {
        if matches!(parameter.property.control, Control::Number { .. }) {
            let start = rope.char_to_byte(parameter.range.start);
            let end = rope.char_to_byte(parameter.range.end);
            key.push_str(&source[previous..start]);
            key.push('0');
            previous = end;
        }
    }
    key.push_str(&source[previous..]);
    Some((key, parameters))
}

pub fn extract(source: &Rope, tree: &tree_sitter::Tree) -> Vec<Parameter> {
    let mut parameters = Vec::new();
    let root = tree.root_node();
    let mut lua = None;
    let mut cursor = root.walk();
    for declaration in root.named_children(&mut cursor).take(4096) {
        if parameters.len() == PARAMETER_COUNT_MAX {
            break;
        }
        if declaration.kind() != "variable_declaration" || declaration.has_error() {
            continue;
        }
        let Some(assignment) = declaration.named_child(0) else {
            continue;
        };
        if assignment.kind() != "assignment_statement" {
            continue;
        }
        let (Some(variables), Some(values)) =
            (assignment.named_child(0), assignment.named_child(1))
        else {
            continue;
        };
        let Some(name) = variables.child_by_field_name("name") else {
            continue;
        };
        let Some(value) = values.child_by_field_name("value") else {
            continue;
        };
        if variables
            .children_by_field_name("name", &mut variables.walk())
            .count()
            != 1
            || values
                .children_by_field_name("value", &mut values.walk())
                .count()
                != 1
            || name.kind() != "identifier"
            || name.byte_range().len() > 128
            || value.byte_range().len() > 4096
        {
            continue;
        }
        let Some((value, property)) = extract_property(source, value, &mut lua) else {
            continue;
        };
        let literal = source.byte_slice(value.byte_range()).to_string();
        if property.validate(&literal).is_err() {
            continue;
        }
        parameters.push(Parameter {
            name: source.byte_slice(name.byte_range()).to_string(),
            literal,
            range: source.byte_to_char(value.start_byte())..source.byte_to_char(value.end_byte()),
            line: value.start_position().row as u32 + 1,
            property,
        });
    }
    parameters
}

fn extract_property<'a>(
    source: &Rope,
    call: tree_sitter::Node<'a>,
    lua: &mut Option<Lua>,
) -> Option<(tree_sitter::Node<'a>, Property)> {
    if call.kind() != "function_call" {
        return None;
    }
    let name = call.child_by_field_name("name")?;
    if name.kind() != "dot_index_expression" {
        return None;
    }
    if source.byte_slice(name.child_by_field_name("table")?.byte_range()) != "cad" {
        return None;
    }
    let field = source
        .byte_slice(name.child_by_field_name("field")?.byte_range())
        .to_string();
    let toggle = match field.as_str() {
        "number" => false,
        "toggle" => true,
        _ => return None,
    };
    let arguments = call.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let arguments: Vec<_> = arguments
        .named_children(&mut cursor)
        .filter(|node| node.kind() != "comment")
        .take(3)
        .collect();
    let [value, options] = arguments.as_slice() else {
        return None;
    };
    let literal = source.byte_slice(value.byte_range()).to_string();
    if toggle {
        if !matches!(literal.as_str(), "true" | "false") {
            return None;
        }
    } else if number(&literal).is_err() {
        return None;
    }
    if options.kind() != "table_constructor" {
        return None;
    }
    let mut cursor = options.walk();
    let mut count = 0;
    for field in options
        .named_children(&mut cursor)
        .filter(|node| node.kind() != "comment")
    {
        count += 1;
        if count > 5 || field.kind() != "field" {
            return None;
        }
        let name = field.child_by_field_name("name")?;
        if name.kind() != "identifier" {
            return None;
        }
        let value = field.child_by_field_name("value")?;
        if value.kind() != "string" {
            number(&source.byte_slice(value.byte_range()).to_string()).ok()?;
        }
    }
    let options = source.byte_slice(options.byte_range()).to_string();
    let lua = lua.get_or_insert_with(Lua::new);
    lua.set_memory_limit(1024 * 1024).ok()?;
    let table: Table = lua.load(format!("return {options}")).eval().ok()?;
    Some((*value, Property::from_table(toggle, &table).ok()?))
}

fn number(text: &str) -> Result<f64, &'static str> {
    if text.len() > LITERAL_BYTES_MAX {
        return Err("Number is too long");
    }
    let text = text.trim();
    if text.is_empty() || text.starts_with('+') || text.chars().any(char::is_whitespace) {
        return Err("Enter a decimal number, such as 42 or -1.5");
    }
    let value = text
        .parse::<f64>()
        .map_err(|_| "Enter a decimal number, such as 42 or -1.5")?;
    if !value.is_finite() {
        return Err("The number must be finite");
    }
    Ok(value)
}

pub struct Edit {
    pub input_at: Option<std::time::Instant>,
    revision: u64,
    range: Range<usize>,
    literal: String,
    property: Property,
    group_revision: Option<u64>,
}

impl Edit {
    pub fn apply(&self, source: &mut Buffer, cursor: usize) -> Result<Range<usize>, &'static str> {
        self.property.validate(&self.literal)?;
        if source.revision != self.revision {
            return Err("Source changed; update the parameter again");
        }
        assert!(self.range.start <= self.range.end);
        assert!(self.range.end <= source.rope.len_chars());
        if !source.replace(self.range.clone(), &self.literal, cursor) {
            return Err("Parameter edit exceeds the document limit");
        }
        if let Some(revision) = self.group_revision {
            source.merge_undo(revision);
        }
        Ok(self.range.start..self.range.start + self.literal.chars().count())
    }
}

pub struct DimensionEditor {
    revision: u64,
    parameter: Parameter,
    measurement: String,
    measured_mm: f32,
    draft: String,
    focus: bool,
}

impl DimensionEditor {
    pub fn new(revision: u64, parameter: Parameter, measurement: &str, measured_mm: f32) -> Self {
        assert!(parameter.is_numeric());
        assert!(measured_mm.is_finite());
        let draft = parameter.literal.clone();
        Self {
            revision,
            parameter,
            measurement: measurement.into(),
            measured_mm,
            draft,
            focus: true,
        }
    }

    pub fn current(&self, revision: u64) -> bool {
        self.revision == revision
    }

    fn commit(&self, revision: u64) -> Result<Option<Edit>, &'static str> {
        if !self.current(revision) {
            return Err("Source changed; reopen the measurement");
        }
        if self.draft == self.parameter.literal {
            return Ok(None);
        }
        self.parameter.edit(revision, self.draft.clone()).map(Some)
    }

    pub fn show(
        &mut self,
        context: &egui::Context,
        ready: bool,
    ) -> Result<(Option<Edit>, bool), &'static str> {
        let mut edit = Ok(None);
        let mut close = false;
        let response =
            egui::Modal::new(egui::Id::unique("dimension_parameter_editor")).show(context, |ui| {
                ui.set_width(320.0);
                let enter = ui
                    .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                ui.heading("Edit linked parameter");
                ui.label(format!(
                    "Measured {}: ≈{:.3} mm",
                    self.measurement, self.measured_mm
                ));
                ui.small(format!("Parameter: {}", self.parameter.name));
                ui.horizontal(|ui| {
                    let mut field = egui::TextEdit::singleline(&mut self.draft)
                        .id(egui::Id::unique("dimension_parameter_value"))
                        .char_limit(LITERAL_BYTES_MAX)
                        .desired_width(220.0)
                        .min_size(egui::vec2(220.0, crate::interface::CONTROL_HEIGHT))
                        .margin(egui::vec2(10.0, 8.0))
                        .show(ui);
                    if self.focus {
                        field.response.request_focus();
                        field
                            .state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                egui::text::CCursor::new(self.draft.chars().count()),
                            )));
                        field.state.store(context, field.response.id);
                        self.focus = false;
                    }
                    ui.label(&self.parameter.property.unit);
                });
                let validation = self.parameter.property.validate(&self.draft);
                let status = validation.err().unwrap_or(if ready {
                    "Enter to apply · Esc to cancel"
                } else {
                    "Waiting for preview and source controls…"
                });
                ui.add_sized(
                    egui::vec2(320.0, 36.0),
                    egui::Label::new(egui::RichText::new(status).color(if validation.is_err() {
                        crate::interface::WARNING
                    } else {
                        crate::interface::MUTED
                    }))
                    .wrap(),
                );
                ui.horizontal(|ui| {
                    let apply = ui
                        .add_enabled(
                            ready && validation.is_ok() && self.draft != self.parameter.literal,
                            egui::Button::new("Apply"),
                        )
                        .clicked();
                    if ready && validation.is_ok() && (apply || enter) {
                        edit = self.commit(self.revision);
                        close = edit.is_ok();
                    }
                    close |= ui.button("Cancel").clicked();
                });
            });
        edit.map(|edit| (edit, close || response.should_close()))
    }
}

#[derive(Default)]
pub struct State {
    revision: u64,
    parameters: Vec<Parameter>,
    drafts: Vec<String>,
    error: Option<&'static str>,
    groups: Vec<Option<u64>>,
    input_times: Vec<Option<std::time::Instant>>,
    dragging: Option<usize>,
    highlighted: Option<String>,
    reveal_highlight: bool,
}

impl State {
    pub fn ready(&self, revision: u64) -> bool {
        self.revision == revision && !self.parameters.is_empty()
    }

    pub fn reveal(&mut self, name: Option<&str>) {
        self.highlighted = name.map(str::to_owned);
        self.reveal_highlight = name.is_some();
    }

    pub fn edit_literal(
        &mut self,
        revision: u64,
        name: &str,
        literal: String,
    ) -> Result<Option<Edit>, &'static str> {
        if self.revision != revision || self.parameters.is_empty() {
            return Ok(None);
        }
        let index = self
            .parameters
            .iter()
            .position(|parameter| parameter.name == name)
            .ok_or("Parameter does not exist")?;
        let parameter = &self.parameters[index];
        let edit = parameter.edit(revision, literal)?;
        self.drafts[index].clone_from(&edit.literal);
        Ok(Some(edit))
    }

    pub fn applied(&mut self, edit: &Edit, revision: u64) {
        assert_eq!(self.revision, edit.revision);
        assert_ne!(self.revision, revision);
        let end = edit.range.start + edit.literal.chars().count();
        for (index, parameter) in self.parameters.iter_mut().enumerate() {
            if parameter.range == edit.range {
                self.drafts[index].clone_from(&edit.literal);
                parameter.range.end = end;
                parameter.literal.clone_from(&edit.literal);
            } else if parameter.range.start >= edit.range.end {
                parameter.range = (parameter.range.start - edit.range.end + end)
                    ..(parameter.range.end - edit.range.end + end);
            }
        }
        self.revision = revision;
    }

    pub fn error(&self) -> Option<&'static str> {
        self.error
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        revision: u64,
        parameters: Option<&[Parameter]>,
        commit: bool,
        apply_pending: bool,
    ) -> Option<Edit> {
        crate::interface::section(ui, "PARAMETERS");
        if self.revision != revision {
            self.error = None;
        }
        if self.revision != revision {
            let Some(parameters) = parameters else {
                ui.colored_label(crate::interface::MUTED, "Reading source parameters…");
                return None;
            };
            self.revision = revision;
            self.parameters.clear();
            self.parameters.extend_from_slice(parameters);
            self.drafts = parameters
                .iter()
                .map(|parameter| parameter.literal.clone())
                .collect();
            self.error = None;
            self.groups.clear();
            self.groups.resize(parameters.len(), None);
            self.input_times.clear();
            self.input_times.resize(parameters.len(), None);
            self.dragging = None;
        }
        let parameters = &self.parameters;
        if !ui.input(|input| input.pointer.any_down()) {
            self.dragging = None;
        }
        assert_eq!(self.drafts.len(), parameters.len());
        if parameters.is_empty() {
            ui.small("Expose a property: local width = cad.number(100, {})");
            return None;
        }
        let mut edit = None;
        let label_width = (ui.available_width() * 0.40).max(40.0);
        let control_width = (ui.available_width() - label_width - 10.0).max(120.0);
        egui::Grid::new("source_parameters")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                for (index, parameter) in parameters.iter().enumerate() {
                    let highlighted = self.highlighted.as_deref() == Some(parameter.name.as_str());
                    let label = ui
                        .add_sized(
                            egui::vec2(label_width, crate::interface::CONTROL_HEIGHT),
                            egui::Label::new(
                                egui::RichText::new(
                                    parameter.property.label.as_ref().unwrap_or(&parameter.name),
                                )
                                .color(if highlighted {
                                    crate::interface::ACCENT
                                } else {
                                    ui.visuals().text_color()
                                }),
                            )
                            .truncate()
                            .halign(egui::Align::Min),
                        )
                        .on_hover_text(format!(
                            "{} · source line {}",
                            parameter.name, parameter.line
                        ));
                    if highlighted && self.reveal_highlight {
                        label.scroll_to_me(Some(egui::Align::Center));
                        self.reveal_highlight = false;
                    }
                    let draft = &mut self.drafts[index];
                    let mut immediate = false;
                    let response = match parameter.property.control {
                        Control::Toggle => {
                            let mut value = *draft == "true";
                            let response = ui.checkbox(&mut value, "");
                            if response.changed() {
                                *draft = value.to_string();
                                immediate = true;
                            }
                            response
                        }
                        Control::Number {
                            minimum: Some(minimum),
                            maximum: Some(maximum),
                            step,
                        } => {
                            let mut value = number(draft).unwrap_or(minimum);
                            ui.spacing_mut().slider_width = (control_width - 96.0).max(32.0);
                            let suffix = if parameter.property.unit.is_empty() {
                                String::new()
                            } else {
                                format!(" {}", parameter.property.unit)
                            };
                            let response = ui.add(
                                egui::Slider::new(&mut value, minimum..=maximum)
                                    .step_by(step)
                                    .max_decimals(
                                        if step.fract() == 0.0
                                            && minimum.fract() == 0.0
                                            && maximum.fract() == 0.0
                                        {
                                            0
                                        } else {
                                            16
                                        },
                                    )
                                    .suffix(suffix),
                            );
                            if response.changed() {
                                *draft = value.to_string();
                                self.input_times[index] = Some(std::time::Instant::now());
                                if response.is_pointer_button_down_on() {
                                    if self.dragging != Some(index) {
                                        self.groups[index] = Some(revision);
                                        self.dragging = Some(index);
                                    }
                                } else if !response.drag_stopped() {
                                    self.groups[index] = None;
                                }
                            }
                            immediate = apply_pending || commit;
                            response
                        }
                        Control::Number { .. } => {
                            ui.horizontal(|ui| {
                                let response = ui.add(
                                    egui::TextEdit::singleline(draft)
                                        .id_salt((&parameter.name, parameter.line))
                                        .char_limit(LITERAL_BYTES_MAX)
                                        .desired_width(76.0)
                                        .min_size(egui::vec2(
                                            76.0,
                                            crate::interface::CONTROL_HEIGHT,
                                        ))
                                        .margin(egui::vec2(10.0, 8.0)),
                                );
                                if !parameter.property.unit.is_empty() {
                                    ui.label(&parameter.property.unit);
                                }
                                response
                            })
                            .inner
                        }
                    }
                    .on_hover_text("Enter or leave the field to apply · Esc to cancel");
                    let cancel = response.has_focus()
                        && ui.input_mut(|input| {
                            input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                        });
                    if cancel {
                        draft.clone_from(&parameter.literal);
                        self.error = None;
                        response.surrender_focus();
                    }
                    let enter = response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    let slider = matches!(
                        parameter.property.control,
                        Control::Number {
                            minimum: Some(_),
                            maximum: Some(_),
                            ..
                        }
                    );
                    if !cancel
                        && (!slider || apply_pending || commit)
                        && (immediate
                            || enter
                            || response.lost_focus()
                            || commit && response.has_focus())
                        && *draft != parameter.literal
                        && edit.is_none()
                    {
                        match parameter.property.validate(draft) {
                            Ok(_) => {
                                self.error = None;
                                edit = Some(Edit {
                                    input_at: self.input_times[index],
                                    revision,
                                    range: parameter.range.clone(),
                                    literal: draft.trim().to_owned(),
                                    property: parameter.property.clone(),
                                    group_revision: self.groups[index],
                                })
                            }
                            Err(error) => self.error = Some(error),
                        }
                    }
                    ui.end_row();
                }
            });
        self.error = self
            .drafts
            .iter()
            .zip(parameters)
            .find_map(|(draft, parameter)| parameter.property.validate(draft).err());
        if let Some(error) = self.error {
            edit = None;
            ui.colored_label(crate::interface::WARNING, error);
        }
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("Edits update source · Undo in code")
                .small()
                .color(crate::interface::MUTED),
        );
        if parameters.len() == PARAMETER_COUNT_MAX {
            ui.small("Showing the first 64 declared properties.");
        }
        edit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controls(
        state: &mut State,
        source: &Buffer,
        parameters: Option<&[Parameter]>,
        idle: bool,
    ) -> Option<Edit> {
        let mut edit = None;
        egui::Context::default()
            .run_ui(egui::RawInput::default(), |ui| {
                edit = state.show(ui, source.revision, parameters, false, idle);
            })
            .drop_without_applying_deltas();
        edit
    }

    fn dimension_frame(
        context: &egui::Context,
        editor: &mut DimensionEditor,
        events: Vec<egui::Event>,
        ready: bool,
    ) -> (Option<Edit>, bool) {
        let mut result = None;
        let mut source = Buffer::from("local height=cad.number(20,{min=1,max=25})");
        let mut code = crate::code_editor::State::default();
        code.modal.enable(true);
        code.jump_to(0..5);
        code.reveal_source(0..5);
        context
            .run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    result = Some(editor.show(ui.ctx(), ready));
                    crate::code_editor::show(ui, &mut source, &mut code);
                },
            )
            .drop_without_applying_deltas();
        result.unwrap().unwrap()
    }

    #[test]
    fn dimension_editor_keyboard_apply_cancel_and_busy_state_preserve_source_controls() {
        let text = "local height=cad.number(20,{min=1,max=25})";
        for (ready, value, key, apply, close) in [
            (true, "24", egui::Key::Enter, true, true),
            (true, "26", egui::Key::Enter, false, false),
            (false, "24", egui::Key::Enter, false, false),
            (true, "24", egui::Key::Escape, false, true),
        ] {
            let mut source = Buffer::from(text);
            let mut state = State::default();
            controls(&mut state, &source, Some(&parse(text)), true);
            let context = egui::Context::default();
            let mut editor =
                DimensionEditor::new(source.revision, parse(text).remove(0), "height", 20.0);
            for _ in 0..3 {
                assert!(
                    dimension_frame(&context, &mut editor, Vec::new(), ready)
                        .0
                        .is_none()
                );
            }
            dimension_frame(
                &context,
                &mut editor,
                vec![egui::Event::Text(value.into())],
                ready,
            );
            assert_eq!(editor.draft, value);
            let (edit, closed) = dimension_frame(
                &context,
                &mut editor,
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ready,
            );
            assert_eq!(closed, close);
            assert_eq!(edit.is_some(), apply);
            if let Some(edit) = edit {
                edit.apply(&mut source, 0).unwrap();
                state.applied(&edit, source.revision);
                assert_eq!(state.drafts[0], value);
                assert_eq!(state.parameters[0].literal, value);
                assert!(state.ready(source.revision));
                assert_eq!(source.to_string(), text.replace("(20,", "(24,"));
                assert!(source.undo(false).is_some());
            }
            assert_eq!(source.to_string(), text);
        }
    }

    #[test]
    fn dimension_editor_edits_parameter_once_and_rejects_invalid_or_stale_input() {
        let text = "-- λ\nlocal height=cad.number(20,{min=1,max=25,unit='mm'})";
        let mut source = Buffer::from(text);
        let mut editor =
            DimensionEditor::new(source.revision, parse(text).remove(0), "height", 20.0);
        assert!(editor.commit(source.revision).unwrap().is_none());
        for literal in ["26", "0", "NaN", "inf", "1+2", ""] {
            editor.draft = literal.into();
            assert!(editor.commit(source.revision).is_err(), "{literal}");
            assert_eq!(source.to_string(), text);
        }
        editor.draft = "24".into();
        let edit = editor
            .commit(source.revision)
            .unwrap()
            .expect("Changed parameter");
        edit.apply(&mut source, 0).unwrap();
        assert_eq!(
            source.to_string(),
            text.replace("cad.number(20,", "cad.number(24,")
        );
        assert!(editor.commit(source.revision).is_err());
        assert!(edit.apply(&mut source, 0).is_err());
        assert!(source.undo(false).is_some());
        assert_eq!(source.to_string(), text);
        assert!(source.undo(false).is_none());
    }

    #[test]
    fn dimension_editor_starts_from_parameter_value_instead_of_measured_formula_result() {
        let parameter = parse("local height=cad.number(20,{unit='mm'})").remove(0);
        let editor = DimensionEditor::new(1, parameter, "height doubled", 40.0);
        assert_eq!(editor.measured_mm, 40.0);
        assert_eq!(editor.draft, "20");
        assert!(editor.current(1));
        assert!(!editor.current(2));
    }

    #[test]
    fn programmatic_edits_validate_revisions_and_keep_controls_in_sync() {
        let text = "local separation=cad.number(12,{min=0,max=100,step=1})";
        let mut source = Buffer::from(text);
        let mut state = State::default();
        assert!(
            state
                .edit_literal(source.revision, "separation", "20".into())
                .unwrap()
                .is_none()
        );
        assert!(controls(&mut state, &source, Some(&parse(text)), true).is_none());
        assert!(
            state
                .edit_literal(source.revision, "separation", "101".into())
                .is_err()
        );
        assert!(
            state
                .edit_literal(source.revision, "missing", "20".into())
                .is_err()
        );
        assert_eq!(source.to_string(), text);
        let edit = state
            .edit_literal(source.revision, "separation", "20".into())
            .unwrap()
            .unwrap();
        edit.apply(&mut source, 0).unwrap();
        assert!(
            state
                .edit_literal(source.revision, "separation", "30".into())
                .unwrap()
                .is_none()
        );
        state.applied(&edit, source.revision);
        assert!(controls(&mut state, &source, None, true).is_none());
        assert_eq!(source.to_string(), text.replace("(12,", "(20,"));
    }

    #[test]
    fn slider_samples_coalesce_preserve_ranges_and_undo_the_whole_drag() {
        let text = "-- λ\nlocal width = cad.number(9, {min=0,max=2000})\nlocal gap = cad.number(2, {min=0,max=2000})";
        let mut source = Buffer::from(text);
        let revision = source.revision;
        let mut state = State::default();
        assert!(controls(&mut state, &source, Some(&parse(text)), true).is_none());
        state.groups[0] = Some(revision);
        for value in ["10", "99", "1000"] {
            state.drafts[0] = value.into();
            assert!(controls(&mut state, &source, None, false).is_none());
        }
        assert_eq!(source.to_string(), text);
        let edit = controls(&mut state, &source, None, true).expect("Latest pending value");
        assert_eq!(edit.literal, "1000");
        edit.apply(&mut source, 0).expect("First live sample");
        state.applied(&edit, source.revision);
        state.drafts[0] = "12".into();
        state.drafts[1] = "150".into();
        let edit = controls(&mut state, &source, None, true).expect("Second live sample");
        edit.apply(&mut source, 0).expect("Second live sample");
        state.applied(&edit, source.revision);
        let edit = controls(&mut state, &source, None, true).expect("Other pending slider");
        edit.apply(&mut source, 0).expect("Rebased later literal");
        state.applied(&edit, source.revision);
        assert_eq!(
            source.to_string(),
            text.replacen("(9,", "(12,", 1).replacen("(2,", "(150,", 1)
        );
        assert!(controls(&mut state, &source, None, true).is_none());
        source.undo(false).expect("Other slider undo");
        assert_eq!(source.to_string(), text.replacen("(9,", "(12,", 1));
        assert_eq!(source.undo(false), Some(0));
        assert_eq!(source.revision, revision);
        assert_eq!(source.to_string(), text);
        source.undo(true).expect("Whole drag redo");
        assert_eq!(source.to_string(), text.replacen("(9,", "(12,", 1));
        assert!(controls(&mut state, &source, None, true).is_none());
        assert!(controls(&mut state, &source, Some(&parse(&source.to_string())), true).is_none());
        assert_eq!(state.drafts, ["12", "2"]);
    }

    fn parse(text: &str) -> Vec<Parameter> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_luau::LANGUAGE.into())
            .expect("Luau grammar");
        let tree = parser.parse(text, None).expect("Tree");
        extract(&Rope::from_str(text), &tree)
    }

    #[test]
    fn extracts_only_declared_top_level_properties_without_executing_formulas() {
        let text = "-- λ\nlocal width: number = cad.number(310, {label='Rack width',min=100,max=480,step=1,unit='mm'}) -- width\nlocal gap = cad.number(-1.5, {})\nlocal size = cad.number(1e2, {})\nlocal print_layout = cad.toggle(false, {label='Print layout'})\nlocal hidden = 42\nlocal depth = cad.number(width * 2, {})\nlocal text = 'local fake = 2'\nlocal a,b = 1,2\nlocal function shape() local nested = cad.number(3,{}) end\nlocal bad = cad.number(1, {min=(function() error('must not run') end)()})\n";
        let parameters = parse(text);
        assert_eq!(
            parameters
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect::<Vec<_>>(),
            ["width", "gap", "size", "print_layout"]
        );
        assert_eq!(parameters[0].property.label.as_deref(), Some("Rack width"));
        assert_eq!(parameters[0].property.unit, "mm");
        assert!(matches!(parameters[3].property.control, Control::Toggle));
        let source = Rope::from_str(text);
        for parameter in parameters {
            assert_eq!(source.slice(parameter.range).to_string(), parameter.literal);
        }
        assert!(parse("local value = \n").is_empty());
    }

    #[test]
    fn parameter_edit_preserves_source_is_one_undo_and_rejects_stale_revisions() {
        let text = "-- λ\nlocal width: number = cad.number(310, {min=100,max=480}) -- dimension\nlocal result = width * 2";
        let mut source = Buffer::from(text);
        let parameter = parse(text).remove(0);
        let edit = Edit {
            revision: source.revision,
            range: parameter.range,
            literal: "325.5".into(),
            property: parameter.property,
            group_revision: None,
            input_at: None,
        };
        assert!(edit.apply(&mut source, 0).is_ok());
        assert_eq!(source.to_string(), text.replacen("310", "325.5", 1));
        assert!(edit.apply(&mut source, 0).is_err());
        assert_eq!(source.undo(false), Some(0));
        assert_eq!(source.to_string(), text);
        assert!(source.undo(true).is_some());
        for invalid in ["NaN", "inf", "1e999", "1 + 2", "+2", "--2", ""] {
            assert!(number(invalid).is_err(), "{invalid}");
        }
        assert_eq!(number("-1.5"), Ok(-1.5));
        assert_eq!(
            parse(&"local width = cad.number(10,{})\n".repeat(100)).len(),
            PARAMETER_COUNT_MAX
        );
    }

    #[test]
    fn oversized_parameter_edit_preserves_source_revision_and_history() {
        let mut source = Buffer::from("1".repeat(crate::text_buffer::SIZE_BYTES_MAX));
        let revision = source.revision;
        let edit = Edit {
            revision,
            range: 0..1,
            literal: "12".into(),
            property: parse("local width = cad.number(1,{})").remove(0).property,
            group_revision: None,
            input_at: None,
        };
        assert!(edit.apply(&mut source, 0).is_err());
        assert_eq!(source.revision, revision);
        assert_eq!(source.rope.len_bytes(), crate::text_buffer::SIZE_BYTES_MAX);
        assert_eq!(source.rope.char(1), '1');
        assert!(source.undo(false).is_none());
    }

    #[test]
    fn toggles_edit_only_the_literal_and_undo_restores_the_declaration() {
        let text = "-- λ\nlocal print_layout = cad.toggle(false, {label='Print layout'})";
        let parameter = parse(text).remove(0);
        let mut source = Buffer::from(text);
        let edit = Edit {
            revision: source.revision,
            range: parameter.range,
            literal: "true".into(),
            property: parameter.property,
            group_revision: None,
            input_at: None,
        };
        edit.apply(&mut source, 0).expect("Toggle edit");
        assert_eq!(source.to_string(), text.replace("false", "true"));
        assert_eq!(source.undo(false), Some(0));
        assert_eq!(source.to_string(), text);
    }

    #[test]
    fn runtime_and_inspector_reject_invalid_property_metadata_and_values() -> Result<(), String> {
        let lua = Lua::new();
        let cad = lua.create_table().map_err(|error| error.to_string())?;
        register(&lua, &cad).map_err(|error| error.to_string())?;
        lua.globals()
            .set("cad", cad)
            .map_err(|error| error.to_string())?;
        for declaration in [
            "cad.number(12,{min=0,max=10})",
            "cad.number(1,{min=10,max=0})",
            "cad.number(1,{step=0})",
            "cad.number(1,{step=-1})",
            "cad.number(1,{label=''})",
            "cad.number(1,{unknown=2})",
            "cad.toggle(true,{unit='mm'})",
        ] {
            assert!(
                lua.load(format!("return {declaration}"))
                    .eval::<Value>()
                    .is_err(),
                "{declaration}"
            );
            assert!(
                parse(&format!("local value = {declaration}")).is_empty(),
                "{declaration}"
            );
        }
        for declaration in [
            "cad.number(0/0,{})",
            "cad.number(1,{min=0/0})",
            "cad.number(1,{max=math.huge})",
            "cad.toggle(1,{})",
        ] {
            assert!(
                lua.load(format!("return {declaration}"))
                    .eval::<Value>()
                    .is_err(),
                "{declaration}"
            );
        }
        assert_eq!(
            lua.load("return cad.number(1.25,{min=1,max=2,step=0.1})")
                .eval::<f64>()
                .map_err(|error| error.to_string())?,
            1.25
        );
        assert!(
            !lua.load("return cad.toggle(false,{})")
                .eval::<bool>()
                .map_err(|error| error.to_string())?
        );
        let parameter = parse("local value = cad.number(2,{min=1,max=3})").remove(0);
        let mut source = Buffer::from("local value = cad.number(2,{min=1,max=3})");
        let revision = source.revision;
        let edit = Edit {
            revision,
            range: parameter.range,
            literal: "4".into(),
            property: parameter.property,
            group_revision: None,
            input_at: None,
        };
        assert!(edit.apply(&mut source, 0).is_err());
        assert_eq!(source.revision, revision);
        assert!(source.undo(false).is_none());
        Ok(())
    }
}
