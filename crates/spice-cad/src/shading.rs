use crate::mesh::Mesh;
use glam::Vec3;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Shading {
    #[default]
    Cad,
    Flat,
    Smooth {
        crease_angle_deg: f32,
    },
}

impl Shading {
    pub fn from_table(options: mlua::Table) -> mlua::Result<Self> {
        crate::model::options_fields(&options, &["mode", "crease_angle"])?;
        let mode: String = options.get("mode")?;
        let angle: Option<f64> = options.get("crease_angle")?;
        match mode.as_str() {
            "cad" | "flat" => {
                if angle.is_some() {
                    return Err(mlua::Error::runtime("crease_angle requires smooth shading"));
                }
                Ok(if mode == "cad" { Self::Cad } else { Self::Flat })
            }
            "smooth" => {
                let angle = angle.unwrap_or(60.0);
                if !angle.is_finite() || !(0.0..=180.0).contains(&angle) {
                    return Err(mlua::Error::runtime(
                        "crease_angle must be finite and within 0–180 degrees",
                    ));
                }
                Ok(Self::Smooth {
                    crease_angle_deg: angle as f32,
                })
            }
            _ => Err(mlua::Error::runtime(
                "Shading mode must be cad, flat, or smooth",
            )),
        }
    }

    pub fn apply(self, mesh: &mut Mesh) {
        assert!(mesh.vertices.len() <= 300000);
        assert!(mesh.vertices.len().is_multiple_of(3));
        if self == Self::Cad {
            mesh.smooth_normals();
            return;
        }
        if self == Self::Flat {
            for triangle in mesh.vertices.as_chunks_mut::<3>().0 {
                let points = triangle.map(|vertex| Vec3::from_array(vertex.position));
                let normal = (points[1] - points[0])
                    .cross(points[2] - points[0])
                    .normalize_or_zero()
                    .to_array();
                for vertex in triangle {
                    vertex.normal = normal;
                }
            }
            return;
        }
        let normals: Vec<Vec3> = mesh
            .vertices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|triangle| {
                let points = triangle.map(|vertex| Vec3::from_array(vertex.position));
                (points[1] - points[0]).cross(points[2] - points[0])
            })
            .collect();
        for (triangle, normal) in mesh
            .vertices
            .as_chunks_mut::<3>()
            .0
            .iter_mut()
            .zip(&normals)
        {
            let normal = normal.normalize_or_zero().to_array();
            for vertex in triangle {
                vertex.normal = normal;
            }
        }
        let Self::Smooth { crease_angle_deg } = self else {
            return;
        };
        assert!((0.0..=180.0).contains(&crease_angle_deg));
        let mut edges = Vec::with_capacity(mesh.vertices.len());
        for (index, triangle) in mesh.vertices.as_chunks::<3>().0.iter().enumerate() {
            for first in 0..3 {
                let second = (first + 1) % 3;
                let mut points = [first, second].map(|corner| {
                    triangle[corner]
                        .position
                        .map(|value| if value == 0.0 { 0 } else { value.to_bits() })
                });
                let mut corners = [first, second].map(|corner| (index * 3 + corner) as u32);
                if points[0] > points[1] {
                    points.swap(0, 1);
                    corners.swap(0, 1);
                }
                edges.push((points, corners));
            }
        }
        edges.sort_unstable_by_key(|edge| edge.0);
        let mut groups: Vec<(u32, u32)> = (0..mesh.vertices.len() as u32)
            .map(|index| (index, 1))
            .collect();
        let threshold = crease_angle_deg.to_radians().cos();
        let mut start = 0;
        while start < edges.len() {
            let mut end = start + 1;
            while end < edges.len() && edges[end].0 == edges[start].0 {
                end += 1;
            }
            if end - start == 2 {
                let first = edges[start].1;
                let second = edges[start + 1].1;
                let a = normals[first[0] as usize / 3].normalize_or_zero();
                let b = normals[second[0] as usize / 3].normalize_or_zero();
                if a.length_squared() > 0.5 && b.length_squared() > 0.5 && a.dot(b) >= threshold {
                    for corner in 0..2 {
                        let mut a = shading_root(&mut groups, first[corner]);
                        let mut b = shading_root(&mut groups, second[corner]);
                        if a != b {
                            if groups[a as usize].1 < groups[b as usize].1 {
                                std::mem::swap(&mut a, &mut b);
                            }
                            groups[b as usize].0 = a;
                            groups[a as usize].1 += groups[b as usize].1;
                        }
                    }
                }
            }
            start = end;
        }
        let mut sums = vec![Vec3::ZERO; mesh.vertices.len()];
        for index in 0..mesh.vertices.len() {
            let root = shading_root(&mut groups, index as u32);
            sums[root as usize] += normals[index / 3];
        }
        for (index, vertex) in mesh.vertices.iter_mut().enumerate() {
            let normal = sums[shading_root(&mut groups, index as u32) as usize].normalize_or_zero();
            if normal.length_squared() > 0.5 {
                vertex.normal = normal.to_array();
            }
        }
    }
}

