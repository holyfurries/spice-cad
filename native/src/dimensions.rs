use crate::{mesh::Mesh, model::Solid};
use eframe::egui;
use glam::{Mat4, Vec3};
use mlua::{AnyUserData, Lua, Table, UserData};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

const COUNT_MAX: usize = 64;
const TOLERANCE_MM: f32 = 0.001;

#[derive(Clone, Copy)]
struct Face(usize);
impl UserData for Face {}

struct Selector {
    solid: usize,
    name: String,
    axis: usize,
    positive: bool,
    at: Option<f32>,
}
struct Requirement {
    name: String,
    first: usize,
    second: usize,
    minimum: Option<f64>,
    maximum: Option<f64>,
}
#[derive(Default)]
pub struct Definitions {
    faces: Vec<Selector>,
    measurements: Vec<Requirement>,
}
pub struct NamedFace {
    pub name: String,
    pub id: u32,
    pub point: Vec3,
}
pub struct Measurement {
    pub name: String,
    axis: usize,
    faces: [u32; 2],
    pub points: [Vec3; 2],
    pub distance_mm: f32,
    pub checked: bool,
}
#[derive(Default)]
pub struct Annotations {
    pub faces: Vec<NamedFace>,
    pub measurements: Vec<Measurement>,
}
struct Plane {
    axis: Option<(usize, bool)>,
    minimum: Vec3,
    maximum: Vec3,
}
struct DisplayDimension<'a> {
    direction: &'static str,
    points: [Vec3; 2],
    name: &'a str,
    distance_mm: f32,
    checked: bool,
    selected: bool,
    offset_px: f32,
}

pub fn register(lua: &Lua, cad: &Table) -> mlua::Result<Rc<RefCell<Definitions>>> {
    let definitions = Rc::new(RefCell::new(Definitions::default()));
    let shared = Rc::clone(&definitions);
    cad.set(
        "face",
        lua.create_function(move |_, (solid, options): (AnyUserData, Table)| {
            crate::model::options_fields(&options, &["name", "normal", "at"])?;
            let name: String = options.get("name")?;
            valid_name(&name)?;
            let normal: String = options.get("normal")?;
            let (axis, positive) = match normal.as_str() {
                "x+" => (0, true),
                "x-" => (0, false),
                "y+" => (1, true),
                "y-" => (1, false),
                "z+" => (2, true),
                "z-" => (2, false),
                _ => {
                    return Err(mlua::Error::RuntimeError(
                        "Face normal must be x+, x-, y+, y-, z+, or z-".into(),
                    ));
                }
            };
            let at: Option<f64> = options.get("at")?;
            if at.is_some_and(|value| !value.is_finite() || value.abs() > 2000.0) {
                return Err(mlua::Error::RuntimeError(
                    "Face position must be finite and within ±2000 mm".into(),
                ));
            }
            let mut definitions = shared.borrow_mut();
            if definitions.faces.len() >= COUNT_MAX
                || definitions.faces.iter().any(|face| face.name == name)
            {
                return Err(mlua::Error::RuntimeError(
                    "Face names must be unique; at most 64 are supported".into(),
                ));
            }
            let index = definitions.faces.len();
            definitions.faces.push(Selector {
                solid: solid.borrow::<Solid>()?.0,
                name,
                axis,
                positive,
                at: at.map(|value| value as f32),
            });
            Ok(Face(index))
        })?,
    )?;
    let shared = Rc::clone(&definitions);
    cad.set(
        "measure",
        lua.create_function(move |_, (name, options): (String, Table)| {
            valid_name(&name)?;
            crate::model::options_fields(&options, &["from", "to", "min", "max"])?;
            let first = options.get::<AnyUserData>("from")?.borrow::<Face>()?.0;
            let second = options.get::<AnyUserData>("to")?.borrow::<Face>()?.0;
            let minimum: Option<f64> = options.get("min")?;
            let maximum: Option<f64> = options.get("max")?;
            if minimum
                .into_iter()
                .chain(maximum)
                .any(|value| !value.is_finite() || !(0.0..=4000.0).contains(&value))
            {
                return Err(mlua::Error::RuntimeError(
                    "Measurement limits must be finite and between 0 and 4000 mm".into(),
                ));
            }
            if minimum
                .zip(maximum)
                .is_some_and(|(minimum, maximum)| minimum > maximum)
            {
                return Err(mlua::Error::RuntimeError(
                    "Measurement min must not exceed max".into(),
                ));
            }
            let mut definitions = shared.borrow_mut();
            assert!(first < definitions.faces.len());
            assert!(second < definitions.faces.len());
            if definitions.measurements.len() >= COUNT_MAX
                || definitions
                    .measurements
                    .iter()
                    .any(|measurement| measurement.name == name)
            {
                return Err(mlua::Error::RuntimeError(
                    "Measurement names must be unique; at most 64 are supported".into(),
                ));
            }
            definitions.measurements.push(Requirement {
                name,
                first,
                second,
                minimum,
                maximum,
            });
            Ok(())
        })?,
    )?;
    Ok(definitions)
}

