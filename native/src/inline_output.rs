use crate::text_buffer::Buffer;
use eframe::egui::{self, Color32, FontId};
use glam::Vec3;
use std::{collections::BTreeSet, sync::Arc};

pub enum Content {
    Part {
        image: Option<Arc<egui::ColorImage>>,
        dimensions: Vec3,
        volume_mm3: f64,
    },
    Measurement {
        distance_mm: f32,
        checked: bool,
    },
}

pub struct Entry {
    pub line: usize,
    pub name: String,
    pub content: Content,
}

pub struct Focus {
    pub revision: u64,
    pub name: String,
}

#[derive(Clone)]
pub struct Placement {
    pub index: usize,
    pub line: usize,
    pub top: f32,
    pub height: f32,
    end: f32,
}

#[derive(Clone)]
pub struct Layout {
    pub cards: Vec<Placement>,
    row_height: f32,
    line_count: usize,
    hidden: bool,
}

struct Textures {
    entries: Arc<Vec<Entry>>,
    handles: Vec<Option<egui::TextureHandle>>,
}

impl Layout {
    pub fn line_top(&self, line: usize) -> f32 {
        assert!(line <= self.line_count);
        let count = self.cards.partition_point(|card| card.line < line);
        line as f32 * self.row_height
            + count
                .checked_sub(1)
                .map_or(0.0, |index| self.cards[index].end)
    }

    pub fn height(&self) -> f32 {
        self.line_top(self.line_count)
    }

    pub fn line_index(&self, y: f32) -> usize {
        let mut first = 0;
        let mut last = self.line_count - 1;
        while first < last {
            let middle = first + (last - first).div_ceil(2);
            if self.line_top(middle) <= y {
                first = middle;
            } else {
                last = middle - 1;
            }
        }
        first
    }

    pub fn text_line(&self, y: f32) -> Option<usize> {
        let line = self.line_index(y);
        (y >= self.line_top(line) && y < self.line_top(line) + self.row_height).then_some(line)
    }
}

#[derive(Default)]
pub struct State {
    pub hidden: bool,
    entries: Arc<Vec<Entry>>,
    anchors: Vec<usize>,
    textures: Vec<Option<egui::TextureHandle>>,
    textures_previous: Option<Textures>,
    keys: Vec<String>,
    layout: Option<Arc<Layout>>,
    layout_dirty: bool,
    collapsed: BTreeSet<String>,
    result_revision: u64,
    tracked_revision: u64,
    resolved: bool,
    focus: Option<Focus>,
}

impl State {
    pub fn set(&mut self, revision: u64, entries: Arc<Vec<Entry>>) {
        assert!(revision > 0);
        assert!(entries.len() <= 96);
        assert!(entries.windows(2).all(|pair| pair[0].line <= pair[1].line));
        self.keys = entries.iter().map(Self::key).collect();
        self.collapsed.retain(|key| self.keys.contains(key));
        self.anchors.clear();
        self.anchors.extend(entries.iter().map(|entry| entry.line));
        if self.result_revision != revision {
            self.textures_previous = None;
        }
        if self.result_revision != revision || !Arc::ptr_eq(&entries, &self.entries) {
            let handles = self
                .textures_previous
                .take()
                .filter(|previous| Arc::ptr_eq(&previous.entries, &entries))
                .map(|previous| previous.handles)
                .unwrap_or_else(|| {
                    entries
                        .iter()
                        .map(|entry| {
                            let Content::Part {
                                image: Some(image), ..
                            } = &entry.content
                            else {
                                return None;
                            };
                            self.entries.iter().zip(&self.textures).find_map(
                                |(previous, texture)| {
                                    let Content::Part {
                                        image: Some(previous),
                                        ..
                                    } = &previous.content
                                    else {
                                        return None;
                                    };
                                    Arc::ptr_eq(image, previous)
                                        .then(|| texture.clone())
                                        .flatten()
                                },
                            )
                        })
                        .collect()
                });
            let previous = Textures {
                entries: Arc::clone(&self.entries),
                handles: std::mem::replace(&mut self.textures, handles),
            };
            self.textures_previous = (self.result_revision == revision).then_some(previous);
        }
        self.entries = entries;
        self.layout_dirty = true;
        self.result_revision = revision;
        self.tracked_revision = revision;
        self.resolved = false;
        self.focus = None;
    }

    pub fn focus_requested(&self) -> bool {
        self.focus.is_some()
    }

    pub fn take_focus(&mut self) -> Option<Focus> {
        self.focus.take()
    }

