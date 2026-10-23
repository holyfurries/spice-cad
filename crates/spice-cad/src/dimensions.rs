use crate::{mesh::Mesh, model::Solid};
use eframe::egui;
use glam::{Mat4, Vec3};
use mlua::{AnyUserData, Lua, Table, UserData};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

const COUNT_MAX: usize = 64;
const TOLERANCE_MM: f32 = 0.001;

#[derive(Clone, Copy)]
pub(crate) struct Face(pub usize);
impl UserData for Face {}

#[derive(Clone)]
pub(crate) struct Selector {
    pub solid: usize,
    pub name: String,
    pub axis: usize,
    pub positive: bool,
    at: Option<f32>,
    near: Option<Vec3>,
}
struct Requirement {
    line: usize,
    name: String,
    first: usize,
    second: usize,
    parameter: Option<String>,
    minimum: Option<f64>,
    maximum: Option<f64>,
}
#[derive(Default)]
pub struct Definitions {
    faces: Vec<Selector>,
    modeling_faces: u64,
    measurements: Vec<Requirement>,
    parameters: Vec<crate::parameters::Parameter>,
}
#[derive(Clone)]
pub struct NamedFace {
    pub name: String,
    pub id: u32,
    pub point: Vec3,
    normal: Vec3,
}
#[derive(Clone)]
pub struct Measurement {
    pub parameter: Option<crate::parameters::Parameter>,
    pub line: usize,
    pub name: String,
    axis: usize,
    faces: [u32; 2],
    pub points: [Vec3; 2],
    pub distance_mm: f32,
    pub checked: bool,
}
#[derive(Clone, Default)]
pub struct Annotations {
    pub faces: Vec<NamedFace>,
    pub measurements: Vec<Measurement>,
}
pub struct Planes {
    faces: BTreeMap<u32, Plane>,
}

struct Plane {
    axis: Option<(usize, bool)>,
    minimum: Vec3,
    maximum: Vec3,
}
struct DisplayDimension<'a> {
    measurement: Option<&'a Measurement>,
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
            crate::model::options_fields(&options, &["name", "normal", "at", "near"])?;
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
            let near = options
                .get::<Option<Table>>("near")?
                .map(|point| {
                    crate::model::options_fields(&point, &["x", "y", "z"])?;
                    let values: [f64; 3] = [point.get("x")?, point.get("y")?, point.get("z")?];
                    if values
                        .iter()
                        .any(|value| !value.is_finite() || value.abs() > 2000.0)
                    {
                        return Err(mlua::Error::runtime(
                            "Face near coordinates must be finite and within ±2000 mm",
                        ));
                    }
                    Ok(Vec3::from_array(values.map(|value| value as f32)))
                })
                .transpose()?;
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
                near,
            });
            Ok(Face(index))
        })?,
    )?;
    let shared = Rc::clone(&definitions);
    cad.set(
        "measure",
        lua.create_function(move |lua, (name, options): (String, Table)| {
            valid_name(&name)?;
            crate::model::options_fields(&options, &["from", "to", "min", "max", "parameter"])?;
            let first = options.get::<AnyUserData>("from")?.borrow::<Face>()?.0;
            let second = options.get::<AnyUserData>("to")?.borrow::<Face>()?.0;
            let parameter: Option<String> = options.get("parameter")?;
            if let Some(parameter) = &parameter {
                valid_name(parameter)?;
            }
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
                line: crate::project::source_line(lua),
                name,
                first,
                second,
                parameter,
                minimum,
                maximum,
            });
            Ok(())
        })?,
    )?;
    Ok(definitions)
}

pub(crate) fn valid_name(name: &str) -> mlua::Result<()> {
    if name.is_empty() || name.len() > 80 || name.chars().any(char::is_control) {
        return Err(mlua::Error::RuntimeError(
            "Names must contain 1–80 bytes without control characters".into(),
        ));
    }
    Ok(())
}

pub fn planes(mesh: &Mesh) -> Planes {
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
    Planes { faces: planes }
}

impl Measurement {
    pub fn visible(&self, view: &crate::picking::View<'_>) -> bool {
        self.faces.iter().all(|id| view.face_visible(*id))
    }

    pub fn matches_selection(&self, selection: &crate::inspection::Selection) -> bool {
        let ids = selection.ids();
        match selection {
            crate::inspection::Selection::Face(_) => self.faces.contains(&ids[0]),
            crate::inspection::Selection::Pair { .. } => {
                self.faces == ids || self.faces == [ids[1], ids[0]]
            }
            _ => false,
        }
    }

    pub fn selection(
        &self,
        faces: &BTreeMap<u32, crate::inspection::Face>,
    ) -> Option<crate::inspection::Selection> {
        Some(crate::inspection::Selection::Pair {
            first: *faces.get(&self.faces[0])?,
            second: *faces.get(&self.faces[1])?,
        })
    }
}

