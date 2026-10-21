use crate::{inspection::Edge, mesh::Hit, model, viewport::Instance};
#[cfg(test)]
use crate::{inspection::Edges, mesh::Mesh};
use glam::{Mat4, Vec3};
use std::{
    ops::{Range, RangeInclusive},
    sync::Arc,
};

pub struct Part {
    pub geometry: Arc<model::Geometry>,
    pub faces: RangeInclusive<u32>,
    pub segments: Range<u32>,
    pub mesh_bounds: [Vec3; 2],
}

pub struct View<'a> {
    pub parts: &'a [Part],
    pub instances: &'a [Instance],
    pub visible_parts: u32,
}

impl View<'_> {
    pub fn face_visible(&self, id: u32) -> bool {
        self.parts
            .iter()
            .position(|part| part.faces.contains(&id))
            .is_some_and(|index| self.visible_parts & (1 << index) != 0)
    }

    pub fn selection_part(&self, selection: &crate::inspection::Selection) -> Option<usize> {
        match selection {
            crate::inspection::Selection::Part { index, .. } => {
                ((*index as usize) < self.parts.len()).then_some(*index as usize)
            }
            crate::inspection::Selection::Edge(edge) => {
                self.source_edge(edge.segments).map(|(index, _)| index)
            }
            _ => {
                let id = selection.latest()?.id;
                self.parts.iter().position(|part| part.faces.contains(&id))
            }
        }
    }

    pub fn selection_visible(&self, selection: &crate::inspection::Selection) -> bool {
        if let crate::inspection::Selection::Pair { first, second } = selection {
            return self.face_visible(first.id) && self.face_visible(second.id);
        }
        self.selection_part(selection)
            .is_some_and(|index| self.visible_parts & (1 << index) != 0)
    }

    pub fn pick(&self, matrix: Mat4, position: [f32; 2]) -> Option<Hit> {
        assert_eq!(self.parts.len(), self.instances.len());
        assert!(self.parts.len() <= 32);
        let inverse = matrix.inverse();
        let origin = inverse.project_point3(Vec3::new(position[0], position[1], 0.0));
        let direction =
            (inverse.project_point3(Vec3::new(position[0], position[1], 1.0)) - origin).normalize();
        if !origin.is_finite() || !direction.is_finite() {
            return None;
        }
        let mut distance = f32::INFINITY;
        let mut selected = None;
        for (index, (part, instance)) in self.parts.iter().zip(self.instances).enumerate() {
            if self.visible_parts & (1 << index) == 0 {
                continue;
            }
            let placement = instance.matrix();
            let local = placement.inverse();
            let local_origin = local.transform_point3(origin);
            let local_direction = local.transform_vector3(direction);
            if !ray_bounds(local_origin, local_direction, part.mesh_bounds) {
                continue;
            }
            if let Some(mut hit) = part.geometry.mesh.ray_hit(local_origin, local_direction) {
                hit.position = placement.transform_point3(hit.position);
                let along = (hit.position - origin).dot(direction);
                if along < distance {
                    distance = along;
                    hit.face_id += part.faces.start() - 1;
                    selected = Some(hit);
                }
            }
        }
        selected
    }

    pub fn source_edge(&self, range: [u32; 2]) -> Option<(usize, &Edge)> {
        for (index, part) in self.parts.iter().enumerate() {
            if part.segments.contains(&range[0]) && range[1] <= part.segments.end {
                let local = range.map(|index| index - part.segments.start);
                return part
                    .geometry
                    .edges
                    .items
                    .iter()
                    .find(|edge| edge.segments == local)
                    .map(|edge| (index, edge));
            }
        }
        None
    }

    pub fn edge(&self, range: [u32; 2]) -> Option<Edge> {
        let (index, edge) = self.source_edge(range)?;
        let mut edge = edge.transformed(self.instances[index].matrix());
        edge.segments = range;
        Some(edge)
    }

    pub fn pick_edge(&self, matrix: Mat4, size: [f32; 2], position: [f32; 2]) -> Option<Edge> {
        assert_eq!(self.parts.len(), self.instances.len());
        assert!(self.parts.len() <= 32);
        assert!(size.iter().all(|value| value.is_finite() && *value > 0.0));
        let inverse = matrix.inverse();
        let view_direction =
            (inverse.project_point3(Vec3::Z) - inverse.project_point3(Vec3::ZERO)).normalize();
        let cursor = glam::Vec2::from_array(position);
        let mut closest = 6.0_f32;
        let mut depth = f32::INFINITY;
        let mut selected = None;
        for (index, (part, instance)) in self.parts.iter().zip(self.instances).enumerate() {
            if self.visible_parts & (1 << index) == 0 {
                continue;
            }
            let placement = instance.matrix();
            let projection = matrix * placement;
            for edge in &part.geometry.edges.items {
                for segment in &part.geometry.edges.segments
                    [edge.segments[0] as usize..edge.segments[1] as usize]
                {
                    let start = Vec3::from_array(segment.start);
                    let end = Vec3::from_array(segment.end);
                    let (Some(first), Some(second)) = (
                        project(projection, size, start),
                        project(projection, size, end),
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
                    if let Some(hit) = self.pick(
                        matrix,
                        [point.x / size[0] * 2.0 - 1.0, 1.0 - point.y / size[1] * 2.0],
                    ) {
                        let start_w = (projection * start.extend(1.0)).w;
                        let end_w = (projection * end.extend(1.0)).w;
                        let along =
                            fraction * start_w / (end_w * (1.0 - fraction) + start_w * fraction);
                        let world = placement.transform_point3(start.lerp(end, along));
                        if (world - hit.position).dot(view_direction) > 0.02 {
                            continue;
                        }
                    }
                    closest = distance;
                    depth = point.z;
                    let mut world = edge.transformed(placement);
                    world.segments = world.segments.map(|index| index + part.segments.start);
                    selected = Some(world);
                }
            }
        }
        selected
    }

    #[cfg(test)]
    pub fn world_mesh(&self) -> Mesh {
        let meshes: Vec<_> = self
            .parts
            .iter()
            .zip(self.instances)
            .map(|(part, instance)| {
                part.geometry
                    .mesh
                    .placed(instance.matrix(), instance.color())
            })
            .collect();
        Mesh::combine(&meshes.iter().collect::<Vec<_>>())
            .expect("Scene budgets checked")
            .0
    }

    #[cfg(test)]
    pub fn world_edges(&self) -> Edges {
        let mut result = Edges {
            items: Vec::new(),
            segments: Vec::new(),
        };
        for (part, instance) in self.parts.iter().zip(self.instances) {
            result
                .items
                .extend(part.geometry.edges.items.iter().map(|edge| {
                    self.edge(edge.segments.map(|index| index + part.segments.start))
                        .expect("Canonical edge")
                }));
            result
                .segments
                .extend(part.geometry.edges.segments.iter().map(|segment| {
                    crate::mesh::EdgeSegment {
                        start: instance
                            .matrix()
                            .transform_point3(Vec3::from_array(segment.start))
                            .to_array(),
                        end: instance
                            .matrix()
                            .transform_point3(Vec3::from_array(segment.end))
                            .to_array(),
                    }
                }));
        }
        result
    }
}