    pub fn resolve(&mut self, call_end: impl Fn(usize) -> Option<usize>) {
        if self.resolved || self.result_revision != self.tracked_revision {
            return;
        }
        let mut anchors = [0; 96];
        for (index, entry) in self.entries.iter().enumerate() {
            let Some(line) = call_end(entry.line) else {
                return;
            };
            anchors[index] = line;
        }
        self.anchors.clear();
        self.anchors
            .extend_from_slice(&anchors[..self.entries.len()]);
        self.resolved = true;
        self.layout_dirty = true;
    }

    pub fn sync(&mut self, source: &Buffer) {
        if self.tracked_revision == source.revision {
            return;
        }
        self.focus = None;
        if let Some(change) = source
            .change
            .filter(|change| change.previous_revision == self.tracked_revision)
        {
            for line in &mut self.anchors {
                if *line >= change.old_end_point.0 {
                    *line = (*line - change.old_end_point.0 + change.new_end_point.0)
                        .min(source.rope.len_lines() - 1);
                } else if *line >= change.start_point.0 {
                    *line = change.new_end_point.0.min(source.rope.len_lines() - 1);
                }
            }
        } else {
            self.anchors.clear();
        }
        self.tracked_revision = source.revision;
        self.layout_dirty = true;
    }

    fn key(entry: &Entry) -> String {
        format!(
            "{}:{}",
            if matches!(entry.content, Content::Part { .. }) {
                "part"
            } else {
                "measure"
            },
            entry.name
        )
    }

    pub fn layout(&mut self, line_count: usize, row_height: f32) -> Arc<Layout> {
        assert!(line_count > 0);
        assert!(row_height > 0.0);
        if let Some(layout) = &self.layout {
            if !self.layout_dirty
                && layout.line_count == line_count
                && layout.row_height == row_height
                && layout.hidden == self.hidden
            {
                return Arc::clone(layout);
            }
        }
        let cached = self.layout.get_or_insert_with(|| {
            Arc::new(Layout {
                cards: Vec::with_capacity(self.anchors.len()),
                row_height,
                line_count,
                hidden: self.hidden,
            })
        });
        let layout = Arc::make_mut(cached);
        layout.cards.clear();
        layout.line_count = line_count;
        layout.row_height = row_height;
        layout.hidden = self.hidden;
        self.layout_dirty = false;
        if self.hidden || !self.resolved {
            return Arc::clone(cached);
        }
        for (index, line) in self.anchors.iter().copied().enumerate() {
            let line = line.min(line_count - 1);
            let entry = &self.entries[index];
            let height = if self.collapsed.contains(&self.keys[index]) {
                30.0
            } else if matches!(entry.content, Content::Part { .. }) {
                136.0
            } else {
                66.0
            };
            layout.cards.push(Placement {
                index,
                line,
                top: 0.0,
                height,
                end: 0.0,
            });
        }
        layout
            .cards
            .sort_unstable_by_key(|card| (card.line, card.index));
        let mut extra = 0.0;
        for card in &mut layout.cards {
            card.top = (card.line + 1) as f32 * row_height + extra;
            extra += card.height;
            card.end = extra;
        }
        Arc::clone(cached)
    }

