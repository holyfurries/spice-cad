use crate::{
    assembly::Group,
    model_options::{Axis, Game, Options},
    placement::Placement,
    text_buffer::Buffer,
};
use eframe::egui;
use std::{collections::BTreeMap, ops::Range};
use tree_sitter::{Node, Query, QueryCursor, StreamingIterator};

pub struct Edit {
    pub revision: u64,
    pub range: Range<usize>,
    pub text: String,
}

enum GroupRange {
    Pose(Range<usize>),
    Field(usize),
    Options(usize),
}

struct SourceMap {
    options: Option<Range<usize>>,
    options_error: Option<String>,
    groups: BTreeMap<String, Option<GroupRange>>,
    insertion: usize,
    new_identifier: String,
}

fn literal_table(node: Node<'_>, source: &str) -> bool {
    if node.kind() != "table_constructor" || node.byte_range().len() > 4096 {
        return false;
    }
    let mut pending = vec![node];
    for _ in 0..128 {
        let Some(table) = pending.pop() else {
            return true;
        };
        for field in table
            .named_children(&mut table.walk())
            .filter(|n| n.kind() != "comment")
        {
            if field.kind() != "field" {
                return false;
            }
            let (Some(name), Some(value)) = (
                field.child_by_field_name("name"),
                field.child_by_field_name("value"),
            ) else {
                return false;
            };
            if name.kind() != "identifier" {
                return false;
            }
            if value.kind() == "table_constructor" {
                pending.push(value);
                continue;
            }
            let text = &source[value.byte_range()];
            if value.kind() == "string" && !crate::parameters::literal_string(text) {
                return false;
            }
            if value.kind() != "string"
                && text != "true"
                && text != "false"
                && text.parse::<f64>().is_err()
            {
                return false;
            }
        }
    }
    false
}

