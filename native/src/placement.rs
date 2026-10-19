use crate::kernel::Body;
use glam::{DMat4, DVec3, Mat4};
use mlua::Table;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    #[default]
    Assembly,
    Print,
}

#[derive(Clone, Copy, Default)]
pub struct Placement {
    position: [f64; 3],
    rotation: [f64; 3],
}

impl Placement {
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
            for (index, key) in ["x", "y", "z"].into_iter().enumerate() {
                let value = rotation.get::<Option<f64>>(key)?.unwrap_or(0.0);
                if !value.is_finite() || value.abs() > 360000.0 {
                    return Err(mlua::Error::runtime(
                        "Placement rotations must be finite and within ±360000 degrees",
                    ));
                }
                placement.rotation[index] = value.rem_euclid(360.0);
            }
        }
        Ok(placement)
    }

    pub fn matrix(self) -> Mat4 {
        (DMat4::from_translation(DVec3::from_array(self.position))
            * DMat4::from_rotation_z(self.rotation[2].to_radians())
            * DMat4::from_rotation_y(self.rotation[1].to_radians())
            * DMat4::from_rotation_x(self.rotation[0].to_radians()))
        .as_mat4()
    }

    pub fn apply(self, body: &mut Body<'_>, inverse: bool) -> Result<(), String> {
        let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        if inverse {
            if self.position != [0.0; 3] {
                body.translate(self.position.map(|value| -value))?;
            }
            for index in (0..3).rev() {
                if self.rotation[index] != 0.0 {
                    body.rotate(axes[index], -self.rotation[index])?;
                }
            }
        } else {
            for (index, axis) in axes.into_iter().enumerate() {
                if self.rotation[index] != 0.0 {
                    body.rotate(axis, self.rotation[index])?;
                }
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
        let mesh = body.mesh()?;
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