fn valid_name(name: &str) -> mlua::Result<()> {
    if name.is_empty() || name.len() > 80 || name.chars().any(char::is_control) {
        return Err(mlua::Error::RuntimeError(
            "Names must contain 1–80 bytes without control characters".into(),
        ));
    }
    Ok(())
}

fn planes(mesh: &Mesh) -> BTreeMap<u32, Plane> {
    assert!(mesh.vertices.len() <= 300000);
    assert!(mesh.vertices.len().is_multiple_of(3));
    let mut planes = BTreeMap::<u32, Plane>::new();
    for triangle in mesh.vertices.as_chunks::<3>().0 {
        let points = triangle.map(|vertex| Vec3::from_array(vertex.position));
        let normal = (points[1] - points[0])
            .cross(points[2] - points[0])
            .normalize_or_zero();
        let axis = (0..3)
            .find(|axis| normal[*axis].abs() > 0.999999)
            .map(|axis| (axis, normal[axis] > 0.0));
        let plane = planes.entry(triangle[0].face_id).or_insert(Plane {
            axis,
            minimum: points[0],
            maximum: points[0],
        });
        if plane.axis != axis {
            plane.axis = None;
        }
        for point in points {
            plane.minimum = plane.minimum.min(point);
            plane.maximum = plane.maximum.max(point);
        }
        if let Some((axis, _)) = plane.axis {
            if plane.maximum[axis] - plane.minimum[axis] > TOLERANCE_MM {
                plane.axis = None;
            }
        }
    }
    planes
}

impl Definitions {
    pub fn resolve(&self, mesh: &Mesh, output: usize) -> Result<Annotations, String> {
        assert!(self.faces.len() <= COUNT_MAX);
        assert!(self.measurements.len() <= COUNT_MAX);
        let mut annotations = Annotations::default();
        if self.faces.is_empty() {
            return Ok(annotations);
        }
        let planes = planes(mesh);
        for selector in &self.faces {
            if selector.solid != output {
                return Err(format!(
                    "Face '{}': select faces on the final solid passed to cad.show",
                    selector.name
                ));
            }
            let mut matches = planes.iter().filter(|(_, plane)| {
                plane.axis == Some((selector.axis, selector.positive))
                    && selector
                        .at
                        .is_none_or(|at| (plane.minimum[selector.axis] - at).abs() <= TOLERANCE_MM)
            });
            let (id, plane) = matches.next().ok_or_else(|| {
                format!(
                    "Face '{}': no matching axis-aligned planar face",
                    selector.name
                )
            })?;
            if matches.next().is_some() {
                return Err(format!(
                    "Face '{}': ambiguous selector; add an at coordinate or refine the geometry",
                    selector.name
                ));
            }
            annotations.faces.push(NamedFace {
                name: selector.name.clone(),
                id: *id,
                point: (plane.minimum + plane.maximum) * 0.5,
            });
        }
        for requirement in &self.measurements {
            let first = &self.faces[requirement.first];
            let second = &self.faces[requirement.second];
            if first.axis != second.axis {
                return Err(format!(
                    "Measurement '{}': faces must be parallel",
                    requirement.name
                ));
            }
            let mut points = [
                annotations.faces[requirement.first].point,
                annotations.faces[requirement.second].point,
            ];
            let distance_mm = (points[1][first.axis] - points[0][first.axis]).abs();
            if requirement
                .minimum
                .is_some_and(|minimum| f64::from(distance_mm + TOLERANCE_MM) < minimum)
                || requirement
                    .maximum
                    .is_some_and(|maximum| f64::from(distance_mm - TOLERANCE_MM) > maximum)
            {
                return Err(format!(
                    "Constraint '{}': measured {:.3} mm, required {}–{} mm",
                    requirement.name,
                    distance_mm,
                    requirement
                        .minimum
                        .map_or("0".into(), |value| value.to_string()),
                    requirement
                        .maximum
                        .map_or("unbounded".into(), |value| value.to_string())
                ));
            }
            for axis in [0, 1, 2] {
                if axis != first.axis {
                    let center = (points[0][axis] + points[1][axis]) * 0.5;
                    points[0][axis] = center;
                    points[1][axis] = center;
                }
            }
            annotations.measurements.push(Measurement {
                name: requirement.name.clone(),
                axis: first.axis,
                faces: [
                    annotations.faces[requirement.first].id,
                    annotations.faces[requirement.second].id,
                ],
                points,
                distance_mm,
                checked: requirement.minimum.is_some() || requirement.maximum.is_some(),
            });
        }
        Ok(annotations)
    }
}