impl SourceMap {
    fn parse(source: &str) -> Option<Self> {
        let (rope, tree) = crate::parameters::parse_source(source, 262144)?;
        if tree.root_node().has_error() {
            return None;
        }
        let root = tree.root_node();
        let insertion = root
            .named_children(&mut root.walk())
            .find(|n| n.kind() != "comment")
            .map_or(rope.len_chars(), |n| rope.byte_to_char(n.start_byte()));
        let mut result = Self {
            options: None,
            options_error: None,
            groups: BTreeMap::new(),
            insertion,
            new_identifier: (1..=64)
                .map(|i| format!("_attachment_frame_{i}"))
                .find(|name| !source.contains(name))
                .unwrap_or_default(),
        };
        let query = Query::new(&tree_sitter_luau::LANGUAGE.into(), "(function_call name: (dot_index_expression table: (identifier) @table field: (identifier) @field) arguments: (arguments) @arguments) @call").ok()?;
        let mut cursor = QueryCursor::new();
        cursor.set_match_limit(4096);
        let mut matches = cursor.matches(&query, root, source.as_bytes());
        for _ in 0..4096 {
            let Some(found) = matches.next() else {
                return Some(result);
            };
            let capture = |name: &str| {
                found
                    .captures
                    .iter()
                    .find(|c| query.capture_names()[c.index as usize] == name)
                    .map(|c| c.node)
            };
            if &source[capture("table")?.byte_range()] != "cad" {
                continue;
            }
            let field = &source[capture("field")?.byte_range()];
            if field != "group" && field != "model_options" {
                continue;
            }
            let arguments = capture("arguments")?;
            let arguments: Vec<_> = arguments
                .named_children(&mut arguments.walk())
                .filter(|n| n.kind() != "comment")
                .take(3)
                .collect();
            if field == "model_options" {
                let call = capture("call")?;
                if result.options.is_some()
                    || call.parent().is_none_or(|p| p != root)
                    || arguments.len() != 1
                    || !literal_table(arguments[0], source)
                {
                    result.options_error = Some(
                        "Model options are driven by code; edit their source expressions.".into(),
                    );
                }
                result.options =
                    Some(rope.byte_to_char(call.start_byte())..rope.byte_to_char(call.end_byte()));
                continue;
            }
            let Some(name) = arguments
                .first()
                .filter(|n| n.kind() == "string" && n.byte_range().len() <= 256)
            else {
                continue;
            };
            if !crate::parameters::literal_string(&source[name.byte_range()]) {
                continue;
            }
            let lua = mlua::Lua::new();
            lua.set_memory_limit(1024 * 1024).ok()?;
            let Ok(name) = lua
                .load(format!("return {}", &source[name.byte_range()]))
                .eval::<String>()
            else {
                continue;
            };
            let range = match arguments.as_slice() {
                [name] => Some(GroupRange::Options(rope.byte_to_char(name.end_byte()))),
                [_, table] if table.kind() == "table_constructor" => {
                    let assembly: Vec<_> = table
                        .named_children(&mut table.walk())
                        .filter(|n| {
                            n.child_by_field_name("name")
                                .is_some_and(|n| &source[n.byte_range()] == "assembly")
                        })
                        .take(2)
                        .collect();
                    if let [field] = assembly.as_slice() {
                        field
                            .child_by_field_name("value")
                            .filter(|v| literal_table(*v, source))
                            .map(|v| {
                                GroupRange::Pose(
                                    rope.byte_to_char(v.start_byte())
                                        ..rope.byte_to_char(v.end_byte()),
                                )
                            })
                    } else if assembly.is_empty() {
                        Some(GroupRange::Field(rope.byte_to_char(table.start_byte() + 1)))
                    } else {
                        None
                    }
                }
                _ => None,
            };
            match result.groups.entry(name) {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.insert(None);
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(range);
                }
            }
        }
        None
    }

    fn options_edit(&self, revision: u64, options: Options) -> Result<Edit, String> {
        if let Some(error) = &self.options_error {
            return Err(error.clone());
        }
        options.placement()?;
        options.game.validate()?;
        let [x, y, z] = options.rotation;
        let game = options.game;
        let mut text = format!(
            "cad.model_options({{\n    rotation = {{x={x}, y={y}, z={z}}},\n    game = {{preset=\"{}\", forward=\"{}\", up=\"{}\", scale={}, ground={}}},\n}})",
            if game.crowns_adrift {
                "crowns_adrift"
            } else {
                "gltf"
            },
            game.forward.name(),
            game.up.name(),
            game.scale,
            game.ground
        );
        let range = self
            .options
            .clone()
            .unwrap_or(self.insertion..self.insertion);
        if self.options.is_none() {
            text.push_str("\n\n");
        }
        Ok(Edit {
            revision,
            range,
            text,
        })
    }

    fn pose_edit(
        &self,
        revision: u64,
        name: &str,
        position: [f64; 3],
        rotation: [f64; 3],
    ) -> Result<Edit, String> {
        Placement::from_components(position, rotation)?;
        let [x, y, z] = position;
        let [rx, ry, rz] = rotation;
        let pose =
            format!("{{position={{x={x}, y={y}, z={z}}}, rotation={{x={rx}, y={ry}, z={rz}}}}}");
        let (range, text) = match self.groups.get(name).and_then(Option::as_ref) {
            Some(GroupRange::Pose(range)) => (range.clone(), pose),
            Some(GroupRange::Field(index)) => (*index..*index, format!("assembly={pose}, ")),
            Some(GroupRange::Options(index)) => (*index..*index, format!(", {{assembly={pose}}}")),
            None => return Err("This frame is driven by code or a module; edit its source.".into()),
        };
        Ok(Edit {
            revision,
            range,
            text,
        })
    }

    fn new_frame(&self, revision: u64, name: &str, groups: &[Group]) -> Result<Edit, String> {
        if name.is_empty() || name.len() > 80 || name.chars().any(char::is_control) {
            return Err("Use a frame name of 1–80 bytes without control characters.".into());
        }
        if groups.len() >= 32 || groups.iter().any(|g| g.name == name) {
            return Err("Choose a unique frame name; at most 32 groups are supported.".into());
        }
        if self.new_identifier.is_empty() {
            return Err("No unused attachment variable name is available.".into());
        }
        let name = serde_json::to_string(name).map_err(|e| e.to_string())?;
        Ok(Edit {
            revision,
            range: self.insertion..self.insertion,
            text: format!(
                "local {} = cad.group({name}, {{assembly={{position={{x=0,y=0,z=0}},rotation={{x=0,y=0,z=0}}}}}})\n\n",
                self.new_identifier
            ),
        })
    }
}

#[derive(Default)]
pub struct Editor {
    pub open: bool,
    revision: Option<u64>,
    source: Option<SourceMap>,
    options: Options,
    selected: Option<usize>,
    position: [f64; 3],
    rotation: [f64; 3],
    new_name: String,
    error: Option<String>,
    page: Page,
    worlds: Vec<Placement>,
}

#[derive(Default, PartialEq)]
enum Page {
    #[default]
    Model,
    Game,
    Frames,
}