fn project(matrix: Mat4, size: [f32; 2], point: Vec3) -> Option<Vec3> {
    let clip = matrix * point.extend(1.0);
    if clip.w <= 0.1 || !(0.0..=clip.w).contains(&clip.z) {
        return None;
    }
    let point = clip.truncate() / clip.w;
    Some(Vec3::new(
        (point.x + 1.0) * size[0] * 0.5,
        (1.0 - point.y) * size[1] * 0.5,
        point.z,
    ))
}

fn ray_bounds(origin: Vec3, direction: Vec3, bounds: [Vec3; 2]) -> bool {
    let padding = 0.001
        + (bounds[1] - bounds[0]).max_element() * 2e-5
        + bounds[0].abs().max(bounds[1].abs()).max_element() * f32::EPSILON * 4.0;
    let mut near = 0.0_f32;
    let mut far = f32::INFINITY;
    for axis in 0..3 {
        let minimum = bounds[0][axis] - padding;
        let maximum = bounds[1][axis] + padding;
        if direction[axis] == 0.0 {
            if origin[axis] < minimum || origin[axis] > maximum {
                return false;
            }
        } else {
            let first = (minimum - origin[axis]) / direction[axis];
            let second = (maximum - origin[axis]) / direction[axis];
            near = near.max(first.min(second));
            far = far.min(first.max(second));
            if far < near {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_parts_do_not_occlude_or_receive_face_and_edge_picks() -> Result<(), String> {
        let snapshot = crate::build_script_snapshot(
            "cad.show('front',cad.box({width=10,depth=10,height=10}),{assembly={position={y=-20}}});cad.show('rear',cad.box({width=10,depth=10,height=10}))",
            1,
        )?;
        let matrix = Mat4::perspective_rh(40_f32.to_radians(), 1.0, 0.1, 10000.0)
            * Mat4::look_at_rh(
                Vec3::new(5.0, -100.0, 5.0),
                Vec3::new(5.0, 5.0, 5.0),
                Vec3::Z,
            );
        let mut view = snapshot.scene.picking();
        let front = view.pick(matrix, [0.0; 2]).expect("Front hit");
        assert!(view.parts[0].faces.contains(&front.face_id));
        view.visible_parts = 2;
        let rear = view
            .pick(matrix, [0.0; 2])
            .expect("Visible rear hit through hidden front");
        assert!(view.parts[1].faces.contains(&rear.face_id));
        let clip = matrix.project_point3(Vec3::new(0.0, 0.0, 5.0));
        let pixel = [(clip.x + 1.0) * 500.0, (1.0 - clip.y) * 500.0];
        let edge = view
            .pick_edge(matrix, [1000.0; 2], pixel)
            .expect("Visible rear edge");
        assert!(view.parts[1].segments.contains(&edge.segments[0]));
        assert!(!view.face_visible(front.face_id));
        assert!(view.face_visible(rear.face_id));
        let front_face = *snapshot
            .scene
            .faces
            .get(&front.face_id)
            .expect("Front face");
        let rear_face = *snapshot.scene.faces.get(&rear.face_id).expect("Rear face");
        assert!(!view.selection_visible(&crate::inspection::Selection::Face(front_face)));
        assert!(view.selection_visible(&crate::inspection::Selection::Face(rear_face)));
        assert!(
            !view.selection_visible(&crate::inspection::Selection::Pair {
                first: front_face,
                second: rear_face
            })
        );
        assert!(view.selection_visible(&crate::inspection::Selection::Edge(edge)));
        assert_eq!(
            snapshot.scene.visible_bounds(2),
            Some(snapshot.scene.part_bounds[1])
        );
        view.visible_parts = 0;
        assert!(view.pick(matrix, [0.0; 2]).is_none());
        assert!(view.pick_edge(matrix, [1000.0; 2], pixel).is_none());
        assert!(snapshot.scene.visible_bounds(0).is_none());
        Ok(())
    }

    const SOURCE: &str = "local a=cad.box({width=30,depth=20,height=10});local b=cad.cylinder({radius=8,height=35});cad.show('box',a,{assembly={rotation={x=15,z=25},position={x=20,y=-10}},print={rotation={y=90},position={x=20,z=30}}});cad.show('cylinder',b,{assembly={rotation={y=35},position={x=30,y=5}},print={rotation={x=90},position={x=55,z=8}}});cad.show('occluder',cad.box({width=40,depth=40,height=5}),{assembly={position={x=10,y=-20,z=45}},print={position={x=10,y=30}}})";

    #[test]
    fn local_queries_match_world_mesh_and_global_occlusion_across_layouts() -> Result<(), String> {
        let mut snapshot = crate::build_script_snapshot(SOURCE, 1)?;
        let mut hits = 0;
        let mut edges = 0;
        for layout in [
            crate::placement::Layout::Assembly,
            crate::placement::Layout::Print,
        ] {
            snapshot.set_layout(layout);
            let scene = &snapshot.scene;
            let world = scene.world_mesh();
            let sampled = scene.world_edges();
            let center = (scene.bounds[0] + scene.bounds[1]) * 0.5;
            for scale in [0.25, 1.0, 25.0] {
                let matrix = Mat4::perspective_rh(40_f32.to_radians(), 1.25, 0.1, 10000.0)
                    * Mat4::look_at_rh(
                        center + Vec3::new(90.0, -140.0, 120.0) * scale,
                        center,
                        Vec3::Z,
                    );
                for y in 0..12 {
                    for x in 0..16 {
                        let ndc = [(x as f32 + 0.37) / 8.0 - 1.0, (y as f32 + 0.23) / 6.0 - 1.0];
                        let expected = world.pick(matrix, ndc);
                        let actual = scene.picking().pick(matrix, ndc);
                        assert_eq!(
                            actual.as_ref().map(|hit| hit.face_id),
                            expected.as_ref().map(|hit| hit.face_id)
                        );
                        if let (Some(actual), Some(expected)) = (actual, expected) {
                            hits += 1;
                            assert!(actual.position.distance(expected.position) < 0.03);
                        }
                        let pixel = [(ndc[0] + 1.0) * 500.0, (1.0 - ndc[1]) * 400.0];
                        let expected = crate::inspection::pick_edge(
                            &sampled.items,
                            &sampled.segments,
                            &world,
                            matrix,
                            [1000.0, 800.0],
                            pixel,
                        );
                        let actual = scene.picking().pick_edge(matrix, [1000.0, 800.0], pixel);
                        assert_eq!(
                            actual.map(|edge| edge.segments),
                            expected.map(|edge| edge.segments)
                        );
                        edges += u32::from(actual.is_some());
                    }
                }
            }
        }
        assert!(hits > 10);
        assert!(edges > 10);
        assert!(!ray_bounds(
            Vec3::new(2.0, 0.5, 0.5),
            Vec3::Z,
            [Vec3::ZERO, Vec3::ONE]
        ));
        assert!(ray_bounds(
            Vec3::splat(0.5),
            Vec3::X,
            [Vec3::ZERO, Vec3::ONE]
        ));
        Ok(())
    }

    #[test]
    fn repeated_face_and_edge_queries_allocate_no_rust_heap_memory() -> Result<(), String> {
        let snapshot = crate::build_script_snapshot(SOURCE, 1)?;
        let camera = crate::snapshot_camera(&snapshot, 1.25);
        let matrix = camera.matrix(1.25);
        let (_, counts) = crate::allocations::measure(|| {
            for index in 0..512 {
                let x = (index % 32) as f32 / 16.0 - 1.0;
                let y = (index / 32) as f32 / 8.0 - 1.0;
                std::hint::black_box(snapshot.scene.picking().pick(matrix, [x, y]));
                std::hint::black_box(snapshot.scene.picking().pick_edge(
                    matrix,
                    [1000.0, 800.0],
                    [(x + 1.0) * 500.0, (1.0 - y) * 400.0],
                ));
            }
        });
        assert_eq!(counts, crate::allocations::Counts::default());
        eprintln!(
            "Canonical picking: 1024 queries, {} Rust allocation calls, {} requested bytes",
            counts.calls, counts.bytes
        );
        Ok(())
    }
}
