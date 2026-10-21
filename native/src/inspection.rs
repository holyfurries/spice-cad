use crate::mesh::Mesh;
use glam::{DVec3, Mat4, Vec3};
use std::collections::BTreeMap;

const TOLERANCE_MM: f32 = 0.001;

pub struct Edges {
    pub segments: Vec<crate::mesh::EdgeSegment>,
    pub items: Vec<Edge>,
}

#[derive(Clone, Copy)]
pub struct Edge {
    pub segments: [u32; 2],
    pub geometry: EdgeGeometry,
}

#[derive(Clone, Copy)]
pub enum EdgeGeometry {
    Line([Vec3; 2]),
    Circle { center: Vec3, radial: Vec3 },
    Other { center: Vec3 },
}

impl Edge {
    pub fn transformed(self, matrix: Mat4) -> Self {
        let geometry = match self.geometry {
            EdgeGeometry::Line(points) => {
                EdgeGeometry::Line(points.map(|point| matrix.transform_point3(point)))
            }
            EdgeGeometry::Circle { center, radial } => EdgeGeometry::Circle {
                center: matrix.transform_point3(center),
                radial: matrix.transform_vector3(radial),
            },
            EdgeGeometry::Other { center } => EdgeGeometry::Other {
                center: matrix.transform_point3(center),
            },
        };
        Self { geometry, ..self }
    }

    pub fn from_samples(segments: [u32; 2], samples: [DVec3; 17]) -> Self {
        assert!(segments[0] < segments[1]);
        assert!(samples.iter().all(|point| point.is_finite()));
        let [first, middle, last] = [samples[0], samples[8], samples[16]];
        let direction = last - first;
        let tolerance_mm = 1e-7;
        let length_mm = direction.length();
        let geometry = if length_mm > tolerance_mm
            && samples.iter().all(|point| {
                let along_mm = (*point - first).dot(direction) / length_mm;
                (*point - first).cross(direction).length() / length_mm <= tolerance_mm
                    && (-tolerance_mm..=length_mm + tolerance_mm).contains(&along_mm)
            }) {
            EdgeGeometry::Line([first.as_vec3(), last.as_vec3()])
        } else {
            let a = samples[5] - first;
            let b = samples[11] - first;
            let normal = a.cross(b);
            let normal_squared = normal.length_squared();
            let mut geometry = EdgeGeometry::Other {
                center: middle.as_vec3(),
            };
            if normal_squared > 1e-20 {
                let center = first
                    + (b.cross(normal) * a.length_squared() + normal.cross(a) * b.length_squared())
                        / (2.0 * normal_squared);
                let radius_mm = first.distance(center);
                let normal = normal.normalize();
                if samples.iter().all(|point| {
                    (point.distance(center) - radius_mm).abs() <= tolerance_mm
                        && (*point - center).dot(normal).abs() <= tolerance_mm
                }) {
                    geometry = EdgeGeometry::Circle {
                        center: center.as_vec3(),
                        radial: (first - center).as_vec3(),
                    };
                }
            }
            geometry
        };
        Self { segments, geometry }
    }

    pub fn spans(&self) -> [Option<Span>; 3] {
        match self.geometry {
            EdgeGeometry::Line(points) => [
                Some(Span {
                    points,
                    distance_mm: points[0].distance(points[1]),
                    direction: "Edge length",
                }),
                None,
                None,
            ],
            EdgeGeometry::Circle { center, radial } => [
                Some(Span {
                    points: [center, center + radial],
                    distance_mm: radial.length(),
                    direction: "Radius",
                }),
                Some(Span {
                    points: [center - radial, center + radial],
                    distance_mm: radial.length() * 2.0,
                    direction: "Diameter",
                }),
                None,
            ],
            EdgeGeometry::Other { .. } => [None; 3],
        }
    }

    pub fn center(&self) -> Vec3 {
        match self.geometry {
            EdgeGeometry::Line(points) => (points[0] + points[1]) * 0.5,
            EdgeGeometry::Circle { center, .. } | EdgeGeometry::Other { center } => center,
        }
    }
}

