use crate::{interface, model::Part};
use eframe::egui;
use std::collections::BTreeSet;

pub enum Action {
    Select(usize),
    Frame(usize),
    Source(usize),
}

#[derive(Default)]
pub struct State {
    hidden: BTreeSet<String>,
    isolated: Option<String>,
}

impl State {
    pub fn sync(&mut self, parts: &[Part]) {
        assert!(parts.len() <= 32);
        self.hidden
            .retain(|name| parts.iter().any(|part| part.name == *name));
        if self
            .isolated
            .as_ref()
            .is_some_and(|name| !parts.iter().any(|part| part.name == *name))
        {
            self.isolated = None;
        }
    }

    pub fn visible_parts(&self, parts: &[Part]) -> u32 {
        assert!(parts.len() <= 32);
        parts.iter().enumerate().fold(0, |mask, (index, part)| {
            let visible = self.isolated.as_ref().map_or_else(
                || !self.hidden.contains(&part.name),
                |name| part.name == *name,
            );
            mask | (u32::from(visible) << index)
        })
    }

    pub fn reveal(&mut self, name: &str) {
        if self.isolated.as_deref() == Some(name) {
            return;
        }
        self.isolated = None;
        self.hidden.remove(name);
    }

    fn set_visible(&mut self, parts: &[Part], index: usize, visible: bool) {
        assert!(index < parts.len());
        let mask = self.visible_parts(parts);
        if self.isolated.take().is_some() {
            self.hidden.clear();
            for (index, part) in parts.iter().enumerate() {
                if mask & (1 << index) == 0 {
                    self.hidden.insert(part.name.clone());
                }
            }
        }
        if visible {
            self.hidden.remove(&parts[index].name);
        } else {
            self.hidden.insert(parts[index].name.clone());
        }
    }