    pub fn paint(&mut self, ui: &mut egui::Ui, placement: &Placement, rect: egui::Rect) {
        let entry = &self.entries[placement.index];
        let key = &self.keys[placement.index];
        let collapsed = self.collapsed.contains(key);
        let stale = self.result_revision != self.tracked_revision;
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        painter.rect_filled(
            rect.shrink2(egui::vec2(0.0, 3.0)),
            5.0,
            crate::interface::SURFACE,
        );
        let header = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 28.0));
        let response = ui.interact(
            header,
            ui.scope_id().with(("inline_output", placement.index)),
            egui::Sense::click(),
        );
        if response.clicked() {
            if collapsed {
                self.collapsed.remove(key);
            } else {
                self.collapsed.insert(key.clone());
            }
            self.layout_dirty = true;
            ui.ctx().request_repaint();
        }
        painter.text(
            rect.min + egui::vec2(22.0, 7.0),
            egui::Align2::LEFT_TOP,
            if stale {
                format!("Previous result · {}", entry.name)
            } else {
                entry.name.clone()
            },
            FontId::proportional(13.0),
            if stale {
                crate::interface::WARNING
            } else {
                crate::interface::TEXT
            },
        );
        let center = rect.min + egui::vec2(11.0, 14.0);
        let points = if collapsed {
            vec![
                center + egui::vec2(-2.0, -4.0),
                center + egui::vec2(3.0, 0.0),
                center + egui::vec2(-2.0, 4.0),
            ]
        } else {
            vec![
                center + egui::vec2(-4.0, -2.0),
                center + egui::vec2(4.0, -2.0),
                center + egui::vec2(0.0, 3.0),
            ]
        };
        painter.add(egui::Shape::convex_polygon(
            points,
            crate::interface::MUTED,
            egui::Stroke::NONE,
        ));
        response.on_hover_text("Click to collapse or expand this result. Previous results remain marked until a successful run.");
        if collapsed {
            return;
        }
        match &entry.content {
            Content::Part {
                image,
                dimensions,
                volume_mm3,
            } => {
                let body = egui::Rect::from_min_max(rect.min + egui::vec2(0.0, 28.0), rect.max);
                let response = ui.interact(
                    body,
                    ui.scope_id().with(("inline_part", placement.index)),
                    if stale {
                        egui::Sense::hover()
                    } else {
                        egui::Sense::click()
                    },
                );
                if response.clicked() {
                    self.focus = Some(Focus {
                        revision: self.result_revision,
                        name: entry.name.clone(),
                    });
                }
                response
                    .on_hover_text(if stale {
                        "Run to update this part before selecting it"
                    } else {
                        "Select and frame this part in the viewport"
                    })
                    .on_hover_cursor(if stale {
                        egui::CursorIcon::Default
                    } else {
                        egui::CursorIcon::PointingHand
                    });
                let narrow = rect.width() < 340.0;
                let thumbnail = egui::Rect::from_min_size(
                    if narrow {
                        egui::pos2(rect.center().x - 48.0, rect.top() + 30.0)
                    } else {
                        rect.min + egui::vec2(8.0, 30.0)
                    },
                    if narrow {
                        egui::vec2(96.0, 60.0)
                    } else {
                        egui::vec2(144.0, 90.0)
                    },
                );
                if let Some(image) = image {
                    let texture = self.textures[placement.index].get_or_insert_with(|| {
                        ui.ctx().load_texture(
                            format!("inline-{}-{}", self.result_revision, placement.index),
                            image.as_ref().clone(),
                            egui::TextureOptions::LINEAR,
                        )
                    });
                    painter.image(
                        texture.id(),
                        thumbnail,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                } else {
                    painter.text(
                        thumbnail.center(),
                        egui::Align2::CENTER_CENTER,
                        "Part preview\nUse viewport",
                        FontId::proportional(12.0),
                        crate::interface::MUTED,
                    );
                }
                painter.text(
                    rect.min
                        + if narrow {
                            egui::vec2(8.0, 96.0)
                        } else {
                            egui::vec2(164.0, 38.0)
                        },
                    egui::Align2::LEFT_TOP,
                    if narrow {
                        format!(
                            "{:.1} × {:.1} × {:.1} mm\nVolume · {:.1} mm³",
                            dimensions.x, dimensions.y, dimensions.z, volume_mm3
                        )
                    } else {
                        format!(
                            "X × Y × Z · mm\n{:.1} × {:.1} × {:.1}\n\nVolume · {:.1} mm³",
                            dimensions.x, dimensions.y, dimensions.z, volume_mm3
                        )
                    },
                    FontId::proportional(13.0),
                    crate::interface::TEXT,
                );
            }
            Content::Measurement {
                distance_mm,
                checked,
            } => {
                painter.text(
                    rect.min + egui::vec2(12.0, 34.0),
                    egui::Align2::LEFT_TOP,
                    format!(
                        "{distance_mm:.3} mm · {}",
                        if *checked {
                            "check passed"
                        } else {
                            "reference"
                        }
                    ),
                    FontId::proportional(14.0),
                    crate::interface::ACCENT,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_thumbnail_images_keep_their_texture_across_source_revisions() {
        let image = Arc::new(egui::ColorImage::filled([2, 2], Color32::WHITE));
        let entry = |image| {
            Arc::new(vec![Entry {
                line: 1,
                name: "part".into(),
                content: Content::Part {
                    image: Some(image),
                    dimensions: Vec3::ONE,
                    volume_mm3: 1.0,
                },
            }])
        };
        let context = egui::Context::default();
        let mut state = State::default();
        state.set(1, entry(Arc::clone(&image)));
        let texture = context.load_texture(
            "thumbnail",
            image.as_ref().clone(),
            egui::TextureOptions::LINEAR,
        );
        let id = texture.id();
        state.textures[0] = Some(texture);
        state.set(2, entry(Arc::clone(&image)));
        assert_eq!(
            state.textures[0].as_ref().expect("Reused thumbnail").id(),
            id
        );
        state.set(3, entry(Arc::new(image.as_ref().clone())));
        assert!(state.textures[0].is_none());
    }

    fn entries() -> Arc<Vec<Entry>> {
        Arc::new(vec![
            Entry {
                line: 1,
                name: "part".into(),
                content: Content::Part {
                    image: None,
                    dimensions: Vec3::ONE,
                    volume_mm3: 1.0,
                },
            },
            Entry {
                line: 1,
                name: "copy".into(),
                content: Content::Part {
                    image: None,
                    dimensions: Vec3::ONE,
                    volume_mm3: 1.0,
                },
            },
            Entry {
                line: 20000,
                name: "height".into(),
                content: Content::Measurement {
                    distance_mm: 1.0,
                    checked: true,
                },
            },
        ])
    }

    #[test]
    fn sparse_layout_maps_text_and_output_gaps_without_indexing_every_line() {
        let mut state = State::default();
        state.set(1, entries());
        state.resolve(Some);
        let layout = state.layout(100001, 21.0);
        assert_eq!(layout.cards.len(), 3);
        assert_eq!(layout.line_top(2), 42.0 + 272.0);
        assert_eq!(layout.height(), 100001.0 * 21.0 + 272.0 + 66.0);
        for line in [0, 1, 2, 19999, 20000, 20001, 100000] {
            assert_eq!(layout.text_line(layout.line_top(line) + 10.0), Some(line));
        }
        for card in &layout.cards {
            assert_eq!(layout.text_line(card.top + card.height / 2.0), None);
            assert_eq!(layout.line_index(card.top + card.height / 2.0), card.line);
        }
        state.collapsed.insert("part:part".into());
        state.layout_dirty = true;
        assert_eq!(state.layout(100001, 21.0).cards[0].height, 30.0);
        state.hidden = true;
        assert_eq!(state.layout(100001, 21.0).height(), 100001.0 * 21.0);
    }

    #[test]
    fn anchors_follow_edits_mark_previous_results_and_reject_unknown_edit_chains() {
        let mut source = Buffer::from("-- λ\ncad.show('part', part)\nreturn\n");
        let mut state = State::default();
        let entries = Arc::new(vec![Entry {
            line: 1,
            name: "part".into(),
            content: Content::Measurement {
                distance_mm: 1.0,
                checked: false,
            },
        }]);
        state.set(source.revision, Arc::clone(&entries));
        state.resolve(Some);
        source.replace(0..0, "-- header\n", 0);
        state.sync(&source);
        assert_eq!(state.anchors, [2]);
        assert_ne!(state.result_revision, state.tracked_revision);
        source.replace(0..0, "-- first\n", 0);
        source.replace(0..0, "-- second\n", 0);
        state.sync(&source);
        assert!(state.anchors.is_empty());
        state.set(source.revision, entries);
        state.resolve(Some);
        assert_eq!(state.anchors, [1]);
        assert_eq!(state.result_revision, state.tracked_revision);
    }

    #[test]
    fn layouts_and_alternate_thumbnails_reuse_storage_until_inputs_change() {
        let mut state = State::default();
        let assembly = entries();
        let print = entries();
        state.set(1, Arc::clone(&assembly));
        state.resolve(Some);
        let first = state.layout(100001, 21.0);
        let again = state.layout(100001, 21.0);
        assert!(Arc::ptr_eq(&first, &again));
        let cards = first.cards.as_ptr();
        drop(first);
        drop(again);
        state.hidden = true;
        assert!(state.layout(100001, 21.0).cards.is_empty());
        state.hidden = false;
        assert_eq!(state.layout(100001, 21.0).cards.as_ptr(), cards);
        assert_eq!(state.layout(100001, 42.0).line_top(1), 42.0);
        let context = egui::Context::default();
        let texture = context.load_texture(
            "cached-thumbnail",
            egui::ColorImage::filled([2, 2], Color32::WHITE),
            egui::TextureOptions::LINEAR,
        );
        let id = texture.id();
        state.textures[0] = Some(texture);
        state.set(1, Arc::clone(&print));
        assert!(state.textures[0].is_none());
        state.set(1, Arc::clone(&assembly));
        assert_eq!(
            state.textures[0]
                .as_ref()
                .expect("Assembly texture retained")
                .id(),
            id
        );
        state.set(2, print);
        assert!(state.textures[0].is_none());
        assert!(state.textures_previous.is_none());
    }
}
