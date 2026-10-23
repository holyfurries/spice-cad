use mlua::{Lua, Table, Value};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[derive(Clone)]
pub struct Pose {
    pub name: String,
    pub parameters: BTreeMap<String, serde_json::Value>,
}

pub struct State {
    pub selected: Option<String>,
    pub poses: Vec<Pose>,
    parameters: BTreeMap<String, bool>,
    started: bool,
}

pub fn identifier(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

pub fn register(
    lua: &Lua,
    cad: &Table,
    selected: Option<&str>,
) -> mlua::Result<Rc<RefCell<State>>> {
    let state = Rc::new(RefCell::new(State {
        selected: selected.map(str::to_owned),
        poses: Vec::new(),
        parameters: BTreeMap::new(),
        started: false,
    }));
    let target = Rc::clone(&state);
    cad.set(
        "pose",
        lua.create_function(move |_, (name, values): (String, Table)| {
            let mut state = target.borrow_mut();
            if state.started {
                return Err(mlua::Error::runtime("Declare poses before parameters"));
            }
            if !identifier(&name)
                || state.poses.len() >= 32
                || state.poses.iter().any(|pose| pose.name == name)
            {
                return Err(mlua::Error::runtime(
                    "Declare at most 32 poses with unique portable identifiers",
                ));
            }
            let mut parameters = BTreeMap::new();
            for entry in values.pairs::<String, Value>() {
                let (key, value) = entry?;
                if !identifier(&key) || parameters.len() >= 64 {
                    return Err(mlua::Error::runtime(
                        "Pose exceeds parameter identifier or count limits",
                    ));
                }
                let value = match value {
                    Value::Boolean(value) => serde_json::json!(value),
                    Value::Integer(value) => serde_json::json!(value),
                    Value::Number(value) if value.is_finite() => serde_json::json!(value),
                    _ => {
                        return Err(mlua::Error::runtime(
                            "Pose values must be finite numbers or booleans",
                        ));
                    }
                };
                parameters.insert(key, value);
            }
            state.poses.push(Pose { name, parameters });
            Ok(())
        })?,
    )?;
    Ok(state)
}

impl State {
    pub fn resolve(&mut self, options: &Table, default: Value) -> mlua::Result<Value> {
        self.started = true;
        let Some(id) = options.get::<Option<String>>("id")? else {
            return Ok(default);
        };
        if !identifier(&id)
            || self.parameters.len() >= 64
            || self
                .parameters
                .insert(id.clone(), matches!(default, Value::Boolean(_)))
                .is_some()
        {
            return Err(mlua::Error::runtime(
                "Parameter ids must be unique portable identifiers; at most 64 are supported",
            ));
        }
        let value = self
            .poses
            .iter()
            .find(|pose| Some(&pose.name) == self.selected.as_ref())
            .and_then(|pose| pose.parameters.get(&id));
        match value {
            None => Ok(default),
            Some(serde_json::Value::Bool(value)) if matches!(default, Value::Boolean(_)) => {
                Ok(Value::Boolean(*value))
            }
            Some(value) if !matches!(default, Value::Boolean(_)) && value.is_number() => {
                Ok(Value::Number(value.as_f64().ok_or_else(|| {
                    mlua::Error::runtime("Invalid pose number")
                })?))
            }
            _ => Err(mlua::Error::runtime(format!(
                "Pose parameter type mismatch: {id}"
            ))),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(selected) = &self.selected {
            if !self.poses.iter().any(|pose| &pose.name == selected) {
                return Err(format!("Unknown pose: {selected}"));
            }
        }
        for pose in &self.poses {
            for (id, value) in &pose.parameters {
                let toggle = self.parameters.get(id).ok_or_else(|| {
                    format!("Pose '{}' refers to unknown parameter '{id}'", pose.name)
                })?;
                if *toggle != value.is_boolean() {
                    return Err(format!("Pose '{}' has wrong type for '{id}'", pose.name));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_overrides_validate_types_ids_and_ranges() -> Result<(), String> {
        let source = "cad.pose('idle',{stride=0}) cad.pose('step-left',{stride=1}) local stride=cad.number(0,{id='stride',min=-1,max=1}) cad.show('body',cad.box({width=2,depth=3,height=4}),{assembly={position={x=stride*10}}})";
        let mut analysis = crate::analysis::Cache::default();
        let model =
            crate::model::compile_project_pose(source, None, &mut analysis, Some("step-left"))?;
        assert_eq!(model.pose.as_deref(), Some("step-left"));
        assert_eq!(model.parts[0].assembly.translation().x, 10.0);
        assert!(
            crate::model::compile_project_pose(source, None, &mut analysis, Some("missing"))
                .is_err()
        );
        for invalid in [
            source.replace("stride=1", "stride=2"),
            source.replace("stride=1", "stride=true"),
            source.replace("stride=1", "missing=1"),
            source.replace("step-left", "../escape"),
            source.replace("step-left", "idle"),
        ] {
            assert!(
                crate::model::compile_project_pose(
                    &invalid,
                    None,
                    &mut analysis,
                    Some("step-left")
                )
                .is_err()
            );
        }
        assert!(identifier("step-left"));
        assert!(!identifier("../escape"));
        Ok(())
    }
}
