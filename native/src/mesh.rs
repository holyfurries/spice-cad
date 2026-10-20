use glam::{Mat4, Vec3};

pub const DEFAULT_COLOR: [f32; 3] = [0.70, 0.73, 0.78];

pub fn vertex_color(color: [f32; 3]) -> [u8; 4] {
    assert!(color.iter().all(|value| value.is_finite()));
    assert!(color.iter().all(|value| (0.0..=1.0).contains(value)));
    let [r, g, b] = color.map(|value| (value * 255.0).round() as u8);
    [r, g, b, 255]
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct EdgeSegment {
    pub start: [f32; 3],
    pub end: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub face_id: u32,
    pub color: [u8; 4],
}

const _: () = assert!(std::mem::size_of::<Vertex>() == 32);

pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub face_count: u32,
}

#[derive(Debug, PartialEq)]
pub struct Hit {
    pub face_id: u32,
    pub position: Vec3,
}

impl Mesh {
    pub fn placed(&self, matrix: Mat4, color: [f32; 3]) -> Self {
        let color = vertex_color(color);
        Self {
            face_count: self.face_count,
            vertices: self
                .vertices
                .iter()
                .map(|vertex| Vertex {
                    position: matrix
                        .transform_point3(Vec3::from_array(vertex.position))
                        .to_array(),
                    normal: matrix
                        .transform_vector3(Vec3::from_array(vertex.normal))
                        .normalize_or_zero()
                        .to_array(),
                    color,
                    ..*vertex
                })
                .collect(),
        }
    }

    #[cfg(test)]
    pub fn combine(meshes: &[&Self]) -> Result<(Self, Vec<std::ops::RangeInclusive<u32>>), String> {
        let count: usize = meshes.iter().map(|mesh| mesh.vertices.len()).sum();
        if meshes.is_empty() || meshes.len() > 32 || count > 300000 {
            return Err("Assembly exceeds its part or triangle budget".into());
        }
        let mut combined = Self {
            vertices: Vec::with_capacity(count),
            face_count: 0,
        };
        let mut ranges = Vec::with_capacity(meshes.len());
        for mesh in meshes {
            assert!(mesh.face_count > 0);
            let offset = combined.face_count;
            ranges.push(offset + 1..=offset + mesh.face_count);
            combined
                .vertices
                .extend(mesh.vertices.iter().map(|vertex| Vertex {
                    face_id: vertex.face_id + offset,
                    ..*vertex
                }));
            combined.face_count += mesh.face_count;
        }
        assert_eq!(combined.vertices.len(), count);
        Ok((combined, ranges))
    }
    pub fn smooth_normals(&mut self) {
        assert!(self.vertices.len() <= 300000);
        assert!(self.vertices.len().is_multiple_of(3));
        assert!(self.face_count <= 100000);
        let capacity =
            (self.vertices.len() / 3 + self.face_count as usize * 2).min(self.vertices.len());
        let mut normals = std::collections::HashMap::with_capacity(capacity);
        for triangle in self.vertices.as_chunks::<3>().0 {
            let points = triangle.map(|vertex| Vec3::from_array(vertex.position));
            let normal = (points[1] - points[0]).cross(points[2] - points[0]);
            for vertex in triangle {
                *normals
                    .entry((vertex.face_id, vertex.position.map(f32::to_bits)))
                    .or_insert(Vec3::ZERO) += normal;
            }
        }
        for normal in normals.values_mut() {
            *normal = normal.normalize_or_zero();
        }
        for vertex in &mut self.vertices {
            let normal = normals[&(vertex.face_id, vertex.position.map(f32::to_bits))];
            if normal.length_squared() > 0.5 {
                vertex.normal = normal.to_array();
            }
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.vertices.is_empty() || self.vertices.len() > 300000 {
            return Err("Mesh exceeds its triangle budget".into());
        }
        if !self.vertices.len().is_multiple_of(3) {
            return Err("Incomplete triangle".into());
        }
        const REVERSED_EDGE: u64 = 1 << 63;
        assert!(self.vertices.len() < (1usize << 31));
        let capacity = self.vertices.len() / 6 + 2;
        let mut point_indices = std::collections::HashMap::with_capacity(capacity);
        let mut edges = Vec::with_capacity(self.vertices.len());
        for triangle in self.vertices.as_chunks::<3>().0 {
            let mut indices = [0u32; 3];
            for (index, vertex) in triangle.iter().enumerate() {
                if vertex.position.iter().any(|value| !value.is_finite()) {
                    return Err("Non-finite mesh".into());
                }
                let point = vertex
                    .position
                    .map(|value| (f64::from(value) * 1000.0).round() as i64);
                let next_index = point_indices.len() as u32;
                indices[index] = *point_indices.entry(point).or_insert(next_index);
            }
            for index in 0..3 {
                let a = indices[index];
                let b = indices[(index + 1) % 3];
                let edge = if a < b {
                    (u64::from(a) << 32) | u64::from(b)
                } else {
                    (u64::from(b) << 32) | u64::from(a) | REVERSED_EDGE
                };
                edges.push(edge);
            }
        }
        let key_mask = !REVERSED_EDGE;
        edges.sort_unstable_by_key(|edge| *edge & key_mask);
        let mut invalid_edge = None;
        let mut diagnostic_points = None;
        for group in edges.chunk_by(|first, second| *first & key_mask == *second & key_mask) {
            let count = group.len() as u32;
            let mut direction: i32 = group
                .iter()
                .map(|edge| if *edge & REVERSED_EDGE == 0 { 1 } else { -1 })
                .sum();
            if count == 2 && direction == 0 {
                continue;
            }
            let points = diagnostic_points.get_or_insert_with(|| {
                let mut points = vec![[0i64; 3]; point_indices.len()];
                for (point, index) in &point_indices {
                    points[*index as usize] = *point;
                }
                points
            });
            let key = group[0] & key_mask;
            let mut a = points[(key >> 32) as usize];
            let mut b = points[key as u32 as usize];
            if b < a {
                std::mem::swap(&mut a, &mut b);
                direction = -direction;
            }
            let edge = (a, b);
            if invalid_edge
                .as_ref()
                .is_none_or(|(previous, _, _)| edge < *previous)
            {
                invalid_edge = Some((edge, count, direction));
            }
        }
        if let Some((edge, count, direction)) = invalid_edge {
            return Err(format!(
                "Mesh is not closed and oriented at {edge:?} (0.001 mm units): {count} uses, orientation sum {direction}"
            ));
        }
        if self.volume_mm3() <= 0.0 {
            return Err("Mesh volume must be positive".into());
        }
        Ok(())
    }

    pub fn volume_mm3(&self) -> f64 {
        self.vertices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|triangle| {
                let points = triangle.map(|vertex| Vec3::from_array(vertex.position));
                points[0]
                    .as_dvec3()
                    .dot(points[1].as_dvec3().cross(points[2].as_dvec3()))
                    / 6.0
            })
            .sum()
    }