fn angles(ui: &mut egui::Ui, label: &str, values: &mut [f64; 3], position: bool) {
    ui.label(label);
    ui.horizontal(|ui| {
        for (name, value) in ["X", "Y", "Z"].into_iter().zip(values) {
            let limit = if position { 2000.0 } else { 360000.0 };
            ui.add(
                egui::DragValue::new(value)
                    .prefix(format!("{name} "))
                    .suffix(if position { " mm" } else { "°" })
                    .speed(if position { 0.1 } else { 1.0 })
                    .range(-limit..=limit),
            );
        }
    });
}

fn axes(ui: &mut egui::Ui, label: &str, transform: glam::Mat4) {
    ui.small(label);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 100.0),
        egui::Sense::hover(),
    );
    let center = rect.center();
    let project = |p: glam::Vec3| {
        egui::vec2(
            0.80 * p.x - 0.60 * p.y,
            0.30 * p.x + 0.40 * p.y - 0.86 * p.z,
        )
    };
    for (axis, name, color) in [
        (glam::Vec3::X, "X", egui::Color32::from_rgb(235, 110, 105)),
        (glam::Vec3::Y, "Y", egui::Color32::from_rgb(120, 200, 135)),
        (glam::Vec3::Z, "Z", egui::Color32::from_rgb(115, 165, 245)),
    ] {
        let direction = project(transform.transform_vector3(axis)) * 42.0;
        ui.painter()
            .arrow(center, direction, egui::Stroke::new(2.0, color));
        ui.painter().text(
            center + direction * 1.15,
            egui::Align2::CENTER_CENTER,
            name,
            egui::FontId::proportional(12.0),
            color,
        );
    }
}

impl Editor {
    pub fn open_game(&mut self) {
        self.open = true;
        self.page = Page::Game;
    }

