use crate::kernel::{Body, Kernel};
use mlua::{AnyUserData, Lua, Table, UserData, VmState};
use std::{
    cell::RefCell,
    io::Read,
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};

const OPERATION_COUNT_MAX: usize = 256;
const EVALUATION_COUNT_MAX: usize = 512;

#[derive(Clone, Copy)]
enum Operation {
    Cylinder {
        origin: [f64; 3],
        radius: f64,
        height: f64,
    },
    Modify {
        source: usize,
        modification: Modification,
    },
    Box {
        minimum: [f64; 3],
        maximum: [f64; 3],
    },
    Boolean {
        first: usize,
        second: usize,
        subtract: bool,
    },
}

#[derive(Clone, Copy)]
enum Modification {
    Translate([f64; 3]),
    Rotate { axis: [f64; 3], angle: f64 },
    Fillet(f64),
}

#[derive(Clone, Copy)]
struct Solid(usize);
impl UserData for Solid {}

#[derive(Default)]
struct Recipe {
    operations: Vec<Operation>,
    output: Option<(String, usize)>,
}

pub struct Model {
    recipe: Recipe,
    pub name: String,
    pub analysis_ms: f64,
    pub script_ms: f64,
}

pub fn read_source(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut source = String::new();
    file.take(crate::text_buffer::SIZE_BYTES_MAX as u64 + 1)
        .read_to_string(&mut source)
        .map_err(|error| error.to_string())?;
    if source.len() > crate::text_buffer::SIZE_BYTES_MAX {
        return Err("Editor source exceeds 16 MiB".into());
    }
    Ok(source)
}