    fn actions(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        part: &Part,
        source_ready: bool,
        action: &mut Option<Action>,
    ) {
        let isolated = self.isolated.as_ref() == Some(&part.name);
        if ui
            .button(if isolated {
                "Exit isolation"
            } else {
                "Isolate part"
            })
            .clicked()
        {
            self.isolated = if isolated {
                None
            } else {
                Some(part.name.clone())
            };
            ui.close();
        }
        if ui.button("Show and frame part").clicked() {
            self.reveal(&part.name);
            *action = Some(Action::Frame(index));
            ui.close();
        }
        if ui
            .add_enabled(source_ready, egui::Button::new("Reveal source"))
            .on_disabled_hover_text(
                "Run the current source before revealing its output declaration",
            )
            .clicked()
        {
            *action = Some(Action::Source(index));
            ui.close();
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        parts: &[Part],
        selected: Option<usize>,
        source_ready: bool,
    ) -> Option<Action> {
        assert!(parts.len() <= 32);
        let mut action = None;
        ui.horizontal(|ui| {
            ui.small(
                egui::RichText::new(format!(
                    "{} / {} visible",
                    self.visible_parts(parts).count_ones(),
                    parts.len()
                ))
                .color(interface::MUTED),
            );
            if self.isolated.is_some() {
                if ui.button("Exit isolation").clicked() {
                    self.isolated = None;
                }
            } else if ui
                .add_enabled(!self.hidden.is_empty(), egui::Button::new("Show all"))
                .clicked()
            {
                self.hidden.clear();
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("assembly_parts")
            .max_height(160.0)
            .show(ui, |ui| {
                for (index, part) in parts.iter().enumerate() {
                    let mut visible = self.visible_parts(parts) & (1 << index) != 0;
                    let row = ui.horizontal(|ui| {
                        if ui
                            .checkbox(&mut visible, "")
                            .on_hover_text(format!("Show or hide {}", part.name))
                            .changed()
                        {
                            self.set_visible(parts, index, visible);
                        }
                        let (swatch, _) =
                            ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                        let color: egui::Color32 =
                            egui::Rgba::from_rgb(part.color[0], part.color[1], part.color[2])
                                .into();
                        ui.painter().rect_filled(
                            swatch,
                            2.0,
                            if visible { color } else { interface::BORDER },
                        );
                        let response = ui.add_sized(
                            egui::vec2((ui.available_width() - 40.0).max(40.0), 28.0),
                            egui::Button::new("")
                                .left_text(egui::RichText::new(&part.name).color(if visible {
                                    interface::TEXT
                                } else {
                                    interface::MUTED
                                }))
                                .selected(selected == Some(index))
                                .frame_when_inactive(selected == Some(index))
                                .truncate(),
                        );
                        if response.clicked() && visible {
                            action = Some(Action::Select(index));
                        }
                        response.context_menu(|ui| {
                            self.actions(ui, index, part, source_ready, &mut action)
                        });
                        ui.menu_button("…", |ui| {
                            self.actions(ui, index, part, source_ready, &mut action)
                        })
                        .response
                        .on_hover_text("Part actions · isolate, frame, reveal source");
                    });
                    row.response.on_hover_text(if visible {
                        "Select part · right-click for actions"
                    } else {
                        "Hidden · enable visibility or choose Show and frame part"
                    });
                }
            });
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(names: &[&str]) -> Vec<Part> {
        let template =
            crate::model::compile("cad.show('part',cad.box({width=10,depth=10,height=10}))")
                .expect("Valid part")
                .parts
                .remove(0);
        names
            .iter()
            .map(|name| {
                let mut part = template.clone();
                part.name = (*name).into();
                part
            })
            .collect()
    }

    #[test]
    fn visibility_tracks_names_through_reorder_addition_and_removal() {
        let mut state = State::default();
        let before = parts(&["front", "rear", "lid"]);
        state.set_visible(&before, 1, false);
        let after = parts(&["lid", "rear", "new"]);
        state.sync(&after);
        assert_eq!(state.visible_parts(&after), 0b101);
        state.sync(&parts(&["lid", "new"]));
        assert!(state.hidden.is_empty());
        let renamed = parts(&["renamed", "new"]);
        state.sync(&renamed);
        assert_eq!(state.visible_parts(&renamed), 0b11);
    }

    #[test]
    fn isolation_restores_hidden_parts_and_visibility_edits_commit_the_isolated_view() {
        let parts = parts(&["front", "rear", "lid"]);
        let mut state = State::default();
        state.set_visible(&parts, 1, false);
        state.isolated = Some("rear".into());
        state.reveal("rear");
        assert_eq!(state.isolated.as_deref(), Some("rear"));
        assert_eq!(state.visible_parts(&parts), 0b010);
        assert!(state.hidden.contains("rear"));
        state.isolated = Some("front".into());
        assert_eq!(state.visible_parts(&parts), 1);
        state.isolated = None;
        assert_eq!(state.visible_parts(&parts), 0b101);
        state.isolated = Some("front".into());
        state.set_visible(&parts, 1, true);
        assert!(state.isolated.is_none());
        assert_eq!(state.visible_parts(&parts), 0b011);
        state.reveal("lid");
        assert_eq!(state.visible_parts(&parts), 0b111);
        state.isolated = Some("gone".into());
        state.sync(&parts);
        assert!(state.isolated.is_none());
    }

    #[test]
    fn visibility_supports_the_last_part_and_allocates_nothing_on_repaint() {
        let mut parts = parts(&["part"]);
        for index in 1..32 {
            let mut part = parts[0].clone();
            part.name = format!("part {index}");
            parts.push(part);
        }
        let mut state = State::default();
        assert_eq!(state.visible_parts(&parts), u32::MAX);
        state.set_visible(&parts, 31, false);
        assert_eq!(state.visible_parts(&parts), 0x7fff_ffff);
        state.isolated = Some(parts[31].name.clone());
        let (mask, counts) = crate::allocations::measure(|| state.visible_parts(&parts));
        assert_eq!(mask, 0x8000_0000);
        assert_eq!(counts, crate::allocations::Counts::default());
        for index in 0..32 {
            state.set_visible(&parts, index, false);
        }
        assert_eq!(state.visible_parts(&parts), 0);
    }
}
