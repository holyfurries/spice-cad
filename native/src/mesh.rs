use glam::{Mat4, Vec3};

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
}

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
    pub fn smooth_normals(&mut self) {
        let mut normals = std::collections::BTreeMap::new();
        for triangle in self.vertices.as_chunks::<3>().0 {
            let points = triangle.map(|vertex| Vec3::from_array(vertex.position));
            let normal = (points[1] - points[0]).cross(points[2] - points[0]);
            for vertex in triangle {
                *normals
                    .entry((vertex.face_id, vertex.position.map(f32::to_bits)))
                    .or_insert(Vec3::ZERO) += normal;
            }
        }
        for vertex in &mut self.vertices {
            let normal =
                normals[&(vertex.face_id, vertex.position.map(f32::to_bits))].normalize_or_zero();
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
        let mut edges = std::collections::BTreeMap::new();
        for triangle in self.vertices.as_chunks::<3>().0.iter() {
            for index in 0..3 {
                let point = |vertex: &Vertex| {
                    vertex
                        .position
                        .map(|value| (f64::from(value) * 1000.0).round() as i64)
                };
                if triangle[index]
                    .position
                    .iter()
                    .any(|value| !value.is_finite())
                {
                    return Err("Non-finite mesh".into());
                }
                let a = point(&triangle[index]);
                let b = point(&triangle[(index + 1) % 3]);
                let (key, direction) = if a < b {
                    ((a, b), 1_i32)
                } else {
                    ((b, a), -1_i32)
                };
                let entry = edges.entry(key).or_insert((0_u32, 0_i32));
                entry.0 += 1;
                entry.1 += direction;
            }
        }
        if let Some((edge, (count, direction))) = edges
            .iter()
            .find(|(_, (count, direction))| *count != 2 || *direction != 0)
        {
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

    pub fn pick(&self, matrix: Mat4, position: [f32; 2]) -> Option<Hit> {
        let inverse = matrix.inverse();
        let origin = inverse.project_point3(Vec3::new(position[0], position[1], 0.0));
        let direction =
            (inverse.project_point3(Vec3::new(position[0], position[1], 1.0)) - origin).normalize();
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
