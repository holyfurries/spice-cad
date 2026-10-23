use crate::placement::Placement;
use glam::{Mat4, Vec3};
use mlua::{Lua, Table};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    NegativeX,
    Y,
    NegativeY,
    Z,
    NegativeZ,
}

impl Axis {
    pub const ALL: [Self; 6] = [
        Self::X,
        Self::NegativeX,
        Self::Y,
        Self::NegativeY,
        Self::Z,
        Self::NegativeZ,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::X => "x+",
            Self::NegativeX => "x-",
            Self::Y => "y+",
            Self::NegativeY => "y-",
            Self::Z => "z+",
            Self::NegativeZ => "z-",
        }
    }

    pub fn vector(self) -> Vec3 {
        match self {
            Self::X => Vec3::X,
            Self::NegativeX => Vec3::NEG_X,
            Self::Y => Vec3::Y,
            Self::NegativeY => Vec3::NEG_Y,
            Self::Z => Vec3::Z,
            Self::NegativeZ => Vec3::NEG_Z,
        }
    }

    fn from_name(name: &str) -> mlua::Result<Self> {
        Self::ALL
            .into_iter()
            .find(|a| a.name() == name)
            .ok_or_else(|| mlua::Error::runtime("Axis must be x+, x-, y+, y-, z+, or z-"))
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct Game {
    pub crowns_adrift: bool,
    pub forward: Axis,
    pub up: Axis,
    pub scale: f64,
    pub ground: bool,
}

impl Default for Game {
    fn default() -> Self {
        Self {
            crowns_adrift: false,
            forward: Axis::NegativeY,
            up: Axis::Z,
            scale: 1.0,
            ground: false,
        }
    }
}

impl Game {
    pub fn validate(self) -> Result<(), String> {
        if self.forward.vector().dot(self.up.vector()).abs() > 0.5 {
            return Err("Model forward and up must use perpendicular axes".into());
        }
        if !self.scale.is_finite() || !(0.0001..=10000.0).contains(&self.scale) {
            return Err("Game export scale must be finite and between 0.0001 and 10000".into());
        }
        Ok(())
    }

    pub fn rotation(self) -> Mat4 {
        let basis = |p: Vec3| Vec3::new(p.x, p.z, -p.y);
        let forward = basis(self.forward.vector());
        let up = basis(self.up.vector());
        let right = up.cross(forward);
        Mat4::from_cols(
            right.extend(0.0),
            up.extend(0.0),
            forward.extend(0.0),
            glam::Vec4::W,
        )
        .transpose()
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
pub struct Options {
    pub declared: bool,
    pub rotation: [f64; 3],
    pub game: Game,
}

impl Options {
    pub fn placement(self) -> Result<Placement, String> {
        Placement::from_components([0.0; 3], self.rotation)
    }

    pub fn from_table(table: &Table) -> mlua::Result<Self> {
        crate::model::options_fields(table, &["rotation", "game"])?;
        let mut options = Self {
            declared: true,
            ..Self::default()
        };
        if let Some(rotation) = table.get::<Option<Table>>("rotation")? {
            crate::model::options_fields(&rotation, &["x", "y", "z"])?;
            for (index, name) in ["x", "y", "z"].into_iter().enumerate() {
                options.rotation[index] = rotation.get::<Option<f64>>(name)?.unwrap_or(0.0);
            }
        }
        options.placement().map_err(mlua::Error::runtime)?;
        if let Some(game) = table.get::<Option<Table>>("game")? {
            crate::model::options_fields(&game, &["preset", "forward", "up", "scale", "ground"])?;
            options.game.crowns_adrift = match game.get::<Option<String>>("preset")?.as_deref() {
                None | Some("gltf") => false,
                Some("crowns_adrift") => true,
                _ => {
                    return Err(mlua::Error::runtime(
                        "Game preset must be gltf or crowns_adrift",
                    ));
                }
            };
            for (name, target) in [
                ("forward", &mut options.game.forward),
                ("up", &mut options.game.up),
            ] {
                if let Some(axis) = game.get::<Option<String>>(name)? {
                    *target = Axis::from_name(&axis)?;
                }
            }
            options.game.scale = game.get::<Option<f64>>("scale")?.unwrap_or(1.0);
            options.game.ground = game
                .get::<Option<bool>>("ground")?
                .unwrap_or(options.game.crowns_adrift);
        }
        options.game.validate().map_err(mlua::Error::runtime)?;
        Ok(options)
    }
}

pub fn register(lua: &Lua, cad: &Table) -> mlua::Result<Rc<RefCell<Options>>> {
    let state = Rc::new(RefCell::new(Options::default()));
    let settings = Rc::clone(&state);
    cad.set(
        "model_options",
        lua.create_function(move |_, table: Table| {
            if settings.borrow().declared {
                return Err(mlua::Error::runtime(
                    "Declare cad.model_options only once per model",
                ));
            }
            *settings.borrow_mut() = Options::from_table(&table)?;
            Ok(())
        })?,
    )?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_axis_frames_are_right_handed_and_invalid_settings_are_rejected() {
        let basis = |p: Vec3| Vec3::new(p.x, p.z, -p.y);
        let mut count = 0;
        for forward in Axis::ALL {
            for up in Axis::ALL {
                let game = Game {
                    forward,
                    up,
                    ..Game::default()
                };
                if forward.vector().dot(up.vector()) != 0.0 {
                    assert!(game.validate().is_err());
                    continue;
                }
                game.validate().unwrap();
                assert!(
                    game.rotation()
                        .transform_vector3(basis(forward.vector()))
                        .distance(Vec3::Z)
                        < 1e-6
                );
                assert!(
                    game.rotation()
                        .transform_vector3(basis(up.vector()))
                        .distance(Vec3::Y)
                        < 1e-6
                );
                assert!((game.rotation().determinant() - 1.0).abs() < 1e-6);
                count += 1;
            }
        }
        assert_eq!(count, 24);
        assert_eq!(Game::default().rotation(), Mat4::IDENTITY);
        for options in [
            "{game={forward='x+',up='x-'}}",
            "{game={forward='bad'}}",
            "{game={scale=0}}",
            "{game={scale=0/0}}",
            "{game={scale=10001}}",
            "{game={scale=0.00001}}",
            "{game={preset='unknown'}}",
            "{game={unknown=true}}",
            "{unknown=1}",
            "{rotation={x=1/0}}",
            "{rotation={z=360001}}",
            "{rotation={unknown=0}}",
        ] {
            let source = format!(
                "cad.model_options({options});cad.show('box',cad.box({{width=1,depth=1,height=1}}))"
            );
            assert!(crate::model::compile(&source).is_err(), "{options}");
        }
        assert!(crate::model::compile("cad.model_options({});cad.model_options({});cad.show('box',cad.box({width=1,depth=1,height=1}))").is_err());
    }

    #[test]
    fn assembly_orientation_reuses_geometry_and_keeps_print_positions() -> Result<(), String> {
        let source = "local root=cad.group('root',{assembly={position={x=10},rotation={x=20}}});local grip=cad.group('grip',{parent=root,assembly={position={z=5},rotation={y=25}}});cad.show('box',cad.box({width=2,depth=3,height=4}),{parent=grip})";
        let kernel = crate::kernel::Kernel::new()?;
        let mut cache = crate::model::Cache::new(&kernel);
        let original = cache.compile(source)?;
        let before = cache.prepare(&original)?;
        let rotated=cache.compile(&format!("cad.model_options({{rotation={{x=15,y=90,z=45}},game={{preset='crowns_adrift',scale=25}}}});{source}"))?;
        let after = cache.prepare(&rotated)?;
        assert_eq!(after.hits, 1);
        assert!(std::sync::Arc::ptr_eq(
            &before.geometry[0],
            &after.geometry[0]
        ));
        assert_eq!(
            rotated.parts[0]
                .placement(crate::placement::Layout::Print)
                .matrix(),
            original.parts[0].assembly.matrix()
        );
        let expected = rotated.options.placement()?.matrix() * original.parts[0].assembly.matrix();
        assert!(
            rotated.parts[0]
                .assembly
                .matrix()
                .to_cols_array()
                .iter()
                .zip(expected.to_cols_array())
                .all(|(a, b)| (*a - b).abs() < 1e-5)
        );
        assert!(rotated.options.game.ground);
        let game_only = cache.compile(&format!(
            "cad.model_options({{game={{scale=25}}}});{source}"
        ))?;
        assert!(game_only.parts[0].print.is_none());
        assert_eq!(
            game_only.parts[0].assembly.matrix(),
            original.parts[0].assembly.matrix()
        );
        let explicit = source.replace("{parent=grip}", "{parent=grip,print={position={x=30}}}");
        let rotated = cache.compile(&format!(
            "cad.model_options({{rotation={{y=90}}}});{explicit}"
        ))?;
        assert_eq!(
            rotated.parts[0]
                .placement(crate::placement::Layout::Print)
                .translation(),
            glam::vec3(30.0, 0.0, 0.0)
        );
        let mixed = format!(
            "cad.model_options({{rotation={{y=90}}}});{explicit};cad.show('other',cad.box({{width=1,depth=1,height=1}}))"
        );
        assert!(cache.compile(&mixed).is_err());
        Ok(())
    }
}