    #[cfg(test)]
    pub fn pick(&self, matrix: Mat4, position: [f32; 2]) -> Option<Hit> {
        let inverse = matrix.inverse();
        let origin = inverse.project_point3(Vec3::new(position[0], position[1], 0.0));
        let direction =
            (inverse.project_point3(Vec3::new(position[0], position[1], 1.0)) - origin).normalize();
        self.ray_hit(origin, direction)
    }

    pub fn ray_hit(&self, origin: Vec3, direction: Vec3) -> Option<Hit> {
        assert!(origin.is_finite());
        assert!(direction.is_finite());
        let mut closest = f32::INFINITY;
        let mut selected = None;
        for triangle in self.vertices.as_chunks::<3>().0.iter() {
            let points = triangle.map(|vertex| Vec3::from_array(vertex.position));
            let edge1 = points[1] - points[0];
            let edge2 = points[2] - points[0];
            let cross = direction.cross(edge2);
            let determinant = edge1.dot(cross);
            if determinant.abs() < 1e-7 {
                continue;
            }
            let offset = origin - points[0];
            let u = offset.dot(cross) / determinant;
            if !(-1e-5..=1.0 + 1e-5).contains(&u) {
                continue;
            }
            let other = offset.cross(edge1);
            let v = direction.dot(other) / determinant;
            if v < -1e-5 || u + v > 1.0 + 1e-5 {
                continue;
            }
            let distance = edge2.dot(other) / determinant;
            if distance >= 0.0 && distance < closest {
                closest = distance;
                selected = Some(Hit {
                    face_id: triangle[0].face_id,
                    position: origin + direction * distance,
                });
            }
        }
        selected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tetrahedron() -> Mesh {
        let origin = [0.0; 3];
        let x = [1.0, 0.0, 0.0];
        let y = [0.0, 1.0, 0.0];
        let z = [0.0, 0.0, 1.0];
        Mesh {
            face_count: 4,
            vertices: [[origin, y, x], [origin, x, z], [origin, z, y], [x, y, z]]
                .into_iter()
                .enumerate()
                .flat_map(|(index, points)| triangle(points, index as u32 + 1, [0.0; 3]))
                .collect(),
        }
    }

    #[test]
    fn validation_preserves_first_edge_diagnostics_and_signed_volume_checks() -> Result<(), String>
    {
        let mut mesh = tetrahedron();
        mesh.validate()?;
        mesh.vertices.truncate(9);
        assert_eq!(
            mesh.validate().unwrap_err(),
            "Mesh is not closed and oriented at ([0, 0, 1000], [0, 1000, 0]) (0.001 mm units): 1 uses, orientation sum 1"
        );
        let mut mesh = tetrahedron();
        mesh.vertices.swap(1, 2);
        assert_eq!(
            mesh.validate().unwrap_err(),
            "Mesh is not closed and oriented at ([0, 0, 0], [0, 1000, 0]) (0.001 mm units): 2 uses, orientation sum -2"
        );
        let mut mesh = tetrahedron();
        mesh.vertices.extend(mesh.vertices.clone());
        assert_eq!(
            mesh.validate().unwrap_err(),
            "Mesh is not closed and oriented at ([0, 0, 0], [0, 0, 1000]) (0.001 mm units): 4 uses, orientation sum 0"
        );
        let mut mesh = tetrahedron();
        for triangle in mesh.vertices.as_chunks_mut::<3>().0 {
            triangle.swap(1, 2);
        }
        assert_eq!(mesh.validate().unwrap_err(), "Mesh volume must be positive");
        Ok(())
    }

    #[test]
    fn validation_preserves_quantized_seams_and_rejects_bad_input() -> Result<(), String> {
        let mut disconnected = tetrahedron();
        let mut translated = tetrahedron();
        for vertex in &mut translated.vertices {
            vertex.position[0] -= 5.0;
            vertex.position[2] -= 3.0;
        }
        translated.validate()?;
        disconnected.vertices.extend(translated.vertices);
        disconnected.validate()?;
        let mut mesh = tetrahedron();
        mesh.vertices[0].position[0] = 0.00049;
        mesh.validate()?;
        mesh.vertices[0].position[0] = 0.00051;
        assert!(
            mesh.validate()
                .unwrap_err()
                .contains("not closed and oriented")
        );
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut mesh = tetrahedron();
            mesh.vertices[11].position[2] = value;
            assert_eq!(mesh.validate().unwrap_err(), "Non-finite mesh");
        }
        let mut mesh = tetrahedron();
        mesh.vertices.pop();
        assert_eq!(mesh.validate().unwrap_err(), "Incomplete triangle");
        mesh.vertices.clear();
        assert_eq!(
            mesh.validate().unwrap_err(),
            "Mesh exceeds its triangle budget"
        );
        Ok(())
    }