#[cfg(test)]
pub fn pick_edge(
    edges: &[Edge],
    segments: &[crate::mesh::EdgeSegment],
    mesh: &Mesh,
    matrix: Mat4,
    size: [f32; 2],
    position: [f32; 2],
) -> Option<Edge> {
    assert!(edges.len() <= 10000);
    assert!(segments.len() <= 131072);
    assert!(
        size.into_iter()
            .all(|value| value.is_finite() && value > 0.0)
    );
    let project = |point: Vec3| {
        let clip = matrix * point.extend(1.0);
        if clip.w <= 0.1 || !(0.0..=clip.w).contains(&clip.z) {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(Vec3::new(
            (ndc.x + 1.0) * size[0] * 0.5,
            (1.0 - ndc.y) * size[1] * 0.5,
            ndc.z,
        ))
    };
    let cursor = glam::Vec2::from_array(position);
    let inverse = matrix.inverse();
    let view_direction =
        (inverse.project_point3(Vec3::Z) - inverse.project_point3(Vec3::ZERO)).normalize();
    let mut closest = 6.0_f32;
    let mut depth = f32::INFINITY;
    let mut selected = None;
    for edge in edges {
        for segment in &segments[edge.segments[0] as usize..edge.segments[1] as usize] {
            let (Some(first), Some(second)) = (
                project(Vec3::from_array(segment.start)),
                project(Vec3::from_array(segment.end)),
            ) else {
                continue;
            };
            let delta = second.truncate() - first.truncate();
            let fraction = ((cursor - first.truncate()).dot(delta)
                / delta.length_squared().max(1e-12))
            .clamp(0.0, 1.0);
            let point = first.lerp(second, fraction);
            let distance = cursor.distance(point.truncate());
            if distance > closest {
                continue;
            }
            if (distance - closest).abs() < 0.25 && point.z >= depth {
                continue;
            }
            let hit = mesh.pick(
                matrix,
                [point.x / size[0] * 2.0 - 1.0, 1.0 - point.y / size[1] * 2.0],
            );
            if let Some(hit) = hit {
                let start = Vec3::from_array(segment.start);
                let end = Vec3::from_array(segment.end);
                let start_w = (matrix * start.extend(1.0)).w;
                let end_w = (matrix * end.extend(1.0)).w;
                let along = fraction * start_w / (end_w * (1.0 - fraction) + start_w * fraction);
                if (start.lerp(end, along) - hit.position).dot(view_direction) > 0.02 {
                    continue;
                }
            }
            closest = distance;
            depth = point.z;
            selected = Some(*edge);
        }
    }
    selected
}

#[derive(Clone, Copy)]
pub struct Span {
    pub points: [Vec3; 2],
    pub distance_mm: f32,
    pub direction: &'static str,
}

#[derive(Clone, Copy)]
pub struct Face {
    pub id: u32,
    pub center: Vec3,
    pub spans: [Option<Span>; 3],
    normal: Option<Vec3>,
}

impl Face {
    pub fn translated(mut self, translation: Vec3) -> Self {
        self.center += translation;
        for span in self.spans.iter_mut().flatten() {
            span.points = span.points.map(|point| point + translation);
        }
        self
    }
}

#[derive(Clone, Copy)]
pub enum Selection {
    Part { index: u32, bounds: [Vec3; 2] },
    Edge(Edge),
    Face(Face),
    Pair { first: Face, second: Face },
}

impl Selection {
    pub fn select(previous: Option<&Self>, face: Face, extend: bool) -> Result<Self, String> {
        assert!(face.id > 0);
        if !extend {
            return Ok(Self::Face(face));
        }
        let Some(previous) = previous else {
            return Ok(Self::Face(face));
        };
        let first = match previous {
            Self::Edge(_) | Self::Part { .. } => return Ok(Self::Face(face)),
            Self::Face(first) => *first,
            Self::Pair { first, second } => {
                if face.id == first.id {
                    return Ok(Self::Face(*second));
                }
                if face.id == second.id {
                    return Ok(Self::Face(*first));
                }
                *first
            }
        };
        if first.id == face.id {
            return Ok(Self::Face(first));
        }
        let (Some(normal), Some(other)) = (first.normal, face.normal) else {
            return Err("Shift-click two parallel planar faces to measure their separation".into());
        };
        if normal.cross(other).length_squared() > 1e-12 {
            return Err("The faces are not parallel; the previous selection was kept".into());
        }
        Ok(Self::Pair {
            first,
            second: face,
        })
    }

    pub fn ids(&self) -> [u32; 2] {
        match self {
            Self::Edge(_) | Self::Part { .. } => [0; 2],
            Self::Face(face) => [face.id, 0],
            Self::Pair { first, second } => [first.id, second.id],
        }
    }

    pub fn latest(&self) -> Option<&Face> {
        match self {
            Self::Edge(_) | Self::Part { .. } => None,
            Self::Face(face) => Some(face),
            Self::Pair { second, .. } => Some(second),
        }
    }

    pub fn spans(&self) -> [Option<Span>; 3] {
        match self {
            Self::Part {
                bounds: [minimum, maximum],
                ..
            } => std::array::from_fn(|axis| {
                let mut end = *minimum;
                end[axis] = maximum[axis];
                Some(Span {
                    points: [*minimum, end],
                    distance_mm: maximum[axis] - minimum[axis],
                    direction: ["X", "Y", "Z"][axis],
                })
            }),
            Self::Edge(edge) => edge.spans(),
            Self::Face(face) => face.spans,
            Self::Pair { first, second } => {
                let normal = first.normal.expect("Parallel planar selection");
                let signed_distance = (second.center - first.center).dot(normal);
                let middle = (first.center + second.center) * 0.5;
                let point = middle - normal * (middle - first.center).dot(normal);
                [
                    Some(Span {
                        points: [point, point + normal * signed_distance],
                        distance_mm: signed_distance.abs(),
                        direction: "Parallel-face distance",
                    }),
                    None,
                    None,
                ]
            }
        }
    }

    pub fn center(&self) -> Vec3 {
        match self {
            Self::Part { bounds, .. } => (bounds[0] + bounds[1]) / 2.0,
            Self::Edge(edge) => edge.center(),
            Self::Face(face) => face.center,
            Self::Pair { first, second } => (first.center + second.center) * 0.5,
        }
    }

    pub fn planar(&self) -> bool {
        self.latest().is_some_and(|face| face.normal.is_some())
    }
}

pub fn faces(mesh: &Mesh) -> Result<BTreeMap<u32, Face>, String> {
    assert!(mesh.vertices.len() <= 300000);
    assert!(mesh.vertices.len().is_multiple_of(3));
    let triangles = mesh.vertices.as_chunks::<3>().0;
    let mut groups = BTreeMap::<u32, Vec<usize>>::new();
    for (index, triangle) in triangles.iter().enumerate() {
        if triangle[0].face_id == 0 {
            return Err("Inspection requires positive face IDs".into());
        }
        groups.entry(triangle[0].face_id).or_default().push(index);
    }
    let mut faces = BTreeMap::new();
    for (id, indices) in groups {
        let mut minimum = Vec3::splat(f32::INFINITY);
        let mut maximum = Vec3::splat(f32::NEG_INFINITY);
        let mut normal = None;
        let mut planar = true;
        let mut origin = Vec3::ZERO;
        let mut center = Vec3::ZERO;
        let mut area = 0.0;
        for (index, triangle_index) in indices.iter().enumerate() {
            let points = triangles[*triangle_index].map(|vertex| Vec3::from_array(vertex.position));
            let cross = (points[1] - points[0]).cross(points[2] - points[0]);
            let weight = cross.length();
            if !weight.is_finite() || weight <= 0.0 {
                return Err("Inspection found a degenerate triangle".into());
            }
            let direction = cross / weight;
            if index == 0 {
                normal = Some(direction);
                origin = points[0];
            }
            let reference = normal.expect("First triangle normal");
            if reference.dot(direction) < 0.999999 {
                planar = false;
            }
            for point in points {
                minimum = minimum.min(point);
                maximum = maximum.max(point);
                if (point - origin).dot(reference).abs() > TOLERANCE_MM {
                    planar = false;
                }
            }
            center += (points[0] + points[1] + points[2]) * (weight / 3.0);
            area += weight;
        }
        assert!(area > 0.0);
        center /= area;
        let mut spans = [None; 3];
        if planar {
            let normal = normal.expect("Planar face normal");
            let mut edges = BTreeMap::<([i64; 3], [i64; 3]), (u32, Vec3)>::new();
            for triangle_index in &indices {
                let points =
                    triangles[*triangle_index].map(|vertex| Vec3::from_array(vertex.position));
                for index in 0..3 {
                    let first = points[index];
                    let second = points[(index + 1) % 3];
                    let key = |point: Vec3| {
                        point
                            .to_array()
                            .map(|value| (f64::from(value) * 1000.0).round() as i64)
                    };
                    let (first_key, second_key) = (key(first), key(second));
                    let key = if first_key < second_key {
                        (first_key, second_key)
                    } else {
                        (second_key, first_key)
                    };
                    let edge = edges.entry(key).or_insert((0, second - first));
                    edge.0 += 1;
                }
            }
            let mut tangent = edges
                .values()
                .filter(|(count, _)| *count == 1)
                .map(|(_, direction)| *direction)
                .max_by(|first, second| first.length_squared().total_cmp(&second.length_squared()))
                .ok_or("Planar face has no inspection boundary")?
                .normalize();
            let dominant = (0..3)
                .max_by(|first, second| tangent[*first].abs().total_cmp(&tangent[*second].abs()))
                .expect("Three axes");
            if tangent[dominant] < 0.0 {
                tangent = -tangent;
            }
            let axes = [tangent, normal.cross(tangent).normalize()];
            let mut lower = [f32::INFINITY; 2];
            let mut upper = [f32::NEG_INFINITY; 2];
            for triangle_index in &indices {
                for vertex in triangles[*triangle_index] {
                    let point = Vec3::from_array(vertex.position) - origin;
                    for index in 0..2 {
                        let value = point.dot(axes[index]);
                        lower[index] = lower[index].min(value);
                        upper[index] = upper[index].max(value);
                    }
                }
            }
            let corner = origin + axes[0] * lower[0] + axes[1] * lower[1];
            for index in 0..2 {
                let distance_mm = upper[index] - lower[index];
                let direction = (0..3)
                    .find(|axis| axes[index][*axis].abs() > 0.999999)
                    .map_or(["Face span U", "Face span V"][index], |axis| {
                        ["Face width (X)", "Face depth (Y)", "Face height (Z)"][axis]
                    });
                spans[index] = Some(Span {
                    points: [corner, corner + axes[index] * distance_mm],
                    distance_mm,
                    direction,
                });
            }
        } else {
            normal = None;
            for index in 0..3 {
                let mut end = minimum;
                end[index] = maximum[index];
                let distance_mm = maximum[index] - minimum[index];
                if distance_mm > TOLERANCE_MM {
                    spans[index] = Some(Span {
                        points: [minimum, end],
                        distance_mm,
                        direction: ["Face X extent", "Face Y extent", "Face Z extent"][index],
                    });
                }
            }
        }
        faces.insert(
            id,
            Face {
                id,
                center,
                spans,
                normal,
            },
        );
    }
    assert!(faces.len() == mesh.face_count as usize);
    Ok(faces)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_edges_measure_lines_and_rotated_circles_without_seams() -> Result<(), String> {
        let kernel = crate::kernel::Kernel::new()?;
        let edges = kernel.box_shape([0.0; 3], [10.0, 20.0, 30.0])?.edges()?;
        assert_eq!(edges.items.len(), 12);
        for length in [10.0, 20.0, 30.0] {
            assert_eq!(
                edges
                    .items
                    .iter()
                    .filter(|edge| edge.spans()[0]
                        .is_some_and(|span| (span.distance_mm - length).abs() < TOLERANCE_MM))
                    .count(),
                4
            );
        }
        let mut cylinder = kernel.cylinder([0.0; 3], 3.0, 8.0)?;
        cylinder.rotate([1.0, 0.0, 0.0], 37.0)?;
        cylinder.translate([14.0, -25.0, 60.0])?;
        let edges = cylinder.edges()?;
        assert_eq!(edges.items.len(), 2);
        for edge in &edges.items {
            assert!(matches!(edge.geometry, EdgeGeometry::Circle { .. }));
            let [Some(radius), Some(diameter), None] = edge.spans() else {
                return Err("Missing circular dimensions".into());
            };
            assert!((radius.distance_mm - 3.0).abs() < TOLERANCE_MM);
            assert!((diameter.distance_mm - 6.0).abs() < TOLERANCE_MM);
            let selection = Selection::Edge(*edge);
            assert_eq!(selection.ids(), [0; 2]);
            assert!(selection.latest().is_none());
            assert!(!selection.planar());
        }
        let selection = Selection::Edge(edges.items[0]);
        let bounds = cylinder.bounds(true)?.map(|point| point.as_vec3());
        let camera = crate::viewport::Camera {
            orthographic: false,
            target: (bounds[0] + bounds[1]) * 0.5,
            distance: 40.0,
            yaw: -0.95,
            pitch: 0.55,
        };
        let output =
            eframe::egui::Context::default().run_ui(eframe::egui::RawInput::default(), |ui| {
                crate::dimensions::draw(
                    ui,
                    eframe::egui::Rect::from_min_size(
                        eframe::egui::Pos2::ZERO,
                        eframe::egui::vec2(600.0, 600.0),
                    ),
                    camera.matrix(1.0),
                    bounds,
                    &crate::dimensions::Annotations::default(),
                    Some(&selection),
                    |_| true,
                );
            });
        for expected in ["R ≈3.00 mm", "Ø ≈6.00 mm"] {
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, eframe::egui::Shape::Text(text) if text.galley.job.text == expected)));
        }
        output.drop_without_applying_deltas();
        Ok(())
    }

    #[test]
    fn partial_arcs_measure_the_supporting_circle_and_reject_ellipses_and_nonplanar_curves() {
        let samples = std::array::from_fn(|index| {
            let angle = index as f64 / 16.0 * 1.2;
            DVec3::new(10.0 + 4.0 * angle.cos(), -7.0 + 4.0 * angle.sin(), 3.0)
        });
        let edge = Edge::from_samples([0, 16], samples);
        assert!(matches!(edge.geometry, EdgeGeometry::Circle { .. }));
        assert!((edge.spans()[0].expect("Radius").distance_mm - 4.0).abs() < TOLERANCE_MM);
        for points in [
            samples.map(|point| point * DVec3::new(2.0, 1.0, 1.0)),
            std::array::from_fn(|index| samples[index] + DVec3::Z * index as f64 * 0.1),
            [DVec3::ZERO; 17],
        ] {
            let unsupported = Edge::from_samples([0, 16], points);
            assert!(matches!(unsupported.geometry, EdgeGeometry::Other { .. }));
            assert!(unsupported.spans().iter().all(Option::is_none));
        }
    }

    #[test]
    fn edge_picking_uses_screen_distance_and_rejects_occluded_edges() -> Result<(), String> {
        let snapshot = crate::build_script_snapshot(
            "--!strict\ncad.show('box',cad.box({width=40,depth=20,height=8}))",
            1,
        )?;
        let matrix = Mat4::orthographic_rh(-30.0, 30.0, -30.0, 30.0, 0.1, 100.0)
            * Mat4::look_at_rh(
                Vec3::new(20.0, 10.0, 60.0),
                Vec3::new(20.0, 10.0, 0.0),
                Vec3::Y,
            );
        let project = |point: Vec3| {
            let point = matrix.project_point3(point);
            [(point.x + 1.0) * 300.0, (1.0 - point.y) * 300.0]
        };
        let edges = snapshot.scene.world_edges();
        let top = edges.items.iter().find(|edge| matches!(edge.geometry, EdgeGeometry::Line(points) if points.iter().all(|point| point.z == 8.0) && (points[0].y-points[1].y).abs()<TOLERANCE_MM)).ok_or("Top edge")?;
        let position = project(top.center());
        let pick = |edges: &[Edge], position| {
            pick_edge(
                edges,
                &snapshot.scene.world_edges().segments,
                &snapshot.scene.world_mesh(),
                matrix,
                [600.0; 2],
                position,
            )
        };
        let selected =
            pick(&snapshot.scene.world_edges().items, position).ok_or("Visible edge not picked")?;
        assert!((selected.center().z - 8.0).abs() < TOLERANCE_MM);
        assert!(
            pick(
                &snapshot.scene.world_edges().items,
                [position[0], position[1] + 12.0]
            )
            .is_none()
        );
        assert!(
            pick(
                &snapshot.scene.world_edges().items,
                project(Vec3::new(20.0, 10.0, 8.0))
            )
            .is_none()
        );
        let hidden = snapshot.scene.world_edges().items.iter().copied().filter(|edge| matches!(edge.geometry, EdgeGeometry::Line(points) if points.iter().all(|point| point.z == 0.0))).collect::<Vec<_>>();
        assert_eq!(hidden.len(), 4);
        for edge in &hidden {
            assert!(pick(&hidden, project(edge.center())).is_none());
        }
        for distance in [100.0, 5000.0] {
            let matrix = Mat4::perspective_rh(40_f32.to_radians(), 1.0, 0.1, 10000.0)
                * Mat4::look_at_rh(
                    Vec3::new(20.0, 10.0, distance),
                    Vec3::new(20.0, 10.0, 0.0),
                    Vec3::Y,
                );
            for edge in &hidden {
                let point = matrix.project_point3(edge.center());
                assert!(
                    pick_edge(
                        &hidden,
                        &snapshot.scene.world_edges().segments,
                        &snapshot.scene.world_mesh(),
                        matrix,
                        [6000.0; 2],
                        [(point.x + 1.0) * 3000.0, (1.0 - point.y) * 3000.0]
                    )
                    .is_none()
                );
            }
        }
        Ok(())
    }

    #[test]
    fn rack_lips_have_local_eight_mm_spans_and_tread_separation() -> Result<(), String> {
        let snapshot = crate::build_script_snapshot(include_str!("../../models/rack.luau"), 1)?;
        let lip = snapshot
            .scene
            .faces
            .values()
            .find(|face| {
                face.normal
                    .is_some_and(|normal| normal.dot(Vec3::Y) > 0.999999)
                    && (face.center.y - 3.0).abs() < TOLERANCE_MM
                    && face
                        .spans
                        .iter()
                        .flatten()
                        .any(|span| (span.distance_mm - 8.0).abs() < TOLERANCE_MM)
            })
            .ok_or("Missing inner lip face with an 8 mm span")?;
        let top = snapshot
            .scene
            .faces
            .values()
            .find(|face| {
                face.normal.is_some_and(|normal| normal.z > 0.999999)
                    && (face.center.z - 11.0).abs() < TOLERANCE_MM
            })
            .ok_or("Missing lip top")?;
        let tread = snapshot
            .scene
            .faces
            .values()
            .find(|face| {
                face.normal.is_some_and(|normal| normal.z > 0.999999)
                    && (face.center.z - 3.0).abs() < TOLERANCE_MM
            })
            .ok_or("Missing first tread")?;
        let first = Selection::Face(*top);
        let pair = Selection::select(Some(&first), *tread, true)?;
        assert!((pair.spans()[0].expect("Separation").distance_mm - 8.0).abs() < TOLERANCE_MM);
        assert_eq!(pair.ids(), [top.id, tread.id]);
        let selected = Selection::Face(*lip);
        assert!(
            selected
                .spans()
                .iter()
                .flatten()
                .any(|span| span.direction == "Face height (Z)"
                    && (span.distance_mm - 8.0).abs() < TOLERANCE_MM)
        );
        let mut camera = crate::snapshot_camera(&snapshot, 1.0);
        camera.distance *= 2.0;
        let output =
            eframe::egui::Context::default().run_ui(eframe::egui::RawInput::default(), |ui| {
                let rect = eframe::egui::Rect::from_min_size(
                    eframe::egui::Pos2::ZERO,
                    eframe::egui::vec2(600.0, 600.0),
                );
                crate::dimensions::draw(
                    ui,
                    rect,
                    camera.matrix(1.0),
                    snapshot.scene.bounds,
                    &snapshot.scene.annotations,
                    Some(&selected),
                    |_| true,
                );
            });
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, eframe::egui::Shape::Text(text) if text.galley.job.text == "≈8.00 mm")));
        output.drop_without_applying_deltas();
        Ok(())
    }

    #[test]
    fn rotated_rectangles_use_boundary_lengths_instead_of_world_bounds() -> Result<(), String> {
        let source = "--!strict\nlocal part=cad.box({width=40,depth=20,height=8})\npart=cad.rotate(part,{axis='z',angle=37})\npart=cad.rotate(part,{axis='y',angle=23})\ncad.show('rotated',part)";
        let snapshot = crate::build_script_snapshot(source, 1)?;
        assert_eq!(snapshot.scene.faces.len(), 6);
        let mut dimensions = Vec::new();
        for face in snapshot.scene.faces.values() {
            assert!(face.normal.is_some());
            assert_eq!(face.spans.iter().flatten().count(), 2);
            for span in face.spans.iter().flatten() {
                let rounded = span.distance_mm.round();
                assert!((span.distance_mm - rounded).abs() < TOLERANCE_MM);
                assert!([8.0, 20.0, 40.0].contains(&rounded));
                dimensions.push(rounded as u32);
            }
        }
        for length in [8, 20, 40] {
            assert_eq!(
                dimensions.iter().filter(|value| **value == length).count(),
                4
            );
        }
        let first = *snapshot.scene.faces.values().next().expect("Box face");
        let other = *snapshot
            .scene
            .faces
            .values()
            .find(|face| {
                face.normal
                    .expect("Planar")
                    .dot(first.normal.expect("Planar"))
                    < -0.999999
            })
            .expect("Opposite box face");
        let pair = Selection::select(Some(&Selection::Face(first)), other, true)?;
        let distance = pair.spans()[0].expect("Parallel separation").distance_mm;
        assert!(
            [8.0, 20.0, 40.0]
                .iter()
                .any(|expected| (distance - expected).abs() < TOLERANCE_MM)
        );
        Ok(())
    }

    #[test]
    fn pair_selection_rejects_nonparallel_and_curved_faces_and_toggles_members()
    -> Result<(), String> {
        let snapshot = crate::build_script_snapshot(
            "--!strict\ncad.show('cylinder',cad.cylinder({radius=5,height=20}))",
            1,
        )?;
        let top = snapshot
            .scene
            .faces
            .values()
            .find(|face| face.normal.is_some_and(|normal| normal.z > 0.99))
            .ok_or("Top face")?;
        let bottom = snapshot
            .scene
            .faces
            .values()
            .find(|face| face.normal.is_some_and(|normal| normal.z < -0.99))
            .ok_or("Bottom face")?;
        let curved = snapshot
            .scene
            .faces
            .values()
            .find(|face| face.normal.is_none())
            .ok_or("Curved face")?;
        assert_eq!(curved.spans.iter().flatten().count(), 3);
        let first = Selection::Face(*top);
        assert!(Selection::select(Some(&first), *curved, true).is_err());
        assert_eq!(first.ids(), [top.id, 0]);
        let pair = Selection::select(Some(&first), *bottom, true)?;
        assert!((pair.spans()[0].expect("Distance").distance_mm - 20.0).abs() < TOLERANCE_MM);
        assert_eq!(
            Selection::select(Some(&pair), *bottom, true)?.ids(),
            [top.id, 0]
        );
        assert_eq!(
            Selection::select(Some(&pair), *top, true)?.ids(),
            [bottom.id, 0]
        );
        let mut perpendicular = *top;
        perpendicular.id += 100;
        perpendicular.normal = Some(Vec3::X);
        assert!(Selection::select(Some(&first), perpendicular, true).is_err());
        perpendicular.normal = Some(Vec3::new(0.0001, 0.0, 1.0).normalize());
        assert!(Selection::select(Some(&first), perpendicular, true).is_err());
        assert_eq!(first.ids(), [top.id, 0]);
        Ok(())
    }
}