impl Selector {
    pub fn resolve(&self, planes: &Planes) -> Result<(u32, [Vec3; 2]), String> {
        let candidates = planes
            .faces
            .iter()
            .filter(|(_, plane)| plane.axis == Some((self.axis, self.positive)))
            .map(|(id, plane)| Ok((*id, [plane.minimum.as_dvec3(), plane.maximum.as_dvec3()])));
        let (id, bounds) = self.resolve_candidates(candidates)?;
        Ok((id, bounds.map(|point| point.as_vec3())))
    }

    pub fn resolve_candidates<T>(
        &self,
        candidates: impl Iterator<Item = Result<(T, [glam::DVec3; 2]), String>>,
    ) -> Result<(T, [glam::DVec3; 2]), String> {
        let mut chosen = None;
        let mut second_distance = f64::INFINITY;
        for candidate in candidates {
            let (id, bounds) = candidate?;
            if self.at.is_some_and(|at| {
                (bounds[0][self.axis] - f64::from(at)).abs() > f64::from(TOLERANCE_MM)
            }) {
                continue;
            }
            let distance = self.near.map_or(0.0, |near| {
                near.as_dvec3().distance((bounds[0] + bounds[1]) * 0.5)
            });
            match chosen.as_ref() {
                None => chosen = Some((id, bounds, distance)),
                Some((_, _, previous)) if distance < *previous => {
                    second_distance = *previous;
                    chosen = Some((id, bounds, distance));
                }
                _ => second_distance = second_distance.min(distance),
            }
        }
        let (id, bounds, distance) = chosen
            .ok_or_else(|| format!("Face '{}': no matching axis-aligned planar face", self.name))?;
        if second_distance - distance <= f64::from(TOLERANCE_MM) {
            return Err(format!(
                "Face '{}': ambiguous selector; add an at coordinate or a near point",
                self.name
            ));
        }
        Ok((id, bounds))
    }

    pub fn geometry_matches(&self, other: &Self) -> bool {
        self.axis == other.axis
            && self.positive == other.positive
            && self.at == other.at
            && self.near == other.near
    }
}