    fn triangle(points: [[f32; 3]; 3], face_id: u32, normal: [f32; 3]) -> [Vertex; 3] {
        points.map(|position| Vertex {
            position,
            normal,
            face_id,
            color: [31, 71, 113, 255],
        })
    }

    #[test]
    fn smoothing_weights_area_preserves_face_boundaries_and_leaves_metadata_unchanged() {
        let mut mesh = Mesh {
            face_count: 2,
            vertices: [
                triangle(
                    [[0.0; 3], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]],
                    1,
                    [0.0, 0.0, 1.0],
                ),
                triangle(
                    [[0.0; 3], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                    1,
                    [1.0, 0.0, 0.0],
                ),
                triangle(
                    [[0.0; 3], [0.0, 0.0, 2.0], [2.0, 0.0, 0.0]],
                    2,
                    [0.0, 1.0, 0.0],
                ),
            ]
            .concat(),
        };
        let metadata: Vec<_> = mesh
            .vertices
            .iter()
            .map(|vertex| (vertex.position, vertex.face_id, vertex.color))
            .collect();
        mesh.smooth_normals();
        let weighted = Vec3::new(1.0, 0.0, 4.0).normalize().to_array();
        assert_eq!(mesh.vertices[0].normal, weighted);
        assert_eq!(mesh.vertices[3].normal, weighted);
        assert_eq!(mesh.vertices[2].normal, [0.0, 0.0, 1.0]);
        assert_eq!(mesh.vertices[4].normal, [1.0, 0.0, 0.0]);
        assert_eq!(mesh.vertices[6].normal, [0.0, 1.0, 0.0]);
        assert_eq!(mesh.face_count, 2);
        for (vertex, original) in mesh.vertices.iter().zip(metadata) {
            assert_eq!((vertex.position, vertex.face_id, vertex.color), original);
        }
    }