    pub fn show(
        &mut self,
        context: &egui::Context,
        buffer: &Buffer,
        options: Options,
        groups: &[Group],
        ready: bool,
    ) -> Option<Edit> {
        if !self.open {
            return None;
        }
        if ready && self.revision != Some(buffer.revision) {
            self.revision = Some(buffer.revision);
            self.source = SourceMap::parse(&buffer.rope.to_string());
            self.options = options;
            self.worlds =
                crate::assembly::world_placements(groups).expect("Validated attachment hierarchy");
            self.selected = None;
            self.error = None;
        }
        let mut open = self.open;
        let mut edit = None;
        egui::Window::new("Orientation & game export")
            .open(&mut open)
            .default_pos(egui::pos2(120.0,120.0))
            .default_size(egui::vec2(500.0,680.0))
            .min_width(440.0)
            .resizable(true)
            .vscroll(true)
            .show(context, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.page,Page::Model,"Model");
                    ui.selectable_value(&mut self.page,Page::Game,"Game export");
                    ui.selectable_value(&mut self.page,Page::Frames,"Attachments");
                });
                ui.separator();
                ui.small("Saved in Luau source · Apply creates one undoable edit");
                if !ready { ui.label("Run the current source to edit these settings."); }
                if ready && self.source.is_none() { ui.label("Cannot safely locate settings in this source. Edit them in code."); }
                let driven = self.source.as_ref().is_some_and(|s|s.options_error.is_some() || options.declared && s.options.is_none());
                ui.add_enabled_ui(ready && self.source.is_some(), |ui| {
                    match self.page {
                        Page::Model => {
                            ui.add_enabled_ui(!driven, |ui| {
                                crate::interface::section(ui,"Model orientation");
                                angles(ui,"Assembly rotation",&mut self.options.rotation,false);
                                ui.small("Rotates the assembly about its origin. The camera and print placements stay unchanged.");
                                if ui.button("Reset rotation").clicked() { self.options.rotation=[0.0;3]; }
                                if let Ok(placement)=self.options.placement() { axes(ui,"Assembly axes · CAD Z up",placement.matrix()); }
                            });
                        }
                        Page::Game => {
                            ui.add_enabled_ui(!driven, |ui| {
                                crate::interface::section(ui,"Export convention");
                                let before=self.options.game.crowns_adrift;
                                egui::ComboBox::from_id_salt("game_preset")
                                    .selected_text(if before {"Crowns Adrift"} else {"glTF"})
                                    .show_ui(ui,|ui| {
                                        ui.selectable_value(&mut self.options.game.crowns_adrift,false,"glTF");
                                        ui.selectable_value(&mut self.options.game.crowns_adrift,true,"Crowns Adrift");
                                    });
                                if before!=self.options.game.crowns_adrift {
                                    self.options.game=Game {crowns_adrift:self.options.game.crowns_adrift,ground:self.options.game.crowns_adrift,..Game::default()};
                                }
                                for (label,axis) in [("Model forward",&mut self.options.game.forward),("Model up",&mut self.options.game.up)] {
                                    ui.horizontal(|ui| {
                                        ui.label(label);
                                        egui::ComboBox::from_id_salt(label).selected_text(axis.name()).show_ui(ui,|ui| {
                                            for value in Axis::ALL { ui.selectable_value(axis,value,value.name()); }
                                        });
                                    });
                                }
                                ui.horizontal(|ui| {
                                    ui.label("Uniform scale");
                                    ui.add(egui::DragValue::new(&mut self.options.game.scale).speed(0.05).range(0.0001..=10000.0));
                                });
                                ui.checkbox(&mut self.options.game.ground,"Align lowest point to the ground");
                                ui.small("GLB uses metres, +Y up and +Z forward. Scale is applied once at the export root.");
                                ui.small("Forward/up refer to assembled CAD axes, after model rotation. Disable grounding to preserve an equipment grip origin.");
                                if let Err(error)=self.options.game.validate() {
                                    ui.colored_label(crate::interface::ACCENT,error);
                                } else if let Ok(placement)=self.options.placement() {
                                    let basis=glam::Mat4::from_cols(glam::Vec4::X,glam::vec4(0.0,0.0,-1.0,0.0),glam::Vec4::Y,glam::Vec4::W);
                                    axes(ui,"CAD axes after export · game Y up",basis.transpose()*self.options.game.rotation()*basis*placement.matrix());
                                }
                            });
                        }
                        Page::Frames => {
                            crate::interface::section(ui,"Named attachment frames");
                            ui.small("Groups are reusable frames. Local forward is −Y; up is +Z.");
                            let previous=self.selected;
                            egui::ComboBox::from_id_salt("attachment_frame")
                                .selected_text(self.selected.and_then(|i|groups.get(i)).map_or("Choose a frame",|g|g.name.as_str()))
                                .show_ui(ui,|ui| {
                                    for (index,group) in groups.iter().enumerate() { ui.selectable_value(&mut self.selected,Some(index),&group.name); }
                                });
                            if previous!=self.selected {
                                if let Some(group)=self.selected.and_then(|i|groups.get(i)) { (self.position,self.rotation)=group.placement.components(); }
                            }
                            if let Some(index)=self.selected.filter(|i|*i<groups.len()) {
                                let group=&groups[index];
                                let editable=self.source.as_ref().and_then(|s|s.groups.get(&group.name)).is_some_and(Option::is_some);
                                ui.small(format!("Parent: {}",group.parent.map_or("Model origin",|i|groups[i].name.as_str())));
                                ui.add_enabled_ui(editable,|ui| {
                                    angles(ui,"Local position",&mut self.position,true);
                                    angles(ui,"Local rotation",&mut self.rotation,false);
                                    if ui.button("Apply frame transform").clicked() {
                                        match self.source.as_ref().expect("Editable source").pose_edit(buffer.revision,&group.name,self.position,self.rotation) {
                                            Ok(value)=>edit=Some(value),Err(error)=>self.error=Some(error),
                                        }
                                    }
                                });
                                if !editable { ui.small("This frame is driven by code or a module; edit its source expressions."); }
                                if let (Ok(orientation),Ok(local))=(options.placement(),Placement::from_components(self.position,self.rotation)) {
                                    let parent=group.parent.map_or(glam::Mat4::IDENTITY,|parent|self.worlds[parent].matrix());
                                    axes(ui,"Frame axes in the assembly",orientation.matrix()*parent*local.matrix());
                                }
                            }
                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut self.new_name).hint_text("New frame name").char_limit(80).desired_width(230.0));
                                if ui.button("Add frame").clicked() {
                                    match self.source.as_ref().expect("Editable source").new_frame(buffer.revision,&self.new_name,groups) {
                                        Ok(value)=>{edit=Some(value);self.new_name.clear();},Err(error)=>self.error=Some(error),
                                    }
                                }
                            });
                            ui.small("New frames start at the model origin. Their local handles can be used as parent= in code.");
                        }
                    }
                    if self.page!=Page::Frames {
                        ui.separator();
                        if self.page==Page::Model { if let Err(error)=self.options.game.validate() { ui.colored_label(crate::interface::ACCENT,error); } }
                        if driven { ui.small("Model options are driven by code or a module; edit their source expressions."); }
                        if ui.add_enabled(!driven && self.options.game.validate().is_ok(),egui::Button::new("Apply model & export settings")).clicked() {
                            match self.source.as_ref().expect("Editable source").options_edit(buffer.revision,self.options) {
                                Ok(value)=>edit=Some(value),Err(error)=>self.error=Some(error),
                            }
                        }
                    }
                    if let Some(error)=&self.error { ui.colored_label(crate::interface::ACCENT,error); }
                });
            });
        self.open = open;
        if !open {
            self.revision = None;
        }
        edit
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn apply(source: &str, edit: Edit) -> String {
        let mut buffer = Buffer::from(source);
        assert!(buffer.replace(edit.range, &edit.text, 0));
        buffer.to_string()
    }

    #[test]
    fn options_edits_preserve_directives_unicode_and_undo_and_never_evaluate_expressions() {
        let source = "--!strict\n-- Café\nlocal note='cad.model_options({})'\ncad.show('box',cad.box({width=1,depth=2,height=3}))\n";
        let map = SourceMap::parse(source).unwrap();
        let options = Options {
            rotation: [-30.0, 0.0, 90.0],
            game: Game {
                crowns_adrift: true,
                ground: true,
                scale: 25.0,
                ..Game::default()
            },
            ..Options::default()
        };
        let edit = map.options_edit(1, options).unwrap();
        let mut buffer = Buffer::from(source);
        assert!(buffer.replace(edit.range, &edit.text, 0));
        let text = buffer.to_string();
        assert!(text.starts_with("--!strict\n-- Café\ncad.model_options("));
        let compiled = crate::model::compile(&text).unwrap();
        assert_eq!(compiled.options.rotation, options.rotation);
        assert_eq!(compiled.options.game.scale, 25.0);
        assert!(buffer.undo(false).is_some());
        assert_eq!(buffer.to_string(), source);
        let map = SourceMap::parse(&text).unwrap();
        let edited = apply(&text, map.options_edit(2, Options::default()).unwrap());
        assert_eq!(edited.matches("cad.model_options(").count(), 2);
        assert!(crate::model::compile(&edited).is_ok());
        for source in [
            "cad.model_options({rotation={z=heading}})",
            "cad.model_options(options)",
            "if true then cad.model_options({}) end",
            "cad.model_options({game={scale=(function() error('never execute') end)()}})",
            "cad.model_options({game={forward=`{'y-'}`}})",
        ] {
            let map = SourceMap::parse(source).unwrap();
            assert!(map.options_edit(1, Options::default()).is_err(), "{source}");
        }
        assert!(SourceMap::parse("cad.model_options({").is_none());
    }

    #[test]
    fn frame_edits_preserve_parents_and_add_usable_handles_before_return() {
        for declaration in [
            "local grip=cad.group('Grip')",
            "local grip=cad.group('Grip',{})",
            "local grip=cad.group('Grip',{parent=root})",
            "local grip=cad.group('Grip',{parent=root,assembly={rotation={z=-20}}})",
        ] {
            let source = format!(
                "local root=cad.group('Root');{declaration};cad.show('box',cad.box({{width=1,depth=2,height=3}}),{{parent=grip}})"
            );
            let map = SourceMap::parse(&source).unwrap();
            let text = apply(
                &source,
                map.pose_edit(1, "Grip", [2.0, 3.0, 4.0], [0.0, 90.0, 0.0])
                    .unwrap(),
            );
            let model = crate::model::compile(&text).unwrap();
            assert_eq!(
                model.groups[1].placement.translation(),
                glam::vec3(2.0, 3.0, 4.0)
            );
            assert_eq!(
                model.groups[1].parent,
                if declaration.contains("parent=root") {
                    Some(0)
                } else {
                    None
                }
            );
        }
        let source = "--!strict\nlocal _attachment_frame_1=1\ncad.show('box',cad.box({width=1,depth=2,height=3}))\nreturn\n";
        let map = SourceMap::parse(source).unwrap();
        let text = apply(source, map.new_frame(1, "Grip \"A\"", &[]).unwrap());
        assert!(text.contains("local _attachment_frame_2 = cad.group("));
        let model = crate::model::compile(&text).unwrap();
        assert_eq!(model.groups[0].name, "Grip \"A\"");
        assert!(map.new_frame(1, "", &[]).is_err());
        assert!(map.new_frame(1, "bad\n", &[]).is_err());
        assert!(map.new_frame(1, "Grip \"A\"", &model.groups).is_err());
        let map =
            SourceMap::parse("cad.group('Driven',{assembly={rotation={z=heading}}})").unwrap();
        assert!(map.pose_edit(1, "Driven", [0.0; 3], [0.0; 3]).is_err());
        let map =
            SourceMap::parse("cad.group('Duplicate',{assembly={},assembly={position={x=1}}})")
                .unwrap();
        assert!(map.pose_edit(1, "Duplicate", [0.0; 3], [0.0; 3]).is_err());
        let map = SourceMap::parse("cad.group(`Grip {1}`,{})").unwrap();
        assert!(map.groups.is_empty());
    }
}