fn shading_root(groups: &mut [(u32, u32)], mut index: u32) -> u32 {
    assert!((index as usize) < groups.len());
    for _ in 0..groups.len() {
        let parent = groups[index as usize].0;
        if parent == index {
            return index;
        }
        let grandparent = groups[parent as usize].0;
        groups[index as usize].0 = grandparent;
        index = grandparent;
    }
    unreachable!("Shading groups contain a cycle")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corner_mesh() -> Mesh {
        let mut vertices = Vec::new();
        for (face_id, points) in [
            (1, [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]),
            (2, [[1.0, 0.0, 0.0], [0.0; 3], [0.0, 0.0, 1.0]]),
            (3, [[0.0; 3], [-1.0, 0.0, 0.0], [0.0, -1.0, 0.0]]),
        ] {
            vertices.extend(points.map(|position| crate::mesh::Vertex {
                position,
                normal: [0.0, 0.0, 1.0],
                face_id,
                color: [255; 4],
            }));
        }
        Mesh {
            vertices,
            face_count: 3,
        }
    }

    #[test]
    fn crease_smoothing_requires_shared_edges_and_respects_angle() {
        let original = corner_mesh();
        let mut mesh = original.clone();
        Shading::Smooth {
            crease_angle_deg: 89.0,
        }
        .apply(&mut mesh);
        assert_eq!(mesh.vertices[0].normal, [0.0, 0.0, 1.0]);
        assert_eq!(mesh.vertices[4].normal, [0.0, 1.0, 0.0]);
        Shading::Smooth {
            crease_angle_deg: 91.0,
        }
        .apply(&mut mesh);
        let expected = Vec3::new(0.0, 1.0, 1.0).normalize();
        assert!(Vec3::from_array(mesh.vertices[0].normal).distance(expected) < 1e-6);
        assert_eq!(mesh.vertices[0].normal, mesh.vertices[4].normal);
        assert_eq!(mesh.vertices[6].normal, [0.0, 0.0, 1.0]);
        Shading::Flat.apply(&mut mesh);
        assert_eq!(mesh.vertices[0].normal, [0.0, 0.0, 1.0]);
        assert_eq!(mesh.vertices[4].normal, [0.0, 1.0, 0.0]);
        for (before, after) in original.vertices.iter().zip(&mesh.vertices) {
            assert_eq!(before.position, after.position);
            assert_eq!(before.face_id, after.face_id);
            assert_eq!(before.color, after.color);
        }
    }

    #[test]
    fn shading_rejects_invalid_modes_angles_and_unknown_options() {
        for options in [
            "{mode='invalid'}",
            "{mode='smooth',crease_angle=-1}",
            "{mode='smooth',crease_angle=181}",
            "{mode='smooth',crease_angle=0/0}",
            "{mode='smooth',crease_angle=1/0}",
            "{mode='flat',crease_angle=30}",
            "{mode='cad',crease_angle=30}",
            "{mode='smooth',angle=30}",
            "{}",
        ] {
            let source = format!(
                "cad.show('box',cad.box({{width=10,depth=10,height=10}}),{{shading={options}}})"
            );
            assert!(crate::model::compile(&source).is_err(), "{options}");
        }
        for mode in ["cad", "flat", "smooth"] {
            let source = format!(
                "cad.show('box',cad.box({{width=10,depth=10,height=10}}),{{shading={{mode='{mode}'}}}})"
            );
            assert!(crate::model::compile(&source).is_ok(), "{mode}");
        }
    }

    #[test]
    fn smoothing_handles_empty_degenerate_and_nonmanifold_edges() {
        let mut empty = Mesh {
            vertices: Vec::new(),
            face_count: 0,
        };
        Shading::Smooth {
            crease_angle_deg: 180.0,
        }
        .apply(&mut empty);
        assert!(empty.vertices.is_empty());
        let mut mesh = corner_mesh();
        mesh.vertices.extend_from_within(0..3);
        Shading::Smooth {
            crease_angle_deg: 180.0,
        }
        .apply(&mut mesh);
        assert_eq!(mesh.vertices[0].normal, [0.0, 0.0, 1.0]);
        assert_eq!(mesh.vertices[4].normal, [0.0, 1.0, 0.0]);
        mesh.vertices.truncate(3);
        for vertex in &mut mesh.vertices {
            vertex.position = [0.0; 3];
        }
        Shading::Smooth {
            crease_angle_deg: 180.0,
        }
        .apply(&mut mesh);
        assert!(mesh.vertices.iter().all(|vertex| vertex.normal == [0.0; 3]));
    }

    #[test]
    fn shading_edits_reuse_bodies_without_tessellation_and_preserve_measurements()
    -> Result<(), String> {
        use std::sync::Arc;
        let kernel = crate::kernel::Kernel::new()?;
        let mut cache = crate::model::Cache::new(&kernel);
        let source = "local a=cad.box({width=10,depth=20,height=30});local bottom=cad.face(a,{name='bottom',normal='z-'});local top=cad.face(a,{name='top',normal='z+'});cad.measure('height',{from=bottom,to=top});cad.show('box',a,OPTIONS)";
        let first = crate::model::compile(&source.replace("OPTIONS", "{}"))?;
        let original = cache.prepare(&first)?;
        let geometry = original.geometry[0].clone();
        let expected = cache.annotations(&first)?.measurements[0].distance_mm;
        for options in [
            "{shading={mode='flat'}}",
            "{shading={mode='smooth',crease_angle=100}}",
            "{shading={mode='smooth',crease_angle=20}}",
            "{shading={mode='cad'}}",
        ] {
            let model = crate::model::compile(&source.replace("OPTIONS", options))?;
            let prepared = cache.prepare(&model)?;
            assert_eq!(prepared.hits, 1);
            assert_eq!(prepared.build_ms, 0.0);
            assert_eq!(prepared.tessellation_ms, 0.0);
            assert!(std::rc::Rc::ptr_eq(
                &original.bodies[0],
                &prepared.bodies[0]
            ));
            assert!(!Arc::ptr_eq(&geometry, &prepared.geometry[0]));
            assert_eq!(prepared.geometry[0].volume_mm3, geometry.volume_mm3);
            assert_eq!(
                cache.annotations(&model)?.measurements[0].distance_mm,
                expected
            );
            for (before, after) in geometry
                .mesh
                .vertices
                .iter()
                .zip(&prepared.geometry[0].mesh.vertices)
            {
                assert_eq!(before.position, after.position);
                assert_eq!(before.face_id, after.face_id);
                if model.parts[0].shading == Shading::Cad {
                    assert_eq!(before.normal, after.normal);
                }
            }
            let repeated = cache.prepare(&model)?;
            assert!(Arc::ptr_eq(&prepared.geometry[0], &repeated.geometry[0]));
        }
        Ok(())
    }

    #[test]
    fn cylinder_shading_matches_preview_and_glb_without_rounding_caps() -> Result<(), String> {
        let kernel = crate::kernel::Kernel::new()?;
        for mode in ["flat", "smooth"] {
            let source = format!(
                "cad.show('cylinder',cad.cylinder({{radius=15,height=30}}),{{shading={{mode='{mode}'}}}})"
            );
            let model = crate::model::compile(&source)?;
            let meshes = model.meshes(&mut model.build(&kernel)?, true)?;
            let mesh = &meshes[0];
            mesh.validate()?;
            let bytes = crate::game_asset::glb(&model.parts, &meshes)?;
            let word =
                |index| u32::from_le_bytes(bytes[index..index + 4].try_into().unwrap()) as usize;
            let json_size = word(12);
            let json: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_size])
                .map_err(|error| error.to_string())?;
            let binary = &bytes[28 + json_size..];
            let indices = json["bufferViews"][1]["byteOffset"].as_u64().unwrap() as usize;
            let mut smooth_count = 0;
            for (index, vertex) in mesh.vertices.iter().enumerate() {
                let offset = indices + index * 4;
                let offset = u32::from_le_bytes(binary[offset..offset + 4].try_into().unwrap())
                    as usize
                    * 24
                    + 12;
                let normal = Vec3::from_array(std::array::from_fn(|component| {
                    f32::from_le_bytes(
                        binary[offset + component * 4..offset + component * 4 + 4]
                            .try_into()
                            .unwrap(),
                    )
                }));
                let [x, y, z] = vertex.normal;
                assert!(normal.distance(Vec3::new(x, z, -y)) < 1e-5);
                assert!((normal.length() - 1.0).abs() < 1e-5);
                let triangle = &mesh.vertices[index / 3 * 3..index / 3 * 3 + 3];
                let points: [Vec3; 3] =
                    std::array::from_fn(|corner| Vec3::from_array(triangle[corner].position));
                let face_normal = (points[1] - points[0])
                    .cross(points[2] - points[0])
                    .normalize();
                let difference = face_normal.distance(Vec3::from_array(vertex.normal));
                if face_normal.z.abs() > 0.99 || mode == "flat" {
                    assert!(difference < 1e-5);
                }
                if difference > 0.01 {
                    smooth_count += 1;
                }
            }
            if mode == "smooth" {
                assert!(smooth_count > 30);
            } else {
                assert_eq!(smooth_count, 0);
            }
        }
        Ok(())
    }

    #[test]
    fn maximum_budget_shading_uses_bounded_scratch_and_flat_shading_allocates_nothing() {
        let mut mesh = Mesh {
            vertices: Vec::with_capacity(300000),
            face_count: 100000,
        };
        for index in 0..100000 {
            let x = index as f32;
            mesh.vertices
                .extend(
                    [[x, 0.0, 0.0], [x + 1.0, 0.0, 0.0], [x, 1.0, 0.0]].map(|position| {
                        crate::mesh::Vertex {
                            position,
                            normal: [0.0; 3],
                            face_id: index + 1,
                            color: [255; 4],
                        }
                    }),
                );
        }
        let (_, flat) = crate::allocations::measure(|| Shading::Flat.apply(&mut mesh));
        assert_eq!(flat.calls, 0);
        assert_eq!(flat.bytes, 0);
        let started = std::time::Instant::now();
        let (_, smooth) = crate::allocations::measure(|| {
            Shading::Smooth {
                crease_angle_deg: 60.0,
            }
            .apply(&mut mesh)
        });
        assert_eq!(smooth.calls, 4);
        assert!(smooth.bytes <= 300000 * 60);
        assert!(
            mesh.vertices
                .iter()
                .all(|vertex| vertex.normal == [0.0, 0.0, 1.0])
        );
        eprintln!(
            "100000-triangle smoothing: {:?}, {} scratch allocations, {} bytes",
            started.elapsed(),
            smooth.calls,
            smooth.bytes
        );
    }

    #[test]
    fn bevel_example_stays_closed_and_low_poly_across_slider_limits() -> Result<(), String> {
        let kernel = crate::kernel::Kernel::new()?;
        let source = include_str!("../../../examples/bevels-and-shading.luau");
        let mut previous_volume = f64::INFINITY;
        for width in [0, 1, 4] {
            let source = source.replace("cad.number(1,", &format!("cad.number({width},"));
            let model = crate::model::compile(&source)?;
            let mut bodies = model.build(&kernel)?;
            assert_eq!(bodies.len(), 3);
            let volume = bodies[0].volume()?;
            assert!(volume < previous_volume);
            previous_volume = volume;
            let meshes = model.meshes(&mut bodies, true)?;
            let triangles: usize = meshes.iter().map(|mesh| mesh.vertices.len() / 3).sum();
            assert!(triangles <= 500, "width {width}: {triangles} triangles");
            for mesh in &meshes {
                mesh.validate()?;
            }
            assert!(!crate::game_asset::glb(&model.parts, &meshes)?.is_empty());
            eprintln!("Bevel width {width}: {triangles} triangles, block volume {volume} mm³");
        }
        Ok(())
    }
}
