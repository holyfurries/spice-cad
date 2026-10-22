use mlua::{Lua, Table, UserData};

#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    pub base_color: [f32; 3],
    pub roughness: f32,
    pub metallic: f32,
    pub specular: f32,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            name: "Default".into(),
            base_color: crate::mesh::DEFAULT_COLOR,
            roughness: 0.7,
            metallic: 0.0,
            specular: 1.0,
        }
    }
}

impl UserData for Material {}

pub fn register(lua: &Lua, cad: &Table) -> mlua::Result<()> {
    cad.set(
        "material",
        lua.create_function(|_, (name, options): (String, Table)| {
            if name.is_empty() || name.len() > 80 || name.chars().any(char::is_control) {
                return Err(mlua::Error::RuntimeError(
                    "Material name must contain 1–80 bytes without control characters".into(),
                ));
            }
            crate::model::options_fields(
                &options,
                &["base_color", "roughness", "metallic", "specular"],
            )?;
            let mut material = Material {
                name,
                ..Material::default()
            };
            if let Some(rgb) = options.get::<Option<Table>>("base_color")? {
                crate::model::options_fields(&rgb, &["r", "g", "b"])?;
                for (index, key) in ["r", "g", "b"].into_iter().enumerate() {
                    material.base_color[index] = unit_interval(rgb.get(key)?)?;
                }
            }
            for (key, target) in [
                ("roughness", &mut material.roughness),
                ("metallic", &mut material.metallic),
                ("specular", &mut material.specular),
            ] {
                if let Some(value) = options.get::<Option<f64>>(key)? {
                    *target = unit_interval(value)?;
                }
            }
            Ok(material)
        })?,
    )
}

fn unit_interval(value: f64) -> mlua::Result<f32> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(mlua::Error::RuntimeError(
            "Material values must be finite and between 0 and 1".into(),
        ));
    }
    Ok(value as f32)
}

#[cfg(test)]
mod tests {
    #[test]
    fn reusable_materials_are_immutable_validated_values() -> Result<(), String> {
        let source = "local m=cad.material('Bronze',{base_color={r=0.6,g=0.3,b=0.1},roughness=0.2,metallic=1,specular=1});local a=cad.box({width=10,depth=10,height=10});cad.show('one',a,{material=m});cad.show('two',cad.translate(a,{x=20}),{material=m})";
        let model = crate::model::compile(source)?;
        assert_eq!(model.parts[0].material, model.parts[1].material);
        assert_eq!(model.parts[0].material.name, "Bronze");
        assert_eq!(model.parts[0].material.metallic, 1.0);
        for options in [
            "{roughness=-1}",
            "{roughness=1.1}",
            "{metallic=0/0}",
            "{specular=1/0}",
            "{unknown=1}",
            "{base_color={r=1,g=0,b=2}}",
            "{base_color={r=1,g=0}}",
            "{base_color={r=1,g=0,b=0,a=1}}",
            "{roughness='bad'}",
        ] {
            assert!(
                crate::model::compile(&format!("cad.material('bad',{options});cad.show('box',cad.box({{width=1,depth=1,height=1}}))")).is_err(),
                "{options}"
            );
        }
        for name in ["", "\n", &"x".repeat(81)] {
            let name = serde_json::to_string(name).unwrap();
            assert!(crate::model::compile(&format!("cad.material({name},{{}});cad.show('box',cad.box({{width=1,depth=1,height=1}}))")).is_err());
        }
        assert!(
            crate::model::compile("local m=cad.material('immutable',{}); m.roughness=0.5;cad.show('box',cad.box({width=1,depth=1,height=1}))")
                .is_err()
        );
        Ok(())
    }
}
