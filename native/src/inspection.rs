use crate::mesh::Mesh;
use glam::Vec3;
use std::collections::BTreeMap;

const TOLERANCE_MM: f32 = 0.001;

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

#[derive(Clone, Copy)]
pub enum Selection {
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
            Self::Face(face) => [face.id, 0],
            Self::Pair { first, second } => [first.id, second.id],
        }
    }

    pub fn latest(&self) -> &Face {
        match self {
            Self::Face(face) => face,
            Self::Pair { second, .. } => second,
        }
    }

    pub fn spans(&self) -> [Option<Span>; 3] {
        match self {
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
            Self::Face(face) => face.center,
            Self::Pair { first, second } => (first.center + second.center) * 0.5,
        }
    }

    pub fn planar(&self) -> bool {
        self.latest().normal.is_some()
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
    fn rack_lips_have_local_eight_mm_spans_and_tread_separation() -> Result<(), String> {
        let snapshot = crate::build_script_snapshot(include_str!("../../models/rack.luau"), 1)?;
        let lip = snapshot
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
            .faces
            .values()
            .find(|face| {
                face.normal.is_some_and(|normal| normal.z > 0.999999)
                    && (face.center.z - 11.0).abs() < TOLERANCE_MM
            })
            .ok_or("Missing lip top")?;
        let tread = snapshot
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
                    camera.eye(),
                    snapshot.bounds,
                    &snapshot.annotations,
                    Some(&selected),
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
        assert_eq!(snapshot.faces.len(), 6);
        let mut dimensions = Vec::new();
        for face in snapshot.faces.values() {
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
        let first = *snapshot.faces.values().next().expect("Box face");
        let other = *snapshot
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
            .faces
            .values()
            .find(|face| face.normal.is_some_and(|normal| normal.z > 0.99))
            .ok_or("Top face")?;
        let bottom = snapshot
            .faces
            .values()
            .find(|face| face.normal.is_some_and(|normal| normal.z < -0.99))
            .ok_or("Bottom face")?;
        let curved = snapshot
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