impl Definitions {
    pub fn modeling_face(&mut self, index: usize, solid: usize) -> mlua::Result<Selector> {
        assert!(index < self.faces.len());
        assert!(index < COUNT_MAX);
        let face = &self.faces[index];
        if face.solid != solid {
            return Err(mlua::Error::runtime(format!(
                "Face '{}' belongs to a different source solid; select it on the solid passed to the modeling operation",
                face.name
            )));
        }
        self.modeling_faces |= 1u64 << index;
        Ok(face.clone())
    }
    pub fn validate_parameters(
        &mut self,
        parameters: Option<Vec<crate::parameters::Parameter>>,
    ) -> Result<(), String> {
        if !self
            .measurements
            .iter()
            .any(|measurement| measurement.parameter.is_some())
        {
            return Ok(());
        }
        self.parameters = parameters.ok_or("Could not read measurement parameter declarations")?;
        for measurement in &self.measurements {
            if let Some(name) = &measurement.parameter {
                let mut matches = self
                    .parameters
                    .iter()
                    .filter(|parameter| parameter.name == *name);
                match (matches.next(), matches.next()) {
                    (Some(parameter), None) if parameter.is_numeric() => {}
                    _ => {
                        return Err(format!(
                            "Measurement '{}': parameter '{name}' must name one declared numeric cad.number control",
                            measurement.name
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    pub fn resolve(
        &self,
        outputs: &[(usize, std::ops::RangeInclusive<u32>, Mat4, &Planes)],
    ) -> Result<Annotations, String> {
        assert!(self.faces.len() <= COUNT_MAX);
        assert!(self.measurements.len() <= COUNT_MAX);
        let mut annotations = Annotations::default();
        if self.faces.is_empty() {
            return Ok(annotations);
        }
        let mut face_indices = [None; COUNT_MAX];
        for (index, selector) in self.faces.iter().enumerate() {
            let output = outputs
                .iter()
                .find(|(solid, _, _, _)| *solid == selector.solid);
            if output.is_none()
                && self.modeling_faces & (1u64 << index) != 0
                && !self
                    .measurements
                    .iter()
                    .any(|measurement| measurement.first == index || measurement.second == index)
            {
                continue;
            }
            let (_, range, placement, planes) = output.ok_or_else(|| {
                format!(
                    "Face '{}': select faces on the final solid passed to cad.show",
                    selector.name
                )
            })?;
            let (id, bounds) = selector.resolve(planes)?;
            face_indices[index] = Some(annotations.faces.len());
            annotations.faces.push(NamedFace {
                name: selector.name.clone(),
                id: range.start() + id - 1,
                point: placement.transform_point3((bounds[0] + bounds[1]) * 0.5),
                normal: placement
                    .transform_vector3(Vec3::AXES[selector.axis])
                    .normalize(),
            });
        }
        for requirement in &self.measurements {
            let first = &annotations.faces
                [face_indices[requirement.first].ok_or("Missing measurement face")?];
            let second = &annotations.faces
                [face_indices[requirement.second].ok_or("Missing measurement face")?];
            if first.normal.cross(second.normal).length_squared() > 1e-8 {
                return Err(format!(
                    "Measurement '{}': faces must be parallel",
                    requirement.name
                ));
            }
            let mut points = [first.point, second.point];
            let signed_distance = (points[1] - points[0]).dot(first.normal);
            let distance_mm = signed_distance.abs();
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
            let middle = (points[0] + points[1]) * 0.5;
            points = [
                middle - first.normal * signed_distance * 0.5,
                middle + first.normal * signed_distance * 0.5,
            ];
            let axis = (0..3)
                .find(|axis| first.normal[*axis].abs() > 0.99999)
                .unwrap_or(3);
            annotations.measurements.push(Measurement {
                parameter: requirement.parameter.as_ref().map(|name| {
                    self.parameters
                        .iter()
                        .find(|parameter| parameter.name == *name)
                        .expect("Validated measurement parameter")
                        .clone()
                }),
                line: requirement.line,
                name: requirement.name.clone(),
                axis,
                faces: [first.id, second.id],
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
    let plane = planes.faces.get(&id)?;
    let (axis, positive) = plane.axis?;
    let center = (plane.minimum + plane.maximum) * 0.5;
    Some(format!(
        "cad.face(part, {{ name = \"face_name\", normal = \"{}{}\", at = {:.6}, near = {{ x = {:.6}, y = {:.6}, z = {:.6} }} }})",
        ["x", "y", "z"][axis],
        if positive { "+" } else { "-" },
        plane.minimum[axis],
        center.x,
        center.y,
        center.z,
    ))
}

pub fn draw_sample(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    matrix: Mat4,
    points: [Vec3; 2],
    distance_mm: f32,
    label: &str,
) {
    let project = |point: Vec3| {
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
    let [Some(first), Some(second)] = points.map(project) else {
        return;
    };
    let anchor = first.lerp(second, 0.5);
    if !rect.contains(anchor) {
        return;
    }
    let painter = ui.painter().with_clip_rect(rect);
    let color = crate::interface::WARNING;
    let galley = painter.layout_no_wrap(
        format!("{label} ≈{distance_mm:.3} mm"),
        egui::FontId::monospace(15.0),
        egui::Color32::WHITE,
    );
    let size = galley.size() + egui::vec2(16.0, 10.0);
    let available = rect.shrink(8.0);
    let position = egui::pos2(
        (anchor.x + 18.0).clamp(
            available.left(),
            (available.right() - size.x).max(available.left()),
        ),
        (anchor.y - 36.0).clamp(
            available.top(),
            (available.bottom() - size.y).max(available.top()),
        ),
    );
    let background = egui::Rect::from_min_size(position, size);
    painter.line_segment([first, second], egui::Stroke::new(2.0, color));
    for point in [first, second] {
        painter.circle_filled(point, 3.5, color);
    }
    painter.line_segment([anchor, background.center()], egui::Stroke::new(1.0, color));
    painter.rect_filled(background.expand(2.0), 4.0, crate::interface::PANEL);
    painter.rect_stroke(
        background,
        3.0,
        egui::Stroke::new(1.0, color),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        position + egui::vec2(8.0, 5.0),
        galley,
        egui::Color32::WHITE,
    );
}

pub fn draw(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    matrix: Mat4,
    bounds: [Vec3; 2],
    annotations: &Annotations,
    selected: Option<&crate::inspection::Selection>,
    face_visible: impl Fn(u32) -> bool,
) -> Option<String> {
    let measurement_visible =
        |measurement: &Measurement| measurement.faces.into_iter().all(&face_visible);
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
        let named_pair = matches!(selection, crate::inspection::Selection::Pair { .. })
            && annotations.measurements.iter().any(|measurement| {
                measurement_visible(measurement) && measurement.matches_selection(selection)
            });
        let local = spans
            .iter()
            .flatten()
            .filter(|_| !named_pair)
            .enumerate()
            .map(|(index, span)| DisplayDimension {
                measurement: None,
                direction: span.direction,
                points: span.points,
                name: span.direction,
                distance_mm: span.distance_mm,
                checked: false,
                selected: true,
                offset_px: 24.0
                    + if matches!(selection, crate::inspection::Selection::Edge(_)) {
                        index as f32 * 28.0
                    } else {
                        0.0
                    },
            });
        let related = annotations
            .measurements
            .iter()
            .filter(|measurement| {
                measurement_visible(measurement) && measurement.matches_selection(selection)
            })
            .map(|measurement| DisplayDimension {
                measurement: Some(measurement),
                direction: ["X", "Y", "Z", "Distance"][measurement.axis],
                points: measurement.points,
                name: &measurement.name,
                distance_mm: measurement.distance_mm,
                checked: measurement.checked,
                selected: true,
                offset_px: 48.0,
            });
        let clicked = draw_dimensions(ui, rect, matrix, selection.center(), local.chain(related));
        if let Some(face) = annotations.faces.iter().find(|face| {
            selection
                .latest()
                .is_some_and(|selected| face.id == selected.id)
        }) {
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
        return clicked;
    }
    let [minimum, maximum] = bounds;
    assert!(minimum.is_finite());
    assert!(maximum.is_finite());
    let center = (minimum + maximum) * 0.5;
    let view_direction = -matrix.row(3).truncate();
    let corner = Vec3::new(
        if view_direction.x >= -0.0001 {
            maximum.x
        } else {
            minimum.x
        },
        if view_direction.y >= -0.0001 {
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
            if !measurement_visible(measurement) {
                return false;
            }
            let first = measurement.points[0][axis];
            let second = measurement.points[1][axis];
            measurement.axis == axis
                && (first.min(second) - minimum[axis]).abs() <= TOLERANCE_MM
                && (first.max(second) - maximum[axis]).abs() <= TOLERANCE_MM
        })
    });
    let axes = (0..3).map(|axis| DisplayDimension {
        measurement: primary[axis],
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
    let mut lanes = [0_u32; 4];
    let extra = annotations
        .measurements
        .iter()
        .filter(|measurement| measurement_visible(measurement))
        .filter(|measurement| {
            !primary
                .iter()
                .flatten()
                .any(|primary| primary.name == measurement.name)
        })
        .map(|measurement| {
            lanes[measurement.axis] += 1;
            DisplayDimension {
                measurement: Some(measurement),
                direction: ["X", "Y", "Z", "Distance"][measurement.axis],
                points: if measurement.axis < 3 {
                    anchors(
                        measurement.axis,
                        measurement.points.map(|point| point[measurement.axis]),
                    )
                } else {
                    measurement.points
                },
                name: &measurement.name,
                distance_mm: measurement.distance_mm,
                checked: measurement.checked,
                selected: false,
                offset_px: 24.0 + lanes[measurement.axis] as f32 * 18.0,
            }
        });
    draw_dimensions(ui, rect, matrix, center, axes.chain(extra))
}

fn draw_dimensions<'a>(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    matrix: Mat4,
    center: Vec3,
    dimensions: impl Iterator<Item = DisplayDimension<'a>>,
) -> Option<String> {
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
    let screen_vector = |vector: Vec3| {
        let projected = matrix * vector.extend(0.0);
        egui::vec2(projected.x * rect.width(), -projected.y * rect.height())
    };
    let painter = ui.painter().with_clip_rect(rect);
    let line_color = egui::Color32::from_rgba_unmultiplied(140, 150, 145, 90);
    let mut clicked = None;
    for (index, dimension) in dimensions.enumerate() {
        let [Some(first), Some(second)] = dimension.points.map(&project) else {
            continue;
        };
        let length_px = first.distance(second);
        if length_px < 1.0 {
            continue;
        }
        let direction = (second - first) / length_px;
        let projected_axis = screen_vector(dimension.points[1] - dimension.points[0]);
        let reference_direction = if projected_axis.length() > 0.0001 {
            projected_axis.normalized()
        } else {
            direction
        };
        let mut outward = direction.rot90();
        let reference = screen_vector((dimension.points[0] + dimension.points[1]) * 0.5 - center);
        if reference_direction.rot90().dot(reference) < -0.0001 * reference.length().max(1.0) {
            outward = -outward;
        }
        let offset = outward * dimension.offset_px;
        let endpoints = [first + offset, second + offset];
        let reverse = if reference_direction.x.abs() <= 0.0001 {
            reference_direction.y > 0.0
        } else {
            reference_direction.x < 0.0
        };
        let baseline = if reverse { -direction } else { direction };
        let angle = baseline.y.atan2(baseline.x);
        let prefix = match dimension.direction {
            "Radius" => "R ",
            "Diameter" => "Ø ",
            _ => "",
        };
        let text = format!("{prefix}≈{:.2} mm", dimension.distance_mm);
        let galley =
            painter.layout_no_wrap(text, egui::FontId::monospace(15.0), egui::Color32::WHITE);
        let rotation = egui::emath::Rot2::from_angle(angle);
        let extension_px = (galley.size().x + 12.0 - length_px).max(0.0);
        let label_center = endpoints[0].lerp(endpoints[1], 0.5)
            + outward * (galley.size().y * 0.5 + 6.0)
            + direction * (extension_px * 0.5);
        let half_size = galley.size() * 0.5 + egui::vec2(6.0, 3.0);
        let corners = [
            egui::vec2(-half_size.x, -half_size.y),
            egui::vec2(half_size.x, -half_size.y),
            half_size,
            egui::vec2(-half_size.x, half_size.y),
        ]
        .map(|point| label_center + rotation * point);
        let response = ui.interact(
            egui::Rect::from_points(&corners).intersect(rect),
            ui.make_persistent_id(("dimension_label", index)),
            if dimension.measurement.is_some() {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            },
        );
        if response.clicked() {
            if let Some(measurement) = dimension.measurement {
                clicked = Some(measurement.name.clone());
            }
        }
        let active = response.hovered() || dimension.selected;
        response.on_hover_text(format!(
            "{} · {} · {}{}",
            dimension.name,
            dimension.direction,
            if dimension.checked {
                "distance check passed"
            } else {
                "reference dimension"
            },
            dimension
                .measurement
                .and_then(|measurement| measurement.parameter.as_ref())
                .map_or(String::new(), |parameter| format!(
                    " · linked to {} · click to edit linked parameter",
                    parameter.name
                ))
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
        if extension_px > 0.0 {
            painter.line_segment(
                [endpoints[1], endpoints[1] + direction * extension_px],
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
    clicked
}

#[cfg(test)]
mod tests {
    #[test]
    fn direct_face_queries_respect_circular_trims_without_polygon_approximation()
    -> Result<(), String> {
        let kernel = crate::kernel::Kernel::new()?;
        let mut body = kernel.cylinder([0.0; 3], 10.0, 8.0)?;
        body.subtract(kernel.cylinder([0.0, 0.0, -1.0], 2.0, 10.0)?)?;
        let selector = super::Selector {
            solid: 0,
            name: "annulus".into(),
            axis: 2,
            positive: true,
            at: None,
            near: None,
        };
        let angle = 7.5f64.to_radians();
        for (radius, valid) in [
            (1.995, false),
            (2.005, true),
            (9.995, true),
            (10.005, false),
        ] {
            let offset = [radius * angle.cos(), radius * angle.sin()];
            let origin = body.planar_feature_origin(&selector, offset);
            if valid {
                let point = origin?;
                assert!((point - glam::DVec3::new(offset[0], offset[1], 8.0)).length() < 1e-6);
            } else {
                assert!(origin.is_err(), "Accepted radius {radius}");
            }
        }
        Ok(())
    }

    #[test]
    fn direct_face_queries_keep_rust_allocations_small_across_curved_surface_sizes()
    -> Result<(), String> {
        let kernel = crate::kernel::Kernel::new()?;
        let selector = super::Selector {
            solid: 0,
            name: "top".into(),
            axis: 2,
            positive: true,
            at: None,
            near: None,
        };
        let mut previous = None;
        for radius in [5.0, 100.0] {
            let mut body = kernel.cylinder([0.0; 3], radius, 8.0)?;
            let (origin, counts) =
                crate::allocations::measure(|| body.planar_feature_origin(&selector, [0.0; 2]));
            assert!((origin? - glam::DVec3::Z * 8.0).length() < 1e-6);
            assert!(counts.bytes < 4096, "{counts:?}");
            if let Some(previous) = previous {
                assert_eq!(counts, previous);
            }
            previous = Some(counts);
        }
        Ok(())
    }

    #[test]
    fn direct_face_queries_use_cavity_normals_and_exact_local_coordinates() -> Result<(), String> {
        let kernel = crate::kernel::Kernel::new()?;
        let mut body = kernel.box_shape([0.0; 3], [20.0; 3])?;
        body.subtract(kernel.box_shape([5.0; 3], [15.0; 3])?)?;
        for axis in 0..3 {
            for positive in [false, true] {
                let coordinate = if positive { 5.0 } else { 15.0 };
                let selector = super::Selector {
                    solid: 0,
                    name: "cavity".into(),
                    axis,
                    positive,
                    at: Some(coordinate as f32),
                    near: None,
                };
                let mut expected = glam::DVec3::splat(10.0);
                expected[axis] = coordinate;
                assert!(
                    (body.planar_feature_origin(&selector, [0.0; 2])? - expected).length() < 1e-6
                );
            }
        }
        let mut precise = kernel.box_shape(
            [7.123456789, 11.876543219, 13.123456789],
            [27.123456789, 31.876543219, 21.123456789],
        )?;
        let selector = super::Selector {
            solid: 0,
            name: "precise".into(),
            axis: 2,
            positive: true,
            at: None,
            near: None,
        };
        let expected = glam::dvec3(17.123456789, 21.876543219, 21.123456789);
        assert!((precise.planar_feature_origin(&selector, [0.0; 2])? - expected).length() < 1e-8);
        Ok(())
    }

    #[test]
    fn direct_face_queries_resolve_steps_and_reject_concave_gaps_and_oblique_planes()
    -> Result<(), String> {
        let kernel = crate::kernel::Kernel::new()?;
        let mut body = kernel.box_shape([0.0; 3], [20.0, 20.0, 8.0])?;
        body.union(kernel.box_shape([0.0; 3], [10.0, 10.0, 16.0])?)?;
        let mut selector = super::Selector {
            solid: 0,
            name: "step".into(),
            axis: 2,
            positive: true,
            at: None,
            near: None,
        };
        assert!(
            body.planar_feature_origin(&selector, [0.0; 2])
                .err()
                .is_some_and(|error| error.contains("ambiguous"))
        );
        selector.near = Some(glam::vec3(5.0, 5.0, 16.0));
        assert!(
            (body.planar_feature_origin(&selector, [0.0; 2])? - glam::dvec3(5.0, 5.0, 16.0))
                .length()
                < 1e-6
        );
        selector.near = None;
        selector.at = Some(8.0);
        assert!(body.planar_feature_origin(&selector, [-5.0, -5.0]).is_err());
        assert!(
            (body.planar_feature_origin(&selector, [5.0, 5.0])? - glam::dvec3(15.0, 15.0, 8.0))
                .length()
                < 1e-6
        );
        let mut tilted = kernel.box_shape([0.0; 3], [20.0, 20.0, 8.0])?;
        tilted.rotate([1.0, 0.0, 0.0], 15.0)?;
        selector.at = None;
        assert!(
            tilted
                .planar_feature_origin(&selector, [0.0; 2])
                .err()
                .is_some_and(|error| error.contains("no matching"))
        );
        Ok(())
    }

    use super::*;

    #[test]
    fn hidden_measurement_parts_do_not_leave_dimension_labels() -> Result<(), String> {
        let snapshot = crate::build_script_snapshot(
            "local a=cad.box({width=10,depth=10,height=10});local b=cad.translate(a,{z=20});cad.show('a',a);cad.show('b',b);local first=cad.face(a,{name='first',normal='z+'});local second=cad.face(b,{name='second',normal='z+'});cad.measure('cross part',{from=first,to=second})",
            1,
        )?;
        let mut view = snapshot.scene.picking();
        assert!(snapshot.checks.measurements[0].visible(&view));
        view.visible_parts = 1;
        assert!(!snapshot.checks.measurements[0].visible(&view));
        let bounds = snapshot.scene.visible_bounds(1).expect("Visible bounds");
        let mut camera = crate::snapshot_camera(&snapshot, 1.0);
        camera.frame(bounds, 1.0);
        let output = egui::Context::default().run_ui(egui::RawInput::default(), |ui| {
            draw(
                ui,
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 600.0)),
                camera.matrix(1.0),
                bounds,
                &snapshot.scene.annotations,
                None,
                |id| view.face_visible(id),
            );
        });
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect();
        let hidden_label = labels.iter().any(|label| label.contains("20.00"));
        let visible_bounds =
            labels.len() == 3 && labels.iter().all(|label| label.contains("10.00"));
        output.drop_without_applying_deltas();
        assert!(!hidden_label);
        assert!(visible_bounds);
        Ok(())
    }

    fn camera_labels(
        camera: &crate::viewport::Camera,
        bounds: [Vec3; 2],
        selected: Option<&crate::inspection::Selection>,
    ) -> Vec<(String, egui::Pos2, f32)> {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 600.0));
        let output = egui::Context::default().run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            },
            |ui| {
                draw(
                    ui,
                    rect,
                    camera.matrix(1.0),
                    bounds,
                    &Annotations::default(),
                    selected,
                    |_| true,
                );
            },
        );
        let labels = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some((
                    text.galley.job.text.clone(),
                    text.pos
                        + egui::emath::Rot2::from_angle(text.angle) * (text.galley.size() * 0.5),
                    text.angle,
                )),
                _ => None,
            })
            .collect();
        output.drop_without_applying_deltas();
        labels
    }

    #[test]
    fn sampled_gap_callouts_remain_readable_at_zero_and_subpixel_distances() {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 600.0));
        for distance in [0.0, 0.001, 0.1] {
            let camera = crate::viewport::Camera::for_dimensions([100.0; 3]);
            let points = [camera.target, camera.target + Vec3::X * distance];
            let output = egui::Context::default().run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    ..Default::default()
                },
                |ui| {
                    draw_sample(
                        ui,
                        rect,
                        camera.matrix(1.0),
                        points,
                        distance,
                        "Sampled gap",
                    )
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
            assert_eq!(labels.len(), 1);
            assert_eq!(
                labels[0].galley.job.text,
                format!("Sampled gap ≈{distance:.3} mm")
            );
            assert!(rect.contains(labels[0].pos));
            assert_eq!(labels[0].angle, 0.0);
            output.drop_without_applying_deltas();
        }
    }

    #[test]
    fn panning_does_not_switch_dimension_anchor_corners() {
        let bounds = [Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)];
        let center = (bounds[0] + bounds[1]) * 0.5;
        let mut camera = crate::viewport::Camera::for_dimensions([40.0, 30.0, 20.0]);
        camera.yaw = -1.45;
        camera.distance = 150.0;
        let eye_offset_x = camera.yaw.cos() * camera.pitch.cos() * camera.distance;
        camera.target = center - Vec3::X * (eye_offset_x + 0.01);
        let before = camera_labels(&camera, bounds, None);
        camera.target.x += 0.02;
        let after = camera_labels(&camera, bounds, None);
        assert_eq!(before.len(), 3);
        assert_eq!(after.len(), before.len());
        for ((name, before, _), (after_name, after, _)) in before.iter().zip(&after) {
            assert_eq!(name, after_name);
            assert!(
                before.distance(*after) < 2.0,
                "{name} jumped {} pixels",
                before.distance(*after)
            );
        }
    }

    #[test]
    fn panning_vertical_edges_does_not_flip_text_upside_down() {
        let points = [Vec3::ZERO, Vec3::new(0.0, 0.0, 40.0)];
        let selected = crate::inspection::Selection::Edge(crate::inspection::Edge {
            segments: [0, 1],
            geometry: crate::inspection::EdgeGeometry::Line(points),
        });
        let mut camera = crate::viewport::Camera::for_dimensions([1.0, 1.0, 40.0]);
        camera.distance = 200.0;
        camera.target = Vec3::new(-0.01, 0.0, 20.0);
        let before = camera_labels(&camera, points, Some(&selected));
        camera.target.x = 0.01;
        let after = camera_labels(&camera, points, Some(&selected));
        assert_eq!(before.len(), 1);
        assert_eq!(after.len(), 1);
        assert!(
            (before[0].2 - after[0].2).abs() < 0.01,
            "Text angle jumped from {} to {}",
            before[0].2,
            after[0].2
        );
    }

    #[test]
    fn zooming_centered_edge_labels_has_no_side_or_short_edge_jumps() {
        let points = [Vec3::ZERO, Vec3::new(40.0, 0.0, 0.0)];
        let selected = crate::inspection::Selection::Edge(crate::inspection::Edge {
            segments: [0, 1],
            geometry: crate::inspection::EdgeGeometry::Line(points),
        });
        let mut camera = crate::viewport::Camera::for_dimensions([40.0, 1.0, 1.0]);
        camera.target = Vec3::new(20.0, 0.0, 0.0);
        let mut previous: Option<egui::Pos2> = None;
        for distance in 200..=400 {
            camera.distance = distance as f32;
            let labels = camera_labels(&camera, points, Some(&selected));
            assert_eq!(labels.len(), 1);
            assert_eq!(labels[0].0, "≈40.00 mm");
            let position = labels[0].1;
            if let Some(previous) = previous {
                assert!(
                    previous.distance(position) < 3.0,
                    "Distance {distance}: label jumped {} pixels",
                    previous.distance(position)
                );
            }
            previous = Some(position);
        }
    }

    #[test]
    fn dimension_labels_follow_edges_stay_upright_and_merge_matching_checks() {
        let camera = crate::viewport::Camera::for_dimensions([40.0, 30.0, 20.0]);
        let annotations = Annotations {
            faces: Vec::new(),
            measurements: vec![Measurement {
                parameter: None,
                line: 0,
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
                    [Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)],
                    &annotations,
                    None,
                    |_| true,
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
    fn parameter_links_resolve_unicode_source_ranges_and_survive_cached_numeric_edits()
    -> Result<(), String> {
        let source = format!(
            "-- Unicode: λ\n{}",
            include_str!("../tests/fixtures/measured-block.luau")
        );
        let kernel = crate::kernel::Kernel::new()?;
        let mut cache = crate::model::Cache::new(&kernel);
        let first = crate::build_script_snapshot_cached(source.clone(), 1, &mut cache)?;
        let second_source = source
            .replace("cad.number(40,", "cad.number(100,")
            .replace("cad.number(20,", "cad.number(24,");
        let second = crate::build_script_snapshot_cached(second_source.clone(), 2, &mut cache)?;
        for (source, snapshot, expected) in
            [(&source, &first, "20"), (&second_source, &second, "24")]
        {
            let measurement = &snapshot.scene.annotations.measurements[0];
            let binding = measurement
                .parameter
                .as_ref()
                .ok_or("Missing parameter link")?;
            let rope = ropey::Rope::from_str(source);
            assert_eq!(binding.name, "height");
            assert_eq!(rope.slice(binding.range.clone()).to_string(), expected);
            assert_eq!(
                rope.char_to_line(binding.range.start) as u32 + 1,
                binding.line
            );
            let selected = measurement
                .selection(&snapshot.scene.faces)
                .ok_or("Missing selected faces")?;
            assert!(measurement.matches_selection(&selected));
            assert_eq!(selected.ids(), measurement.faces);
            assert!(
                (measurement.distance_mm - expected.parse::<f32>().unwrap()).abs() < TOLERANCE_MM
            );
        }
        let reference = source.replace(", parameter = \"height\"", "");
        let reference = crate::build_script_snapshot_cached(reference, 3, &mut cache)?;
        assert!(reference.checks.measurements[0].parameter.is_none());
        Ok(())
    }

    #[test]
    fn parameter_links_reject_unknown_toggle_computed_and_shadowed_controls() {
        let source = include_str!("../tests/fixtures/measured-block.luau");
        for source in [
            source.replace("parameter = \"height\"", "parameter = \"missing\""),
            source
                .replace("parameter = \"height\"", "parameter = \"enabled\"")
                .replace(
                    "local part =",
                    "local enabled = cad.toggle(true, {})\nlocal part =",
                ),
            source.replace(
                "local height: number = cad.number(20, { label = \"Height\", unit = \"mm\" })",
                "local height: number = 20",
            ),
            source.replace(
                "local part =",
                "local height = cad.number(20, {})\nlocal part =",
            ),
        ] {
            let error = crate::model::compile(&source)
                .err()
                .expect("Invalid measurement link");
            assert!(
                error.contains("declared numeric cad.number control"),
                "{error}"
            );
        }
    }

    #[test]
    fn rotated_measurement_labels_are_clickable_and_use_world_endpoints() -> Result<(), String> {
        let source = include_str!("../tests/fixtures/measured-block.luau").replace(
            "cad.show(\"Measured block\", part)",
            "cad.show(\"Measured block\", part, {assembly={rotation={y=45}}})",
        );
        let snapshot = crate::build_script_snapshot(&source, 1)?;
        assert_eq!(snapshot.checks.measurements[0].axis, 3);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 600.0));
        let camera = crate::snapshot_camera(&snapshot, 1.0);
        let context = egui::Context::default();
        let mut position = None;
        let mut clicked = None;
        for phase in 0..4 {
            let events = position.map_or(Vec::new(), |position| {
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed: phase == 2,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            });
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    events,
                    ..Default::default()
                },
                |ui| {
                    clicked = draw(
                        ui,
                        rect,
                        camera.matrix(1.0),
                        snapshot.scene.bounds,
                        &snapshot.scene.annotations,
                        None,
                        |_| true,
                    )
                    .or(clicked.take());
                },
            );
            if phase == 1 {
                position = output.shapes.iter().find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == "≈20.00 mm" => Some(
                        text.pos
                            + egui::emath::Rot2::from_angle(text.angle)
                                * (text.galley.size() * 0.5),
                    ),
                    _ => None,
                });
                assert!(position.is_some(), "Rotated dimension label is visible");
            }
            output.drop_without_applying_deltas();
        }
        assert_eq!(clicked.as_deref(), Some("height"));
        Ok(())
    }

    #[test]
    fn named_faces_and_distance_checks_follow_rebuilt_geometry() -> Result<(), String> {
        let source = include_str!("../tests/fixtures/measured-block.luau");
        for (source, height) in [
            (source.to_owned(), 20.0),
            (
                source
                    .replace("cad.number(20,", "cad.number(24,")
                    .replace("cad.number(40,", "cad.number(55,"),
                24.0,
            ),
        ] {
            let snapshot = crate::build_script_snapshot(&source, 1)?;
            assert_eq!(snapshot.scene.annotations.faces.len(), 2);
            assert_eq!(snapshot.scene.annotations.faces[0].name, "top");
            assert!((snapshot.scene.annotations.faces[0].point.z - height).abs() < TOLERANCE_MM);
            assert!(
                (snapshot.scene.annotations.measurements[0].distance_mm - height).abs()
                    < TOLERANCE_MM
            );
            assert!(snapshot.scene.annotations.measurements[0].checked);
            let selector = selector(
                &snapshot.scene.world_mesh(),
                snapshot.scene.annotations.faces[0].id,
            )
            .ok_or("Missing selector")?;
            assert!(selector.contains("normal = \"z+\""));
        }
        Ok(())
    }

    #[test]
    fn near_selectors_choose_unique_local_faces_and_reject_ties_and_invalid_points()
    -> Result<(), String> {
        let source = "local base=cad.box({width=20,depth=20,height=2});local step=cad.box({width=10,depth=20,height=10,z=2});local part=cad.union(base,step);cad.face(part,{name='top',normal='z+',near={x=5,y=10,z=12}});cad.show('step',part,{assembly={position={x=100}}})";
        let snapshot = crate::build_script_snapshot(source, 1)?;
        assert!(
            (snapshot.checks.faces[0].point - Vec3::new(105.0, 10.0, 12.0)).length() < TOLERANCE_MM
        );
        for (source, expected) in [
            (
                source.replace("x=5,y=10,z=12", "x=10,y=10,z=7"),
                "ambiguous selector",
            ),
            (
                source.replace("x=5,y=10,z=12", "x=math.huge,y=10,z=12"),
                "finite",
            ),
            (source.replace("x=5,y=10,z=12", "x=2001,y=10,z=12"), "±2000"),
            (
                source.replace("x=5,y=10,z=12", "x=5,y=10"),
                "missing field 'z'",
            ),
        ] {
            let error = crate::build_script_snapshot(&source, 1)
                .err()
                .ok_or("Expected invalid selector")?;
            assert!(error.contains(expected), "{error}");
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
        assert!((snapshot.scene.annotations.faces[0].point.z - 20.0).abs() < TOLERANCE_MM);
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
        let source = include_str!("../tests/fixtures/measured-block.luau");
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
        for target in [crate::export::Target::Print, crate::export::Target::Game] {
            let error = crate::export::model_files(&invalid, &folder, target)
                .expect_err("Invalid constraints must block export");
            assert!(error.contains("Constraint 'height'"), "{error}");
            assert!(!folder.exists());
        }
        let reference = source.replace(", min = 2, max = 25", "");
        let snapshot = crate::build_script_snapshot(&reference, 1)?;
        assert!(!snapshot.scene.annotations.measurements[0].checked);
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
        let source = include_str!("../tests/fixtures/measured-block.luau");
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