pub fn selector(mesh: &Mesh, id: u32) -> Option<String> {
    let planes = planes(mesh);
    let plane = planes.get(&id)?;
    let (axis, positive) = plane.axis?;
    Some(format!(
        "cad.face(part, {{ name = \"face_name\", normal = \"{}{}\", at = {:.6} }})",
        ["x", "y", "z"][axis],
        if positive { "+" } else { "-" },
        plane.minimum[axis]
    ))
}

pub fn draw(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    matrix: Mat4,
    eye: Vec3,
    bounds: [Vec3; 2],
    annotations: &Annotations,
    selected: Option<&crate::inspection::Selection>,
) {
    let project = |point: Vec3| -> Option<egui::Pos2> {
        let point = matrix * point.extend(1.0);
        if point.w <= 0.1 || !(0.0..=point.w).contains(&point.z) {
            return None;
        }
        Some(
            rect.center()
                + egui::vec2(
                    point.x / point.w * rect.width() * 0.5,
                    -point.y / point.w * rect.height() * 0.5,
                ),
        )
    };
    if let Some(selection) = selected {
        let spans = selection.spans();
        let ids = selection.ids();
        let local = spans.iter().flatten().map(|span| DisplayDimension {
            direction: span.direction,
            points: span.points,
            name: span.direction,
            distance_mm: span.distance_mm,
            checked: false,
            selected: true,
            offset_px: 24.0,
        });
        let related = annotations
            .measurements
            .iter()
            .filter(|measurement| {
                matches!(selection, crate::inspection::Selection::Face(_))
                    && measurement.faces.iter().any(|id| ids.contains(id))
            })
            .map(|measurement| DisplayDimension {
                direction: ["X", "Y", "Z"][measurement.axis],
                points: measurement.points,
                name: &measurement.name,
                distance_mm: measurement.distance_mm,
                checked: measurement.checked,
                selected: true,
                offset_px: 48.0,
            });
        draw_dimensions(ui, rect, matrix, selection.center(), local.chain(related));
        if let Some(face) = annotations
            .faces
            .iter()
            .find(|face| face.id == selection.latest().id)
        {
            if let Some(position) = project(face.point) {
                ui.painter().with_clip_rect(rect).text(
                    position + egui::vec2(5.0, -5.0),
                    egui::Align2::LEFT_BOTTOM,
                    &face.name,
                    egui::FontId::proportional(12.0),
                    crate::interface::ACCENT,
                );
            }
        }
        return;
    }
    let [minimum, maximum] = bounds;
    assert!(minimum.is_finite());
    assert!(maximum.is_finite());
    let center = (minimum + maximum) * 0.5;
    let corner = Vec3::new(
        if eye.x >= center.x {
            maximum.x
        } else {
            minimum.x
        },
        if eye.y >= center.y {
            maximum.y
        } else {
            minimum.y
        },
        minimum.z,
    );
    let anchors = |axis: usize, positions: [f32; 2]| {
        positions.map(|position| {
            let mut point = corner;
            point[axis] = position;
            point
        })
    };
    let primary: [Option<&Measurement>; 3] = std::array::from_fn(|axis| {
        annotations.measurements.iter().find(|measurement| {
            let first = measurement.points[0][axis];
            let second = measurement.points[1][axis];
            measurement.axis == axis
                && (first.min(second) - minimum[axis]).abs() <= TOLERANCE_MM
                && (first.max(second) - maximum[axis]).abs() <= TOLERANCE_MM
        })
    });
    let axes = (0..3).map(|axis| DisplayDimension {
        direction: ["X", "Y", "Z"][axis],
        points: anchors(axis, [minimum[axis], maximum[axis]]),
        name: primary[axis].map_or(
            ["Overall X", "Overall Y", "Overall Z"][axis],
            |measurement| &measurement.name,
        ),
        distance_mm: maximum[axis] - minimum[axis],
        checked: primary[axis].is_some_and(|measurement| measurement.checked),
        selected: false,
        offset_px: if axis == 2 { 0.0 } else { 24.0 },
    });
    let mut lanes = [0_u32; 3];
    let extra = annotations
        .measurements
        .iter()
        .filter(|measurement| {
            !primary
                .iter()
                .flatten()
                .any(|primary| primary.name == measurement.name)
        })
        .map(|measurement| {
            lanes[measurement.axis] += 1;
            DisplayDimension {
                direction: ["X", "Y", "Z"][measurement.axis],
                points: anchors(
                    measurement.axis,
                    measurement.points.map(|point| point[measurement.axis]),
                ),
                name: &measurement.name,
                distance_mm: measurement.distance_mm,
                checked: measurement.checked,
                selected: false,
                offset_px: 24.0 + lanes[measurement.axis] as f32 * 18.0,
            }
        });
    draw_dimensions(ui, rect, matrix, center, axes.chain(extra));
}

