use crate::text_buffer::Buffer;
use eframe::egui;
use ropey::Rope;
use std::ops::Range;

const PARAMETER_COUNT_MAX: usize = 64;
const LITERAL_BYTES_MAX: usize = 128;

pub struct Parameter {
    name: String,
    literal: String,
    range: Range<usize>,
    line: u32,
}

pub fn extract(source: &Rope, tree: &tree_sitter::Tree) -> Vec<Parameter> {
    let mut parameters = Vec::new();
    let root = tree.root_node();
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
            || value.byte_range().len() > LITERAL_BYTES_MAX
        {
            continue;
        }
        if value.kind() != "number" {
            if value.kind() != "unary_expression"
                || !value
                    .child_by_field_name("operand")
                    .is_some_and(|node| node.kind() == "number")
            {
                continue;
            }
        }
        let literal = source.byte_slice(value.byte_range()).to_string();
        if number(&literal).is_err() {
            continue;
        }
        parameters.push(Parameter {
            name: source.byte_slice(name.byte_range()).to_string(),
            literal,
            range: source.byte_to_char(value.start_byte())..source.byte_to_char(value.end_byte()),
            line: value.start_position().row as u32 + 1,
        });
    }
    parameters
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
    revision: u64,
    range: Range<usize>,
    literal: String,
}

impl Edit {
    pub fn apply(&self, source: &mut Buffer, cursor: usize) -> Result<Range<usize>, &'static str> {
        number(&self.literal)?;
        if source.revision != self.revision {
            return Err("Source changed; update the parameter again");
        }
        assert!(self.range.start <= self.range.end);
        assert!(self.range.end <= source.rope.len_chars());
        if !source.replace(self.range.clone(), &self.literal, cursor) {
            return Err("Parameter edit exceeds the document limit");
        }
        Ok(self.range.start..self.range.start + self.literal.chars().count())
    }
}

#[derive(Default)]
pub struct State {
    revision: u64,
    drafts: Vec<String>,
    error: Option<&'static str>,
}

impl State {
    pub fn error(&self) -> Option<&'static str> {
        self.error
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        revision: u64,
        parameters: Option<&[Parameter]>,
        commit: bool,
    ) -> Option<Edit> {
        crate::interface::section(ui, "PARAMETERS");
        if self.revision != revision {
            self.error = None;
        }
        let Some(parameters) = parameters else {
            ui.colored_label(crate::interface::MUTED, "Reading source parameters…");
            return None;
        };
        if self.revision != revision {
            self.revision = revision;
            self.drafts = parameters
                .iter()
                .map(|parameter| parameter.literal.clone())
                .collect();
            self.error = None;
        }
        assert_eq!(self.drafts.len(), parameters.len());
        if parameters.is_empty() {
            ui.small("Define a top-level number: local width = 100");
            return None;
        }
        let mut edit = None;
        let label_width = (ui.available_width() - 110.0).max(40.0);
        egui::Grid::new("source_parameters")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                for (index, parameter) in parameters.iter().enumerate() {
                    ui.add_sized(
                        egui::vec2(label_width, crate::interface::CONTROL_HEIGHT),
                        egui::Label::new(&parameter.name)
                            .truncate()
                            .halign(egui::Align::Min),
                    )
                    .on_hover_text(format!(
                        "{} · source line {}",
                        parameter.name, parameter.line
                    ));
                    let draft = &mut self.drafts[index];
                    let response = ui
                        .add(
                            egui::TextEdit::singleline(draft)
                                .id_salt((&parameter.name, parameter.line))
                                .char_limit(LITERAL_BYTES_MAX)
                                .desired_width(76.0)
                                .min_size(egui::vec2(76.0, crate::interface::CONTROL_HEIGHT))
                                .margin(egui::vec2(10.0, 8.0)),
                        )
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
                    if !cancel
                        && (enter || response.lost_focus() || commit && response.has_focus())
                        && *draft != parameter.literal
                        && edit.is_none()
                    {
                        match number(draft) {
                            Ok(_) => {
                                self.error = None;
                                edit = Some(Edit {
                                    revision,
                                    range: parameter.range.clone(),
                                    literal: draft.trim().to_owned(),
                                })
                            }
                            Err(error) => self.error = Some(error),
                        }
                    }
                    ui.end_row();
                }
            });
        self.error = self.drafts.iter().find_map(|draft| number(draft).err());
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
            ui.small("Showing the first 64 literal parameters.");
        }
        edit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Vec<Parameter> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_luau::LANGUAGE.into())
            .expect("Luau grammar");
        let tree = parser.parse(text, None).expect("Tree");
        extract(&Rope::from_str(text), &tree)
    }

    #[test]
    fn extracts_only_top_level_literal_numbers_without_touching_expressions() {
        let text = "-- λ\nlocal width: number = 310 -- width\nlocal gap = -1.5\nlocal size = 1e2\nlocal depth = width * 2\nlocal text = 'local fake = 2'\nlocal a,b = 1,2\nlocal function shape() local nested = 3 end\nlocal bad = 0/0\n";
        let parameters = parse(text);
        assert_eq!(
            parameters
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect::<Vec<_>>(),
            ["width", "gap", "size"]
        );
        let source = Rope::from_str(text);
        for parameter in parameters {
            assert_eq!(source.slice(parameter.range).to_string(), parameter.literal);
        }
        assert!(parse("local value = \n").is_empty());
    }

    #[test]
    fn parameter_edit_preserves_source_is_one_undo_and_rejects_stale_revisions() {
        let text = "-- λ\nlocal width: number = 310 -- dimension\nlocal result = width * 2";
        let mut source = Buffer::from(text);
        let parameter = parse(text).remove(0);
        let edit = Edit {
            revision: source.revision,
            range: parameter.range,
            literal: "325.5".into(),
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
            parse(&"local width = 10\n".repeat(100)).len(),
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
        };
        assert!(edit.apply(&mut source, 0).is_err());
        assert_eq!(source.revision, revision);
        assert_eq!(source.rope.len_bytes(), crate::text_buffer::SIZE_BYTES_MAX);
        assert_eq!(source.rope.char(1), '1');
        assert!(source.undo(false).is_none());
    }
}
