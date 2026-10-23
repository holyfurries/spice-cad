use crate::kernel::Body;
use glam::{DMat4, DQuat, DVec3, Mat4};
use mlua::Table;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    #[default]
    Assembly,
    Print,
}

#[derive(Clone, Copy, Default, PartialEq)]
pub struct Placement {
    position: [f64; 3],
    rotation: DQuat,
}

impl Placement {
    pub fn orientation(self) -> Self {
        Self {
            position: [0.0; 3],
            ..self
        }
    }

    pub fn translation(self) -> glam::Vec3 {
        DVec3::from_array(self.position).as_vec3()
    }

    pub fn from_table(options: Table) -> mlua::Result<Self> {
        crate::model::options_fields(&options, &["position", "rotation"])?;
        let mut placement = Self::default();
        if let Some(position) = options.get::<Option<Table>>("position")? {
            crate::model::options_fields(&position, &["x", "y", "z"])?;
            for (index, key) in ["x", "y", "z"].into_iter().enumerate() {
                let value = position.get::<Option<f64>>(key)?.unwrap_or(0.0);
                if !value.is_finite() || value.abs() > 2000.0 {
                    return Err(mlua::Error::runtime(
                        "Placement positions must be finite and within ±2000 mm",
                    ));
                }
                placement.position[index] = value;
            }
        }
        if let Some(rotation) = options.get::<Option<Table>>("rotation")? {
            crate::model::options_fields(&rotation, &["x", "y", "z"])?;
            let mut angles = [0.0; 3];
            for (index, key) in ["x", "y", "z"].into_iter().enumerate() {
                let value = rotation.get::<Option<f64>>(key)?.unwrap_or(0.0);
                if !value.is_finite() || value.abs() > 360000.0 {
                    return Err(mlua::Error::runtime(
                        "Placement rotations must be finite and within ±360000 degrees",
                    ));
                }
                angles[index] = value.rem_euclid(360.0).to_radians();
            }
            placement.rotation =
                DQuat::from_euler(glam::EulerRot::ZYX, angles[2], angles[1], angles[0]).normalize();
        }
        Ok(placement)
    }

    pub fn matrix(self) -> Mat4 {
        self.matrix_f64().as_mat4()
    }

    fn matrix_f64(self) -> DMat4 {
        DMat4::from_rotation_translation(self.rotation, DVec3::from_array(self.position))
    }

    pub fn composed(self, local: Self) -> Result<Self, String> {
        let position =
            self.rotation * DVec3::from_array(local.position) + DVec3::from_array(self.position);
        if !position.is_finite() || position.abs().max_element() > 2000.0 {
            return Err("Composed group positions must remain within ±2000 mm".into());
        }
        let placement = Self {
            position: position.to_array(),
            rotation: (self.rotation * local.rotation).normalize(),
        };
        assert!(placement.matrix().is_finite());
        assert!((placement.rotation.length() - 1.0).abs() < 1e-12);
        Ok(placement)
    }

    pub fn apply(self, body: &mut Body<'_>, inverse: bool) -> Result<(), String> {
        assert!(self.rotation.is_finite());
        assert!((self.rotation.length() - 1.0).abs() < 1e-12);
        let (axis, angle) = self.rotation.to_axis_angle();
        if inverse {
            if self.position != [0.0; 3] {
                body.translate(self.position.map(|value| -value))?;
            }
            if angle != 0.0 {
                body.rotate(axis.to_array(), -angle.to_degrees())?;
            }
        } else {
            if angle != 0.0 {
                body.rotate(axis.to_array(), angle.to_degrees())?;
            }
            if self.position != [0.0; 3] {
                body.translate(self.position)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composed_rotations_remain_accurate_at_euler_singularities() -> Result<(), String> {
        let lua = mlua::Lua::new();
        for angle in [89.999999, 90.0, 270.0] {
            let parent = lua
                .load(format!(
                    "return {{position={{x=10,y=-20,z=30}},rotation={{x=35,y={angle},z=80}}}}"
                ))
                .eval()
                .map_err(|error| error.to_string())?;
            let local = lua
                .load("return {position={x=3,y=8,z=-5},rotation={x=-80,y=90,z=170}}")
                .eval()
                .map_err(|error| error.to_string())?;
            let parent = Placement::from_table(parent).map_err(|error| error.to_string())?;
            let local = Placement::from_table(local).map_err(|error| error.to_string())?;
            let composed = parent.composed(local)?;
            let expected = parent.matrix_f64() * local.matrix_f64();
            assert!(
                (composed.matrix_f64() - expected)
                    .to_cols_array()
                    .iter()
                    .all(|value| value.abs() < 1e-10)
            );
            assert!((composed.rotation.length() - 1.0).abs() < 1e-12);
        }
        Ok(())
    }

    #[test]
    fn native_placements_match_the_matrix_and_inverse_restores_geometry() -> Result<(), String> {
        let lua = mlua::Lua::new();
        let options = lua
            .load("return {rotation={x=30,y=-45,z=359730},position={x=40,y=-20,z=12}}")
            .eval()
            .map_err(|error| error.to_string())?;
        let placement = Placement::from_table(options).map_err(|error| error.to_string())?;
        let kernel = crate::kernel::Kernel::new()?;
        let mut body = kernel.box_shape([0.0; 3], [10.0, 20.0, 30.0])?;
        let original = body.bounds(true)?;
        placement.apply(&mut body, false)?;
        let mesh = body.mesh()?.mesh;
        let points = [0.0, 10.0]
            .into_iter()
            .flat_map(|x| {
                [0.0, 20.0].into_iter().flat_map(move |y| {
                    [0.0, 30.0]
                        .into_iter()
                        .map(move |z| placement.matrix().transform_point3(glam::vec3(x, y, z)))
                })
            })
            .collect::<Vec<_>>();
        for vertex in &mesh.vertices {
            let point = glam::Vec3::from_array(vertex.position);
            assert!(
                points
                    .iter()
                    .any(|expected| expected.distance(point) < 0.001)
            );
        }
        assert!((body.volume()? - 6000.0).abs() < 0.001);
        placement.apply(&mut body, true)?;
        let restored = body.bounds(true)?;
        assert!((restored[0] - original[0]).abs().max_element() < 1e-7);
        assert!((restored[1] - original[1]).abs().max_element() < 1e-7);
        Ok(())
    }

    #[test]
    fn invalid_placements_and_incomplete_print_layouts_are_rejected() {
        let lua = mlua::Lua::new();
        for source in [
            "{scale=2}",
            "{position={x=0/0}}",
            "{position={x=2001}}",
            "{rotation={x=math.huge}}",
            "{rotation={z=360001}}",
            "{position={width=2}}",
        ] {
            let table = lua
                .load(format!("return {source}"))
                .eval()
                .expect("Literal placement");
            assert!(Placement::from_table(table).is_err(), "{source}");
        }
        let error = crate::model::compile("cad.show('a',cad.box({width=1,depth=1,height=1}),{print={}});cad.show('b',cad.box({width=1,depth=1,height=1}))").err().expect("Incomplete layout");
        assert!(
            error.contains("declare a print placement for every part"),
            "{error}"
        );
    }
}