fn draw_dimensions<'a>(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    matrix: Mat4,
    center: Vec3,
    dimensions: impl Iterator<Item = DisplayDimension<'a>>,
) {
    let project = |point: Vec3| -> Option<egui::Pos2> {
        let point = matrix * point.extend(1.0);
        if point.w <= 0.1 || !(0.0..=point.w).contains(&point.z) {
            return None;
        }
        Some(
            rect.center()
                + egui::vec2(
                    point.x / point.w * rect.width() * 0.5,
                    -point.y / point.w * rect.height() * 0.5,
                ),
        )
    };
    let Some(center_screen) = project(center) else {
        return;
    };
    let painter = ui.painter().with_clip_rect(rect);
    let line_color = egui::Color32::from_rgba_unmultiplied(140, 150, 145, 90);
    for (index, dimension) in dimensions.enumerate() {
        let [Some(first), Some(second)] = dimension.points.map(&project) else {
            continue;
        };
        let length_px = first.distance(second);
        let direction = if length_px > 0.01 {
            (second - first) / length_px
        } else {
            egui::Vec2::X
        };
        let mut outward = direction.rot90();
        if outward.dot(first.lerp(second, 0.5) - center_screen) < 0.0 {
            outward = -outward;
        }
        let offset = outward * dimension.offset_px;
        let endpoints = [first + offset, second + offset];
        let mut angle = direction.y.atan2(direction.x);
        if angle >= std::f32::consts::FRAC_PI_2 {
            angle -= std::f32::consts::PI;
        }
        if angle < -std::f32::consts::FRAC_PI_2 {
            angle += std::f32::consts::PI;
        }
        let text = format!("≈{:.2} mm", dimension.distance_mm);
        let galley =
            painter.layout_no_wrap(text, egui::FontId::monospace(15.0), egui::Color32::WHITE);
        let rotation = egui::emath::Rot2::from_angle(angle);
        let short = length_px < galley.size().x + 12.0;
        let label_center = endpoints[0].lerp(endpoints[1], 0.5)
            + outward * (galley.size().y * 0.5 + 6.0)
            + if short {
                direction * (galley.size().x * 0.5 + 12.0)
            } else {
                egui::Vec2::ZERO
            };
        let half_size = galley.size() * 0.5 + egui::vec2(6.0, 3.0);
        let corners = [
            egui::vec2(-half_size.x, -half_size.y),
            egui::vec2(half_size.x, -half_size.y),
            half_size,
            egui::vec2(-half_size.x, half_size.y),
        ]
        .map(|point| label_center + rotation * point);
        let response = ui.interact(
            egui::Rect::from_points(&corners),
            ui.make_persistent_id(("dimension_label", index)),
            egui::Sense::hover(),
        );
        let active = response.hovered() || dimension.selected;
        response.on_hover_text(format!(
            "{} · {} · {}",
            dimension.name,
            dimension.direction,
            if dimension.checked {
                "distance check passed"
            } else {
                "reference dimension"
            }
        ));
        if dimension.offset_px > 0.0 {
            for (anchor, endpoint) in [first, second].into_iter().zip(endpoints) {
                painter.line_segment(
                    [anchor + outward * 3.0, endpoint + outward * 4.0],
                    egui::Stroke::new(1.0, line_color),
                );
            }
        }
        painter.line_segment(endpoints, egui::Stroke::new(1.0, line_color));
        if short {
            painter.line_segment(
                [
                    endpoints[1],
                    endpoints[1] + direction * (galley.size().x + 12.0),
                ],
                egui::Stroke::new(1.0, line_color),
            );
        }
        let (background, border) = if active {
            (
                egui::Color32::from_rgb(22, 36, 52),
                egui::Color32::from_rgb(89, 135, 189),
            )
        } else {
            (
                egui::Color32::from_rgb(18, 23, 29),
                egui::Color32::from_rgb(66, 84, 73),
            )
        };
        painter.add(egui::Shape::convex_polygon(
            corners.to_vec(),
            background,
            egui::Stroke::new(1.0, border),
        ));
        let color = if active {
            egui::Color32::from_rgb(194, 220, 255)
        } else {
            egui::Color32::from_rgb(196, 237, 180)
        };
        let text_position = label_center - rotation * (galley.size() * 0.5);
        let pixels_per_point = ui.ctx().pixels_per_point();
        let text_position = egui::pos2(
            (text_position.x * pixels_per_point).round() / pixels_per_point,
            (text_position.y * pixels_per_point).round() / pixels_per_point,
        );
        painter.add(
            egui::epaint::TextShape::new(text_position, galley, color)
                .with_override_text_color(color)
                .with_angle(angle),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimension_labels_follow_edges_stay_upright_and_merge_matching_checks() {
        let camera = crate::viewport::Camera::rack([40.0, 30.0, 20.0]);
        let annotations = Annotations {
            faces: Vec::new(),
            measurements: vec![Measurement {
                name: "height".into(),
                axis: 2,
                faces: [1, 2],
                points: [Vec3::new(20.0, 15.0, 0.0), Vec3::new(20.0, 15.0, 20.0)],
                distance_mm: 20.0,
                checked: true,
            }],
        };
        let output = egui::Context::default().run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 600.0),
                )),
                ..Default::default()
            },
            |ui| {
                draw(
                    ui,
                    ui.available_rect_before_wrap(),
                    camera.matrix(1.0),
                    camera.eye(),
                    [Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)],
                    &annotations,
                    None,
                );
            },
        );
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(labels.len(), 3);
        assert!(
            labels
                .iter()
                .all(|label| label.angle >= -std::f32::consts::FRAC_PI_2
                    && label.angle < std::f32::consts::FRAC_PI_2)
        );
        assert!(labels.iter().any(|label| label.angle.abs() > 0.2));
        assert!(
            labels
                .iter()
                .all(|label| label.galley.job.text.starts_with('≈'))
        );
        assert!(
            labels
                .iter()
                .all(|label| !label.galley.job.text.contains("height"))
        );
        assert!(
            labels
                .iter()
                .all(|label| label.override_text_color
                    == Some(egui::Color32::from_rgb(196, 237, 180)))
        );
        output.drop_without_applying_deltas();
    }

    #[test]
    fn named_faces_and_distance_checks_follow_rebuilt_geometry() -> Result<(), String> {
        let source = include_str!("../../models/measured-block.luau");
        for (source, height) in [
            (source.to_owned(), 20.0),
            (source.replace("= 20", "= 24").replace("= 40", "= 55"), 24.0),
        ] {
            let snapshot = crate::build_script_snapshot(&source, 1)?;
            assert_eq!(snapshot.annotations.faces.len(), 2);
            assert_eq!(snapshot.annotations.faces[0].name, "top");
            assert!((snapshot.annotations.faces[0].point.z - height).abs() < TOLERANCE_MM);
            assert!(
                (snapshot.annotations.measurements[0].distance_mm - height).abs() < TOLERANCE_MM
            );
            assert!(snapshot.annotations.measurements[0].checked);
            let selector = selector(&snapshot.mesh, snapshot.annotations.faces[0].id)
                .ok_or("Missing selector")?;
            assert!(selector.contains("normal = \"z+\""));
        }
        Ok(())
    }

    #[test]
    fn selectors_reject_ambiguity_and_resolve_explicit_planes() -> Result<(), String> {
        let source = "--!strict\nlocal base = cad.box({width=20,depth=20,height=10})\nlocal step = cad.box({width=10,depth=20,height=10,z=10})\nlocal part = cad.union(base,step)\nlocal top = cad.face(part,{name='top',normal='z+'})\ncad.show('stepped',part)";
        let error = crate::build_script_snapshot(source, 1)
            .err()
            .ok_or("Expected ambiguity")?;
        assert!(error.contains("ambiguous selector"), "{error}");
        let source = source.replace("normal='z+'", "normal='z+',at=20");
        let snapshot = crate::build_script_snapshot(&source, 1)?;
        assert!((snapshot.annotations.faces[0].point.z - 20.0).abs() < TOLERANCE_MM);
        let error = crate::build_script_snapshot(&source.replace("at=20", "at=30"), 1)
            .err()
            .ok_or("Expected missing face")?;
        assert!(error.contains("no matching"), "{error}");
        let error =
            crate::build_script_snapshot(&source.replace("cad.face(part", "cad.face(base"), 1)
                .err()
                .ok_or("Expected intermediate solid rejection")?;
        assert!(error.contains("final solid"), "{error}");
        Ok(())
    }

    #[test]
    fn failing_checks_block_preview_and_export_and_reference_dimensions_remain_unchecked()
    -> Result<(), String> {
        let source = include_str!("../../models/measured-block.luau");
        let invalid = source.replace("max = 25", "max = 19");
        let error = crate::build_script_snapshot(&invalid, 1)
            .err()
            .ok_or("Expected constraint failure")?;
        assert!(error.contains("Constraint 'height'"), "{error}");
        let folder = std::path::PathBuf::from(format!(
            "output/native/rejected-measurement-{}",
            std::process::id()
        ));
        assert!(!folder.exists());
        let error = crate::export::model_files(&invalid, &folder)
            .expect_err("Invalid constraints must block export");
        assert!(error.contains("Constraint 'height'"), "{error}");
        assert!(!folder.exists());
        let reference = source.replace(", min = 2, max = 25", "");
        let snapshot = crate::build_script_snapshot(&reference, 1)?;
        assert!(!snapshot.annotations.measurements[0].checked);
        let perpendicular = source.replace(
            "name = \"top\", normal = \"z+\"",
            "name = \"top\", normal = \"x+\"",
        );
        let error = crate::build_script_snapshot(&perpendicular, 1)
            .err()
            .ok_or("Expected parallel face requirement")?;
        assert!(error.contains("faces must be parallel"), "{error}");
        Ok(())
    }

    #[test]
    fn annotation_api_validates_names_limits_and_unknown_options() {
        let source = include_str!("../../models/measured-block.luau");
        for (source, expected) in [
            (
                source.replace("name = \"bottom\"", "name = \"top\""),
                "unique",
            ),
            (source.replace("name = \"top\"", "name = \"\""), "1–80"),
            (
                source.replace("min = 2", "min = 26"),
                "min must not exceed max",
            ),
            (source.replace("min = 2", "min = 0/0"), "finite"),
        ] {
            let error = crate::model::compile(&source)
                .err()
                .expect("Invalid declaration");
            assert!(error.contains(expected), "Expected {expected}: {error}");
        }
    }
}