pub fn compile(source: &str) -> Result<Model, String> {
    let started = Instant::now();
    let report = crate::analysis::check(source, || false)?.ok_or("Analysis cancelled")?;
    if let Some(error) = report.error() {
        return Err(error);
    }
    let analysis_ms = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let lua = Lua::new();
    lua.set_memory_limit(8 * 1024 * 1024)
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + Duration::from_millis(250);
    lua.set_interrupt(move |_| {
        if Instant::now() >= deadline {
            return Err(mlua::Error::RuntimeError(
                "Model exceeded its 250 ms script budget".into(),
            ));
        }
        Ok(VmState::Continue)
    });
    let recipe = Rc::new(RefCell::new(Recipe::default()));
    let cad = lua.create_table().map_err(|error| error.to_string())?;
    let state = Rc::clone(&recipe);
    cad.set(
        "box",
        lua.create_function(move |_, options: Table| {
            for pair in options.clone().pairs::<String, mlua::Value>() {
                let (key, _) = pair?;
                if !["width", "depth", "height", "x", "y", "z"].contains(&key.as_str()) {
                    return Err(mlua::Error::RuntimeError(format!(
                        "Unknown box field: {key}"
                    )));
                }
            }
            let size = [
                options.get::<f64>("width")?,
                options.get::<f64>("depth")?,
                options.get::<f64>("height")?,
            ];
            let minimum = [
                options.get::<Option<f64>>("x")?.unwrap_or(0.0),
                options.get::<Option<f64>>("y")?.unwrap_or(0.0),
                options.get::<Option<f64>>("z")?.unwrap_or(0.0),
            ];
            if size
                .iter()
                .any(|value| !value.is_finite() || !(0.01..=1000.0).contains(value))
            {
                return Err(mlua::Error::RuntimeError(
                    "Box dimensions must be finite and between 0.01 and 1000 mm".into(),
                ));
            }
            let maximum = std::array::from_fn(|index| minimum[index] + size[index]);
            if minimum
                .iter()
                .chain(maximum.iter())
                .any(|value| !value.is_finite() || value.abs() > 2000.0)
            {
                return Err(mlua::Error::RuntimeError(
                    "Box coordinates must be within ±2000 mm".into(),
                ));
            }
            push_operation(&state, Operation::Box { minimum, maximum })
        })
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    for (name, subtract) in [("union", false), ("subtract", true)] {
        let state = Rc::clone(&recipe);
        cad.set(
            name,
            lua.create_function(move |_, (first, second): (AnyUserData, AnyUserData)| {
                let first = first.borrow::<Solid>()?.0;
                let second = second.borrow::<Solid>()?.0;
                push_operation(
                    &state,
                    Operation::Boolean {
                        first,
                        second,
                        subtract,
                    },
                )
            })
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    }
    let state = Rc::clone(&recipe);
    cad.set(
        "cylinder",
        lua.create_function(move |_, options: Table| {
            options_fields(&options, &["radius", "height", "x", "y", "z"])?;
            let radius = dimension(&options, "radius")?;
            let height = dimension(&options, "height")?;
            let origin = coordinates(&options)?;
            push_operation(
                &state,
                Operation::Cylinder {
                    origin,
                    radius,
                    height,
                },
            )
        })
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    for name in ["translate", "rotate", "fillet"] {
        let state = Rc::clone(&recipe);
        cad.set(
            name,
            lua.create_function(move |_, (solid, options): (AnyUserData, Table)| {
                let source = solid.borrow::<Solid>()?.0;
                let modification = match name {
                    "translate" => {
                        options_fields(&options, &["x", "y", "z"])?;
                        Modification::Translate(coordinates(&options)?)
                    }
                    "rotate" => {
                        options_fields(&options, &["axis", "angle"])?;
                        let axis: String = options.get("axis")?;
                        let axis = match axis.as_str() {
                            "x" => [1.0, 0.0, 0.0],
                            "y" => [0.0, 1.0, 0.0],
                            "z" => [0.0, 0.0, 1.0],
                            _ => {
                                return Err(mlua::Error::RuntimeError(
                                    "Rotation axis must be x, y, or z".into(),
                                ));
                            }
                        };
                        let angle: f64 = options.get("angle")?;
                        if !angle.is_finite() || angle.abs() > 360000.0 {
                            return Err(mlua::Error::RuntimeError(
                                "Rotation angle must be finite and within ±360000 degrees".into(),
                            ));
                        }
                        Modification::Rotate { axis, angle }
                    }
                    "fillet" => {
                        options_fields(&options, &["radius"])?;
                        Modification::Fillet(dimension(&options, "radius")?)
                    }
                    _ => unreachable!(),
                };
                push_operation(
                    &state,
                    Operation::Modify {
                        source,
                        modification,
                    },
                )
            })
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    }
    let state = Rc::clone(&recipe);
    cad.set(
        "show",
        lua.create_function(move |_, (name, solid): (String, AnyUserData)| {
            if name.is_empty() || name.len() > 80 || name.chars().any(char::is_control) {
                return Err(mlua::Error::RuntimeError(
                    "Output name must contain 1–80 bytes without control characters".into(),
                ));
            }
            let mut state = state.borrow_mut();
            if state.output.is_some() {
                return Err(mlua::Error::RuntimeError(
                    "Exactly one cad.show output is supported".into(),
                ));
            }
            state.output = Some((name, solid.borrow::<Solid>()?.0));
            Ok(())
        })
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    cad.set_readonly(true);
    lua.globals()
        .set("cad", cad)
        .map_err(|error| error.to_string())?;
    for name in [
        "require",
        "loadstring",
        "getfenv",
        "setfenv",
        "collectgarbage",
        "print",
    ] {
        lua.globals()
            .set(name, mlua::Value::Nil)
            .map_err(|error| error.to_string())?;
    }
    lua.sandbox(true).map_err(|error| error.to_string())?;
    lua.load(source)
        .set_name("@model.luau")
        .exec()
        .map_err(|error| error.to_string())?;
    let recipe = std::mem::take(&mut *recipe.borrow_mut());
    let name = recipe
        .output
        .as_ref()
        .ok_or("Model must call cad.show(name, solid)")?
        .0
        .clone();
    Ok(Model {
        recipe,
        name,
        analysis_ms,
        script_ms: started.elapsed().as_secs_f64() * 1000.0,
    })
}

fn options_fields(options: &Table, names: &[&str]) -> mlua::Result<()> {
    for pair in options.clone().pairs::<String, mlua::Value>() {
        let (name, _) = pair?;
        if !names.contains(&name.as_str()) {
            return Err(mlua::Error::RuntimeError(format!("Unknown option: {name}")));
        }
    }
    Ok(())
}

fn dimension(options: &Table, name: &str) -> mlua::Result<f64> {
    let value: f64 = options.get(name)?;
    if !value.is_finite() || !(0.01..=1000.0).contains(&value) {
        return Err(mlua::Error::RuntimeError(format!(
            "{name} must be finite and between 0.01 and 1000 mm"
        )));
    }
    Ok(value)
}

fn coordinates(options: &Table) -> mlua::Result<[f64; 3]> {
    let result = [
        options.get::<Option<f64>>("x")?.unwrap_or(0.0),
        options.get::<Option<f64>>("y")?.unwrap_or(0.0),
        options.get::<Option<f64>>("z")?.unwrap_or(0.0),
    ];
    if result
        .iter()
        .any(|value| !value.is_finite() || value.abs() > 2000.0)
    {
        return Err(mlua::Error::RuntimeError(
            "Coordinates must be finite and within ±2000 mm".into(),
        ));
    }
    Ok(result)
}

fn push_operation(recipe: &RefCell<Recipe>, operation: Operation) -> mlua::Result<Solid> {
    let mut recipe = recipe.borrow_mut();
    if recipe.operations.len() >= OPERATION_COUNT_MAX {
        return Err(mlua::Error::RuntimeError(
            "Model exceeds 256 operations".into(),
        ));
    }
    let index = recipe.operations.len();
    recipe.operations.push(operation);
    assert!(index < OPERATION_COUNT_MAX);
    Ok(Solid(index))
}

impl Model {
    pub fn build<'kernel>(&self, kernel: &'kernel Kernel) -> Result<Body<'kernel>, String> {
        let output = self.recipe.output.as_ref().ok_or("Missing model output")?.1;
        assert!(output < self.recipe.operations.len());
        let mut pending = vec![(output, false)];
        let mut bodies = Vec::<Body<'kernel>>::new();
        let mut count = 0;
        while let Some((index, visited)) = pending.pop() {
            count += 1;
            if count > EVALUATION_COUNT_MAX {
                return Err("Model exceeds 512 evaluation steps; reduce repeated subtrees".into());
            }
            match self.recipe.operations[index] {
                Operation::Cylinder {
                    origin,
                    radius,
                    height,
                } => bodies.push(kernel.cylinder(origin, radius, height)?),
                Operation::Modify {
                    source,
                    modification,
                } => {
                    assert!(source < index);
                    if visited {
                        let mut body = bodies.pop().ok_or("Missing modification operand")?;
                        match modification {
                            Modification::Translate(offset) => body.translate(offset)?,
                            Modification::Rotate { axis, angle } => body.rotate(axis, angle)?,
                            Modification::Fillet(radius) => body.fillet(radius)?,
                        }
                        bodies.push(body);
                    } else {
                        pending.extend([(index, true), (source, false)]);
                    }
                }
                Operation::Box { minimum, maximum } => {
                    bodies.push(kernel.box_shape(minimum, maximum)?)
                }
                Operation::Boolean {
                    first,
                    second,
                    subtract,
                } => {
                    assert!(first < index);
                    assert!(second < index);
                    if visited {
                        let second = bodies.pop().ok_or("Missing second operand")?;
                        let mut first = bodies.pop().ok_or("Missing first operand")?;
                        if subtract {
                            first.subtract(second)?;
                        } else {
                            first.union(second)?;
                        }
                        bodies.push(first);
                    } else {
                        pending.extend([(index, true), (second, false), (first, false)]);
                    }
                }
            }
        }
        assert_eq!(bodies.len(), 1);
        bodies.pop().ok_or("Model produced no solid".into())
    }
}
