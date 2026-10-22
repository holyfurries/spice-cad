use mlua::{Lua, Table, UserData};

#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    pub base_color: [f32; 3],
    pub specular_roughness: f32,
    pub base_metalness: f32,
    pub specular_weight: f32,
    pub specular_ior: f32,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            name: "Default".into(),
            base_color: [0.8; 3],
            specular_roughness: 0.3,
            base_metalness: 0.0,
            specular_weight: 1.0,
            specular_ior: 1.5,
        }
    }
}

impl UserData for Material {}

impl Material {
    pub fn effective_ior(&self) -> f32 {
        assert!((1.0..=3.0).contains(&self.specular_ior));
        assert!((0.0..=1.0).contains(&self.specular_weight));
        let ratio = (f64::from(self.specular_ior) - 1.0) / (f64::from(self.specular_ior) + 1.0);
        let reflectivity_sqrt = (f64::from(self.specular_weight) * ratio * ratio).sqrt();
        ((1.0 + reflectivity_sqrt) / (1.0 - reflectivity_sqrt)) as f32
    }
}

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
                &["base_color", "specular_roughness", "base_metalness", "specular_weight", "specular_ior"],
            ).map_err(|error| mlua::Error::RuntimeError(format!("Unsupported OpenPBR material parameter: {error}")))?;
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
                ("specular_roughness", &mut material.specular_roughness),
                ("base_metalness", &mut material.base_metalness),
                ("specular_weight", &mut material.specular_weight),
            ] {
                if let Some(value) = options.get::<Option<f64>>(key)? {
                    *target = unit_interval(value)?;
                }
            }
            if let Some(ior) = options.get::<Option<f64>>("specular_ior")? {
                if !ior.is_finite() || !(1.0..=3.0).contains(&ior) {
                    return Err(mlua::Error::RuntimeError("Supported OpenPBR specular_ior range is 1 to 3".into()));
                }
                material.specular_ior = ior as f32;
            }
            if material.base_metalness > 0.0 && material.specular_weight != 1.0 {
                return Err(mlua::Error::RuntimeError("Unsupported OpenPBR combination: base_metalness > 0 requires specular_weight = 1; metallic lobe weighting cannot be represented by this glTF profile".into()));
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
        let source = "local m=cad.material('Bronze',{base_color={r=0.6,g=0.3,b=0.1},specular_roughness=0.2,base_metalness=1,specular_weight=1});local a=cad.box({width=10,depth=10,height=10});cad.show('one',a,{material=m});cad.show('two',cad.translate(a,{x=20}),{material=m})";
        let model = crate::model::compile(source)?;
        assert_eq!(model.parts[0].material, model.parts[1].material);
        assert_eq!(model.parts[0].material.name, "Bronze");
        assert_eq!(model.parts[0].material.base_metalness, 1.0);
        for options in [
            "{specular_roughness=-1}",
            "{specular_roughness=1.1}",
            "{base_metalness=0/0}",
            "{specular_weight=1/0}",
            "{unknown=1}",
            "{base_color={r=1,g=0,b=2}}",
            "{base_color={r=1,g=0}}",
            "{base_color={r=1,g=0,b=0,a=1}}",
            "{specular_roughness='bad'}",
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
            crate::model::compile("local m=cad.material('immutable',{}); m.specular_roughness=0.5;cad.show('box',cad.box({width=1,depth=1,height=1}))")
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn openpbr_profile_defaults_and_ior_modulation_match_reference_values() -> Result<(), String> {
        let material = super::Material::default();
        assert_eq!(material.base_color, [0.8; 3]);
        assert_eq!(material.specular_roughness, 0.3);
        assert_eq!(material.base_metalness, 0.0);
        assert_eq!(material.specular_weight, 1.0);
        assert_eq!(material.specular_ior, 1.5);
        for (ior, weight, expected_ior, expected_f0) in [
            (1.5, 1.0, 1.5, 0.04),
            (1.5, 0.25, 1.222222222222, 0.01),
            (2.0, 0.25, 1.4, 1.0 / 36.0),
            (3.0, 1.0, 3.0, 0.25),
            (1.5, 0.0, 1.0, 0.0),
            (1.0, 1.0, 1.0, 0.0),
        ] {
            let source = format!(
                "local m=cad.material('reference',{{specular_ior={ior},specular_weight={weight}}});cad.show('box',cad.box({{width=10,depth=10,height=10}}),{{material=m}})"
            );
            let model = crate::model::compile(&source)?;
            let effective = f64::from(model.parts[0].material.effective_ior());
            assert!((effective - expected_ior).abs() < 1e-6);
            let f0 = ((effective - 1.0) / (effective + 1.0)).powi(2);
            assert!((f0 - expected_f0).abs() < 1e-7);
        }
        Ok(())
    }

    #[test]
    fn unsupported_openpbr_features_and_combinations_fail_explicitly() {
        for options in [
            "{coat_weight=0.5}",
            "{transmission_weight=1}",
            "{subsurface_weight=1}",
            "{fuzz_weight=1}",
            "{specular_roughness_anisotropy=0.5}",
            "{base_diffuse_roughness=0.5}",
            "{specular_color={r=1,g=0,b=0}}",
            "{base_metalness=1,specular_weight=0.5}",
            "{base_metalness=0.5,specular_weight=0}",
            "{specular_ior=0.9}",
            "{specular_ior=3.1}",
            "{specular_ior=0/0}",
            "{specular_weight=2}",
            "{roughness=0.3}",
        ] {
            let result = crate::model::compile(&format!(
                "cad.material('unsupported',{options});cad.show('box',cad.box({{width=1,depth=1,height=1}}))"
            ));
            assert!(result.is_err(), "{options}");
        }
    }
}