    #[test]
    fn smoothing_preserves_cancelled_and_degenerate_normals_and_signed_zero_groups() {
        let mut mesh = Mesh {
            face_count: 1,
            vertices: [
                triangle(
                    [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                    1,
                    [0.0, 0.0, 1.0],
                ),
                triangle(
                    [[0.0; 3], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
                    1,
                    [0.0, 0.0, -1.0],
                ),
                triangle([[5.0; 3]; 3], 1, [0.0, -1.0, 0.0]),
                triangle(
                    [[-0.0, 0.0, 0.0], [0.0, 0.0, 2.0], [0.0, 2.0, 0.0]],
                    1,
                    [-1.0, 0.0, 0.0],
                ),
            ]
            .concat(),
        };
        let original: Vec<_> = mesh.vertices.iter().map(|vertex| vertex.normal).collect();
        mesh.smooth_normals();
        for (vertex, normal) in mesh.vertices.iter().zip(original) {
            assert_eq!(vertex.normal, normal);
        }
        assert!(mesh.vertices[9].position[0].is_sign_negative());
        let mut empty = Mesh {
            face_count: 0,
            vertices: Vec::new(),
        };
        empty.smooth_normals();
        assert!(empty.vertices.is_empty());
    }

    #[test]
    fn smoothing_bounds_transient_allocations_on_dense_meshes() {
        let mut mesh = Mesh {
            face_count: 1,
            vertices: Vec::with_capacity(3072),
        };
        for index in 0..1024 {
            let x = index as f32 * 2.0;
            mesh.vertices.extend(triangle(
                [[x, 0.0, 0.0], [x + 1.0, 0.0, 0.0], [x, 1.0, 0.0]],
                1,
                [0.0, 0.0, 1.0],
            ));
        }
        let (_, counts) = crate::allocations::measure(|| mesh.smooth_normals());
        assert!(counts.calls <= 4, "{counts:?}");
        assert!(
            counts.bytes <= mesh.vertices.len() as u64 * 80,
            "{counts:?}"
        );
        assert!(
            mesh.vertices
                .iter()
                .all(|vertex| vertex.normal == [0.0, 0.0, 1.0])
        );
    }

    #[test]
    fn smoothing_preserves_source_order_of_float_accumulation() {
        let mut mesh = Mesh {
            face_count: 1,
            vertices: [
                triangle(
                    [[0.0; 3], [0.0, 1000.0, 0.0], [0.0, 0.0, 1000.0]],
                    1,
                    [1.0, 0.0, 0.0],
                ),
                triangle(
                    [[0.0; 3], [0.0, 0.1, 0.0], [0.0, 0.0, 0.1]],
                    1,
                    [1.0, 0.0, 0.0],
                ),
                triangle(
                    [[0.0; 3], [0.0, 0.0, 1000.0], [0.0, 1000.0, 0.0]],
                    1,
                    [-1.0, 0.0, 0.0],
                ),
                triangle(
                    [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                    1,
                    [0.0, 0.0, 1.0],
                ),
            ]
            .concat(),
        };
        mesh.smooth_normals();
        for index in [0, 3, 6, 9] {
            assert_eq!(mesh.vertices[index].normal, [0.0, 0.0, 1.0]);
        }
        let first: Vec<_> = mesh
            .vertices
            .iter()
            .map(|vertex| vertex.normal.map(f32::to_bits))
            .collect();
        mesh.smooth_normals();
        for (vertex, normal) in mesh.vertices.iter().zip(first) {
            assert_eq!(vertex.normal.map(f32::to_bits), normal);
        }
    }
}
