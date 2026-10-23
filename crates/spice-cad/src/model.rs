use crate::kernel::{Body, EdgeTreatment, Kernel, ProfileSweep};
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
const PART_COUNT_MAX: usize = 32;

#[path = "geometry_cache.rs"]
mod cache;
pub use cache::{Cache, DisplayQuality, Presentation, ThumbnailMode};

#[derive(Clone)]
pub struct Part {
    pub parent: Option<usize>,
    pub assembly: crate::placement::Placement,
    pub print: Option<crate::placement::Placement>,
    pub line: usize,
    pub name: String,
    pub material: crate::material::Material,
    pub shading: crate::shading::Shading,
    solid: usize,
}

impl Part {
    pub fn placement(&self, layout: crate::placement::Layout) -> crate::placement::Placement {
        match layout {
            crate::placement::Layout::Assembly => self.assembly,
            crate::placement::Layout::Print => self.print.unwrap_or(self.assembly),
        }
    }
}

#[derive(Clone, Copy)]
enum Operation {
    Sphere {
        origin: [f64; 3],
        radius: f64,
    },
    Capsule {
        origin: [f64; 3],
        radius: f64,
        height: f64,
    },
    Shape(usize),
    Profile {
        profile: usize,
        sweep: ProfileSweep,
    },
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
    Shell(usize),
    Translate([f64; 3]),
    Rotate { axis: [f64; 3], angle: f64 },
    EdgeTreatment(usize),
    Hole(usize),
    FaceFeature(usize),
}

enum EdgeTargets {
    All,
    Selected(Vec<usize>),
}
struct EdgeTreatmentRecipe {
    treatment: EdgeTreatment,
    edges: EdgeTargets,
}

#[derive(Clone, Copy)]
pub(crate) struct Solid(pub(crate) usize);
impl UserData for Solid {}

struct Profile(usize);
impl UserData for Profile {}

struct Edge(usize);
impl UserData for Edge {}

#[derive(Default)]
struct Recipe {
    shapes: Vec<crate::shape_operation::Recipe>,
    shells: Vec<crate::shape_operation::Shell>,
    print_options: Option<crate::print_preparation::Options>,
    face_features: Vec<crate::face_feature::Recipe>,
    holes: Vec<crate::hole::Recipe>,
    edges: Vec<crate::edge_reference::Reference>,
    edge_treatments: Vec<EdgeTreatmentRecipe>,
    profiles: Vec<crate::profile::Profile>,
    operations: Vec<Operation>,
    outputs: Vec<Part>,
}

pub struct Model {
    pub references: Vec<crate::game_contract::Reference>,
    pub poses: Vec<crate::poses::Pose>,
    pub pose: Option<String>,
    pub asset: crate::game_contract::Asset,
    pub options: crate::model_options::Options,
    pub groups: Vec<crate::assembly::Group>,
    pub print_options: crate::print_preparation::Options,
    recipe: Rc<Recipe>,
    definitions: crate::dimensions::Definitions,
    pub name: String,
    pub parts: Vec<Part>,
    pub analysis_ms: f64,
    pub script_ms: f64,
}

#[derive(Clone)]
pub struct Geometry {
    pub mesh: crate::mesh::Mesh,
    pub edges: crate::inspection::Edges,
    pub volume_mm3: f64,
    pub bounds: [glam::Vec3; 2],
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

#[cfg(test)]
pub fn compile(source: &str) -> Result<Model, String> {
    compile_cached(source, &mut crate::analysis::Cache::default())
}

#[cfg(test)]
pub fn compile_cached(
    source: &str,
    analysis: &mut crate::analysis::Cache,
) -> Result<Model, String> {
    compile_project_cached(source, None, analysis)
}

pub fn compile_project_cached(
    source: &str,
    project: Option<&crate::project::Project>,
    analysis: &mut crate::analysis::Cache,
) -> Result<Model, String> {
    compile_project_pose(source, project, analysis, None)
}

pub fn compile_project_pose(
    source: &str,
    project: Option<&crate::project::Project>,
    analysis: &mut crate::analysis::Cache,
    pose: Option<&str>,
) -> Result<Model, String> {
    let started = Instant::now();
    if let Some(project) = project {
        analysis.validate_project(source, project)?;
    } else {
        analysis.validate(source)?;
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
    let groups = crate::assembly::register(&lua, &cad).map_err(|error| error.to_string())?;
    let options = crate::model_options::register(&lua, &cad).map_err(|error| error.to_string())?;
    let asset = crate::game_contract::register(&lua, &cad).map_err(|error| error.to_string())?;
    let references =
        crate::game_contract::register_references(&lua, &cad).map_err(|error| error.to_string())?;
    let poses = crate::poses::register(&lua, &cad, pose).map_err(|error| error.to_string())?;
    let pose_parameters = Rc::clone(&poses);
    crate::parameters::register(&lua, &cad, move |options, value| {
        pose_parameters.borrow_mut().resolve(options, value)
    })
    .map_err(|error| error.to_string())?;
    let state = Rc::clone(&recipe);
    cad.set(
        "print_options",
        lua.create_function(move |_, options: Table| {
            let options = crate::print_preparation::Options::from_table(&options)?;
            let mut recipe = state.borrow_mut();
            if recipe.print_options.is_some() {
                return Err(mlua::Error::runtime(
                    "Declare cad.print_options only once per model",
                ));
            }
            recipe.print_options = Some(options);
            Ok(())
        })
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let state = Rc::clone(&recipe);
    cad.set(
        "edge",
        lua.create_function(move |_, (solid, options): (AnyUserData, Table)| {
            let reference =
                crate::edge_reference::Reference::from_table(solid.borrow::<Solid>()?.0, options)?;
            let mut recipe = state.borrow_mut();
            if recipe.edges.len() >= crate::edge_reference::COUNT_MAX
                || recipe.edges.iter().any(|edge| edge.name == reference.name)
            {
                return Err(mlua::Error::RuntimeError(
                    "Edge names must be unique; at most 64 are supported".into(),
                ));
            }
            let index = recipe.edges.len();
            recipe.edges.push(reference);
            Ok(Edge(index))
        })
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    for name in ["polygon", "regular_polygon"] {
        let state = Rc::clone(&recipe);
        cad.set(
            name,
            lua.create_function(move |_, points: Table| {
                let polygon = if name == "regular_polygon" {
                    crate::profile::Polygon::regular(points)?
                } else {
                    crate::profile::Polygon::from_table(points)?
                };
                let mut recipe = state.borrow_mut();
                if recipe.profiles.len() >= OPERATION_COUNT_MAX {
                    return Err(mlua::Error::RuntimeError(
                        "Model exceeds 256 profiles".into(),
                    ));
                }
                let index = recipe.profiles.len();
                recipe
                    .profiles
                    .push(crate::profile::Profile::Polygon(polygon));
                Ok(Profile(index))
            })
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    }
    for name in ["circle", "path", "profile"] {
        let state = Rc::clone(&recipe);
        cad.set(
            name,
            lua.create_function(move |_, options: Table| {
                let profile = match name {
                    "circle" => crate::profile::Profile::circle(options)?,
                    "path" => crate::profile::Profile::path(options)?,
                    "profile" => {
                        options_fields(&options, &["loops"])?;
                        let entries: Table = options.get("loops")?;
                        let count = crate::profile::dense_count(
                            &entries,
                            1,
                            crate::profile::LOOP_COUNT_MAX,
                        )?;
                        let mut loops = Vec::with_capacity(count);
                        for index in 1..=count {
                            let profile: AnyUserData = entries.raw_get(index)?;
                            loops.push(
                                state.borrow().profiles[profile.borrow::<Profile>()?.0].clone(),
                            );
                        }
                        crate::profile::Profile::region(loops).map_err(mlua::Error::RuntimeError)?
                    }
                    _ => unreachable!(),
                };
                let mut recipe = state.borrow_mut();
                if recipe.profiles.len() >= OPERATION_COUNT_MAX {
                    return Err(mlua::Error::RuntimeError(
                        "Model exceeds 256 profiles".into(),
                    ));
                }
                let index = recipe.profiles.len();
                recipe.profiles.push(profile);
                Ok(Profile(index))
            })
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    }
    for name in ["extrude", "revolve"] {
        let state = Rc::clone(&recipe);
        cad.set(
            name,
            lua.create_function(move |_, (profile, options): (AnyUserData, Table)| {
                let sweep = if name == "extrude" {
                    options_fields(&options, &["height"])?;
                    ProfileSweep::Extrude {
                        height_mm: dimension(&options, "height")?,
                    }
                } else {
                    options_fields(&options, &["angle", "segments"])?;
                    let angle_deg = options.get::<Option<f64>>("angle")?.unwrap_or(360.0);
                    if !angle_deg.is_finite() || !(0.1..=360.0).contains(&angle_deg) {
                        return Err(mlua::Error::RuntimeError(
                            "Revolve angle must be finite and between 0.1 and 360 degrees".into(),
                        ));
                    }
                    let segments = options.get::<Option<f64>>("segments")?;
                    if let Some(segments) = segments {
                        if !segments.is_finite()
                            || !(3.0..=128.0).contains(&segments)
                            || segments.fract() != 0.0
                        {
                            return Err(mlua::Error::runtime(
                                "Revolve segments must be an integer within 3–128",
                            ));
                        }
                        if angle_deg != 360.0 {
                            return Err(mlua::Error::runtime(
                                "Faceted revolve requires a full 360-degree turn",
                            ));
                        }
                    }
                    ProfileSweep::Revolve {
                        angle_deg,
                        segments: segments.map(|value| value as u32),
                    }
                };
                let profile = profile.borrow::<Profile>()?.0;
                if matches!(sweep, ProfileSweep::Revolve { .. }) {
                    state.borrow().profiles[profile]
                        .validate_revolve()
                        .map_err(mlua::Error::RuntimeError)?;
                }
                push_operation(&state, Operation::Profile { profile, sweep })
            })
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    }
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
    for name in ["sphere", "capsule"] {
        let state = Rc::clone(&recipe);
        cad.set(name, lua.create_function(move |_, options: Table| {
            let fields: &[&str] = if name == "sphere" { &["radius", "x", "y", "z"] } else { &["radius", "height", "x", "y", "z"] };
            options_fields(&options, fields)?;
            let radius = dimension(&options, "radius")?;
            if radius > 500.0 { return Err(mlua::Error::runtime("Rounded primitive radius must not exceed 500 mm")); }
            let origin = coordinates(&options)?;
            let operation = if name == "sphere" { Operation::Sphere { origin, radius } } else {
                let height = dimension(&options, "height")?;
                if height < radius * 2.0 { return Err(mlua::Error::runtime("Capsule height includes both caps and must be at least twice its radius")); }
                Operation::Capsule { origin, radius, height }
            };
            let (bottom, top) = match operation {
                Operation::Sphere { .. } => (origin[2] - radius, origin[2] + radius),
                Operation::Capsule { height, .. } => (origin[2], origin[2] + height),
                _ => unreachable!(),
            };
            if [origin[0] - radius, origin[0] + radius, origin[1] - radius, origin[1] + radius, bottom, top].iter().any(|value| value.abs() > 2000.0) {
                return Err(mlua::Error::runtime("Rounded primitive bounds must remain within ±2000 mm"));
            }
            push_operation(&state, operation)
        }).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    }
    for name in ["translate", "rotate", "fillet", "chamfer"] {
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
                    "fillet" | "chamfer" => {
                        let treatment = if name == "fillet" {
                            options_fields(&options, &["radius", "edges"])?;
                            EdgeTreatment::Fillet { radius_mm: dimension(&options, "radius")? }
                        } else {
                            options_fields(&options, &["width", "edges"])?;
                            EdgeTreatment::Chamfer { width_mm: dimension(&options, "width")? }
                        };
                        let supplied_edges = options.get::<Option<Table>>("edges")?;
                        let mut recipe = state.borrow_mut();
                        if recipe.edge_treatments.len() >= OPERATION_COUNT_MAX {
                            return Err(mlua::Error::RuntimeError("Model exceeds 256 edge treatments".into()));
                        }
                        let edges = if let Some(edges) = supplied_edges {
                            let count = edges.raw_len();
                            if !(1..=crate::edge_reference::COUNT_MAX).contains(&count) {
                                return Err(mlua::Error::RuntimeError(format!("{name} edges needs 1–64 references; omit edges to treat all eligible edges")));
                            }
                            let mut entry_count = 0;
                            for pair in edges.clone().pairs::<mlua::Value, mlua::Value>() {
                                let (key, _) = pair?;
                                if !matches!(key, mlua::Value::Integer(index) if index >= 1 && index as usize <= count) {
                                    return Err(mlua::Error::RuntimeError(format!("{name} edges must be a dense array")));
                                }
                                entry_count += 1;
                            }
                            if entry_count != count { return Err(mlua::Error::RuntimeError(format!("{name} edges must be a dense array"))); }
                            let mut selected = Vec::with_capacity(count);
                            for index in 1..=count {
                                let reference = edges.raw_get::<AnyUserData>(index)?.borrow::<Edge>()?.0;
                                if recipe.edges[reference].solid != source { return Err(mlua::Error::RuntimeError(format!("Edge '{}' belongs to a different source solid; select it on the solid passed to cad.{name}", recipe.edges[reference].name))); }
                                if selected.contains(&reference) { return Err(mlua::Error::RuntimeError(format!("{name} edges must not repeat a reference"))); }
                                selected.push(reference);
                            }
                            EdgeTargets::Selected(selected)
                        } else { EdgeTargets::All };
                        let index = recipe.edge_treatments.len();
                        recipe.edge_treatments.push(EdgeTreatmentRecipe { treatment, edges });
                        Modification::EdgeTreatment(index)
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
        lua.create_function(move |lua, (name, solid, options): (String, AnyUserData, Option<Table>)| {
            if name.is_empty() || name.len() > 80 || name.chars().any(char::is_control) {
                return Err(mlua::Error::RuntimeError(
                    "Output name must contain 1–80 bytes without control characters".into(),
                ));
            }
            let mut material = crate::material::Material::default();
            let mut parent = None;
            let mut shading = crate::shading::Shading::default();
            let mut assembly = crate::placement::Placement::default();
            let mut print = None;
            if let Some(options) = options {
                options_fields(&options, &["material", "assembly", "print", "shading", "parent"])?;
                parent = crate::assembly::parent(&options)?;
                if let Some(options) = options.get::<Option<Table>>("shading")? { shading = crate::shading::Shading::from_table(options)?; }
                if let Some(options) = options.get::<Option<Table>>("assembly")? { assembly = crate::placement::Placement::from_table(options)?; }
                if let Some(options) = options.get::<Option<Table>>("print")? { print = Some(crate::placement::Placement::from_table(options)?); }
                if let Some(value) = options.get::<Option<AnyUserData>>("material")? {
                    material = value.borrow::<crate::material::Material>()?.clone();
                }
            }
            let solid = solid.borrow::<Solid>()?.0;
            let mut state = state.borrow_mut();
            if state.outputs.len() >= PART_COUNT_MAX {
                return Err(mlua::Error::RuntimeError(
                    "At most 32 cad.show parts are supported".into(),
                ));
            }
            if state.outputs.iter().any(|part| part.name == name) {
                return Err(mlua::Error::RuntimeError("Part names must be unique".into()));
            }
            if state.outputs.iter().any(|part| part.solid == solid) {
                return Err(mlua::Error::RuntimeError("Each shown part must have its own solid; translate a copy for repeated parts".into()));
            }
            let line = crate::project::source_line(lua);
            state.outputs.push(Part { parent, assembly, print, line, name, material, shading, solid });
            Ok(())
        })
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    crate::material::register(&lua, &cad).map_err(|error| error.to_string())?;
    let definitions = crate::dimensions::register(&lua, &cad).map_err(|error| error.to_string())?;
    let state = Rc::clone(&recipe);
    let control_count = std::cell::Cell::new(0_usize);
    cad.set(
        "skin",
        lua.create_function(move |_, options: Table| {
            if state.borrow().operations.len() >= OPERATION_COUNT_MAX {
                return Err(mlua::Error::runtime("Model exceeds 256 operations"));
            }
            let remaining = crate::shape_operation::skin::CONTROL_COUNT_MAX - control_count.get();
            let (skin, count) = crate::shape_operation::skin::Skin::from_table(options, remaining)?;
            control_count.set(control_count.get() + count);
            let mut recipe = state.borrow_mut();
            let index = recipe.shapes.len();
            recipe
                .shapes
                .push(crate::shape_operation::Recipe::Skin(skin));
            drop(recipe);
            push_operation(&state, Operation::Shape(index))
        })
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let state = Rc::clone(&recipe);
    cad.set(
        "sweep",
        lua.create_function(move |_, (profile, options): (AnyUserData, Table)| {
            options_fields(&options, &["path", "bend_radius"])?;
            let profile = crate::shape_operation::SweepProfile::from_profile(
                &state.borrow().profiles[profile.borrow::<Profile>()?.0],
            )?;
            let path = crate::shape_operation::path(
                options.get("path")?,
                &profile,
                options.get("bend_radius")?,
            )?;
            let mut recipe = state.borrow_mut();
            let index = recipe.shapes.len();
            recipe
                .shapes
                .push(crate::shape_operation::Recipe::Sweep { profile, path });
            drop(recipe);
            push_operation(&state, Operation::Shape(index))
        })
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let state = Rc::clone(&recipe);
    cad.set(
        "loft",
        lua.create_function(move |_, (sections, options): (Table, Option<Table>)| {
            let count = crate::shape_operation::array_count(&sections, 2, 16)?;
            let degree = if let Some(options) = options {
                options_fields(&options, &["degree"])?;
                options.get::<Option<f64>>("degree")?.unwrap_or(1.0)
            } else {
                1.0
            };
            if !(1.0..=3.0).contains(&degree) || degree.fract() != 0.0 || degree as usize >= count {
                return Err(mlua::Error::runtime(
                    "Loft degree must be 1–3 with at least degree + 1 sections",
                ));
            }
            let degree = degree as u32;
            let mut values = Vec::<crate::shape_operation::Section>::with_capacity(count);
            for index in 1..=count {
                let section: Table = sections.raw_get(index)?;
                options_fields(&section, &["profile", "x", "y", "z"])?;
                let position = crate::shape_operation::position(&section, false)?;
                let profile = section
                    .get::<AnyUserData>("profile")?
                    .borrow::<Profile>()?
                    .0;
                let profile = state.borrow().profiles[profile].clone();
                if let Some(previous) = values.last() {
                    if position[2] - previous.position[2] < 0.01 {
                        return Err(mlua::Error::runtime(
                            "Loft section heights must strictly increase by at least 0.01 mm",
                        ));
                    }
                }
                crate::shape_operation::validate_loft_profile(
                    &profile,
                    values.last().map(|section| &section.profile),
                )?;
                values.push(crate::shape_operation::Section { profile, position });
            }
            let mut recipe = state.borrow_mut();
            let index = recipe.shapes.len();
            recipe.shapes.push(crate::shape_operation::Recipe::Loft {
                sections: values,
                degree,
            });
            drop(recipe);
            push_operation(&state, Operation::Shape(index))
        })
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let state = Rc::clone(&recipe);
    let faces = Rc::clone(&definitions);
    cad.set(
        "shell",
        lua.create_function(move |_, (solid, options): (AnyUserData, Table)| {
            options_fields(&options, &["thickness", "open"])?;
            let source = solid.borrow::<Solid>()?.0;
            let thickness = dimension(&options, "thickness")?;
            let opening = options
                .get::<Option<AnyUserData>>("open")?
                .map(|face| {
                    faces
                        .borrow_mut()
                        .modeling_face(face.borrow::<crate::dimensions::Face>()?.0, source)
                })
                .transpose()?;
            let mut recipe = state.borrow_mut();
            let index = recipe.shells.len();
            recipe
                .shells
                .push(crate::shape_operation::Shell { thickness, opening });
            drop(recipe);
            push_operation(
                &state,
                Operation::Modify {
                    source,
                    modification: Modification::Shell(index),
                },
            )
        })
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let state = Rc::clone(&recipe);
    let faces = Rc::clone(&definitions);
    cad.set(
        "hole",
        lua.create_function(move |_, (solid, options): (AnyUserData, Table)| {
            options_fields(
                &options,
                &[
                    "face",
                    "u",
                    "v",
                    "radius",
                    "depth",
                    "counterbore",
                    "countersink",
                ],
            )?;
            let source = solid.borrow::<Solid>()?.0;
            let face_index = options
                .get::<AnyUserData>("face")?
                .borrow::<crate::dimensions::Face>()?
                .0;
            let radius = dimension(&options, "radius")?;
            let depth = if options.contains_key("depth")? {
                Some(dimension(&options, "depth")?)
            } else {
                None
            };
            let counterbore = options.get::<Option<Table>>("counterbore")?;
            let countersink = options.get::<Option<Table>>("countersink")?;
            if counterbore.is_some() && countersink.is_some() {
                return Err(mlua::Error::runtime(
                    "Use counterbore or countersink, not both",
                ));
            }
            let recess = if let Some(options) = counterbore {
                options_fields(&options, &["radius", "depth"])?;
                Some(crate::hole::Recess::Counterbore {
                    radius: dimension(&options, "radius")?,
                    depth: dimension(&options, "depth")?,
                })
            } else if let Some(options) = countersink {
                options_fields(&options, &["radius", "angle"])?;
                let angle = options.get::<Option<f64>>("angle")?.unwrap_or(90.0);
                if !angle.is_finite() || !(1.0..=179.0).contains(&angle) {
                    return Err(mlua::Error::runtime(
                        "Countersink included angle must be between 1 and 179 degrees",
                    ));
                }
                Some(crate::hole::Recess::Countersink {
                    radius: dimension(&options, "radius")?,
                    angle,
                })
            } else {
                None
            };
            if let Some(recess) = recess {
                let outer_radius = match recess {
                    crate::hole::Recess::Counterbore { radius, .. }
                    | crate::hole::Recess::Countersink { radius, .. } => radius,
                };
                if outer_radius <= radius {
                    return Err(mlua::Error::runtime(
                        "Recess radius must exceed hole radius",
                    ));
                }
                let recess_depth = recess.depth(radius);
                if !recess_depth.is_finite() || recess_depth > 1000.0 {
                    return Err(mlua::Error::runtime("Recess depth must not exceed 1000 mm"));
                }
                if let Some(depth) = depth {
                    if recess_depth > depth {
                        return Err(mlua::Error::runtime(
                            "Recess depth must not exceed blind hole depth",
                        ));
                    }
                }
            }
            let u = options.get::<Option<f64>>("u")?.unwrap_or(0.0);
            let v = options.get::<Option<f64>>("v")?.unwrap_or(0.0);
            if !u.is_finite() || !v.is_finite() || u.abs() > 2000.0 || v.abs() > 2000.0 {
                return Err(mlua::Error::runtime(
                    "Hole offsets must be finite and within ±2000 mm",
                ));
            }
            let face = faces.borrow_mut().modeling_face(face_index, source)?;
            let index = {
                let mut recipe = state.borrow_mut();
                if recipe.holes.len() >= OPERATION_COUNT_MAX {
                    return Err(mlua::Error::runtime("Model exceeds 256 holes"));
                }
                let index = recipe.holes.len();
                recipe.holes.push(crate::hole::Recipe {
                    face,
                    offset: [u, v],
                    radius,
                    depth,
                    recess,
                });
                index
            };
            push_operation(
                &state,
                Operation::Modify {
                    source,
                    modification: Modification::Hole(index),
                },
            )
        })
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    for (name, kind) in [
        ("pocket", crate::face_feature::Kind::Pocket),
        ("boss", crate::face_feature::Kind::Boss),
    ] {
        let state = Rc::clone(&recipe);
        let faces = Rc::clone(&definitions);
        cad.set(
            name,
            lua.create_function(move |_, (solid, options): (AnyUserData, Table)| {
                options_fields(
                    &options,
                    &["face", "width", "height", "profile", "depth", "u", "v"],
                )?;
                let source = solid.borrow::<Solid>()?.0;
                let face_index = options
                    .get::<AnyUserData>("face")?
                    .borrow::<crate::dimensions::Face>()?
                    .0;
                let shape = if options.contains_key("profile")? {
                    if options.contains_key("width")? || options.contains_key("height")? {
                        return Err(mlua::Error::runtime(
                            "Use profile or width/height, not both",
                        ));
                    }
                    let index = options
                        .get::<AnyUserData>("profile")?
                        .borrow::<Profile>()?
                        .0;
                    crate::face_feature::Shape::Profile(state.borrow().profiles[index].clone())
                } else {
                    crate::face_feature::Shape::Rectangle([
                        dimension(&options, "width")?,
                        dimension(&options, "height")?,
                    ])
                };
                let depth = dimension(&options, "depth")?;
                let offset = [
                    options.get::<Option<f64>>("u")?.unwrap_or(0.0),
                    options.get::<Option<f64>>("v")?.unwrap_or(0.0),
                ];
                if offset
                    .iter()
                    .any(|value| !value.is_finite() || value.abs() > 2000.0)
                {
                    return Err(mlua::Error::runtime(
                        "Feature offsets must be finite and within ±2000 mm",
                    ));
                }
                let face = faces.borrow_mut().modeling_face(face_index, source)?;
                let index = {
                    let mut recipe = state.borrow_mut();
                    if recipe.face_features.len() >= OPERATION_COUNT_MAX {
                        return Err(mlua::Error::runtime("Model exceeds 256 face features"));
                    }
                    let index = recipe.face_features.len();
                    recipe.face_features.push(crate::face_feature::Recipe {
                        face,
                        offset,
                        shape,
                        depth,
                        kind,
                    });
                    index
                };
                push_operation(
                    &state,
                    Operation::Modify {
                        source,
                        modification: Modification::FaceFeature(index),
                    },
                )
            })
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
    }
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
    if let Some(project) = project {
        project.install(&lua).map_err(|error| error.to_string())?;
    }
    lua.load(source)
        .set_name("@model.luau")
        .exec()
        .map_err(|error| error.to_string())?;
    let mut recipe = std::mem::take(&mut *recipe.borrow_mut());
    let mut parts = std::mem::take(&mut recipe.outputs);
    let groups = std::mem::take(&mut *groups.borrow_mut());
    let worlds = crate::assembly::world_placements(&groups)?;
    let options = *options.borrow();
    let orientation = options.placement()?;
    for part in &mut parts {
        if let Some(parent) = part.parent {
            let world = worlds.get(parent).ok_or("Part parent does not exist")?;
            part.assembly = world.composed(part.assembly)?;
        }
    }
    if parts.iter().any(|part| part.print.is_some()) {
        if let Some(part) = parts.iter().find(|part| part.print.is_none()) {
            return Err(format!(
                "Part '{}': declare a print placement for every part",
                part.name
            ));
        }
    }
    if options.rotation != [0.0; 3] {
        for part in &mut parts {
            part.print.get_or_insert(part.assembly);
            part.assembly = orientation.composed(part.assembly)?;
        }
    }
    let first = parts
        .first()
        .ok_or("Model must call cad.show(name, solid)")?;
    let name = if parts.len() == 1 {
        first.name.clone()
    } else {
        "Assembly".into()
    };
    let mut definitions = std::mem::take(&mut *definitions.borrow_mut());
    definitions.validate_parameters(analysis.take_parameters())?;
    let mut poses = poses.borrow_mut();
    poses.validate()?;
    Ok(Model {
        references: std::mem::take(&mut *references.borrow_mut()),
        poses: std::mem::take(&mut poses.poses),
        pose: poses.selected.take(),
        asset: asset.borrow_mut().take().unwrap_or_default(),
        options,
        groups,
        print_options: recipe.print_options.unwrap_or_default(),
        recipe: Rc::new(recipe),
        definitions,
        name,
        parts,
        analysis_ms,
        script_ms: started.elapsed().as_secs_f64() * 1000.0,
    })
}

pub(crate) fn options_fields(options: &Table, names: &[&str]) -> mlua::Result<()> {
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
    pub fn has_annotations(&self) -> bool {
        !self.definitions.is_empty()
    }

    pub fn annotations(
        &self,
        meshes: &[&crate::mesh::Mesh],
    ) -> Result<crate::dimensions::Annotations, String> {
        if self.definitions.is_empty() {
            return Ok(crate::dimensions::Annotations::default());
        }
        assert_eq!(meshes.len(), self.parts.len());
        if meshes.is_empty()
            || meshes.len() > PART_COUNT_MAX
            || meshes.iter().map(|mesh| mesh.vertices.len()).sum::<usize>() > 300000
        {
            return Err("Assembly exceeds its part or triangle budget".into());
        }
        let planes: Vec<_> = meshes
            .iter()
            .map(|mesh| crate::dimensions::planes(mesh))
            .collect();
        let mut first = 1;
        let outputs: Vec<_> = self
            .parts
            .iter()
            .zip(meshes)
            .zip(&planes)
            .map(|((part, mesh), planes)| {
                let faces = first..=first + mesh.face_count - 1;
                first += mesh.face_count;
                (part.solid, faces, part.assembly.matrix(), planes)
            })
            .collect();
        self.definitions.resolve(&outputs)
    }

    pub fn build<'kernel>(&self, kernel: &'kernel Kernel) -> Result<Vec<Body<'kernel>>, String> {
        assert!(!self.parts.is_empty());
        assert!(self.parts.len() <= PART_COUNT_MAX);
        let mut count = 0;
        let mut bodies = Vec::with_capacity(self.parts.len());
        for part in &self.parts {
            bodies.push(
                self.build_solid(kernel, part.solid, &mut count)
                    .map_err(|error| format!("Part '{}': {error}", part.name))?,
            );
        }
        Ok(bodies)
    }

    pub fn meshes(
        &self,
        bodies: &mut [Body<'_>],
        game: bool,
    ) -> Result<Vec<crate::mesh::Mesh>, String> {
        assert_eq!(bodies.len(), self.parts.len());
        let mut meshes = Vec::with_capacity(bodies.len());
        let mut count = 0;
        for (body, part) in bodies.iter_mut().zip(&self.parts) {
            let mut mesh = if game {
                body.game_mesh()?.mesh
            } else {
                body.mesh()?.mesh
            };
            count += mesh.vertices.len();
            if count > 300000 {
                return Err("Assembly exceeds its 100000 triangle budget".into());
            }
            part.shading.apply(&mut mesh);
            let color = crate::mesh::vertex_color(part.material.base_color);
            for vertex in &mut mesh.vertices {
                vertex.color = color;
            }
            meshes.push(mesh);
        }
        Ok(meshes)
    }

    fn build_solid<'kernel>(
        &self,
        kernel: &'kernel Kernel,
        output: usize,
        count: &mut usize,
    ) -> Result<Body<'kernel>, String> {
        self.build_solid_cached(kernel, output, count, None, None)
    }

    fn build_solid_cached<'kernel>(
        &self,
        kernel: &'kernel Kernel,
        output: usize,
        count: &mut usize,
        mut cache: Option<&mut cache::Operations<'kernel>>,
        preview: Option<&crate::cancellation::Token>,
    ) -> Result<Body<'kernel>, String> {
        assert!(output < self.recipe.operations.len());
        let mut pending = vec![(output, false, *count)];
        let mut bodies = Vec::<Body<'kernel>>::new();
        while let Some((index, visited, started)) = pending.pop() {
            if let Some(token) = preview {
                token.checkpoint()?;
            }
            let started = if visited { started } else { *count };
            if !visited {
                if let Some(cache) = cache.as_deref_mut() {
                    if let Some((body, steps)) = cache.find(&self.recipe, index)? {
                        *count += steps;
                        if *count > EVALUATION_COUNT_MAX {
                            return Err(
                                "Model exceeds 512 evaluation steps; reduce repeated subtrees"
                                    .into(),
                            );
                        }
                        bodies.push(body);
                        continue;
                    }
                }
            }
            *count += 1;
            if *count > EVALUATION_COUNT_MAX {
                return Err("Model exceeds 512 evaluation steps; reduce repeated subtrees".into());
            }
            match self.recipe.operations[index] {
                Operation::Shape(index) => {
                    bodies.push(match &self.recipe.shapes[index] {
                        crate::shape_operation::Recipe::Skin(skin) => kernel.skin(skin)?,
                        crate::shape_operation::Recipe::Sweep { profile, path } => {
                            kernel.path_sweep(profile, path)?
                        }
                        crate::shape_operation::Recipe::Loft { sections, degree } => {
                            kernel.loft(sections, *degree)?
                        }
                    });
                }
                Operation::Profile { profile, sweep } => {
                    let polygon = self
                        .recipe
                        .profiles
                        .get(profile)
                        .ok_or("Missing sweep profile")?;
                    bodies.push(kernel.sweep_curved_profile(polygon, sweep)?);
                }
                Operation::Sphere { origin, radius } => bodies.push(kernel.sphere(origin, radius)?),
                Operation::Capsule {
                    origin,
                    radius,
                    height,
                } => {
                    let mut body = kernel.capsule(radius, height)?;
                    if origin != [0.0; 3] {
                        body.translate(origin)?;
                    }
                    bodies.push(body);
                }
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
                            Modification::Shell(index) => {
                                let shell = &self.recipe.shells[index];
                                body.shell(shell.thickness, shell.opening.as_ref())?;
                            }
                            Modification::Translate(offset) => body.translate(offset)?,
                            Modification::Rotate { axis, angle } => body.rotate(axis, angle)?,
                            Modification::Hole(index) => {
                                self.recipe.holes[index].apply(kernel, &mut body)?
                            }
                            Modification::FaceFeature(index) => {
                                self.recipe.face_features[index].apply(kernel, &mut body)?
                            }
                            Modification::EdgeTreatment(index) => {
                                let operation = &self.recipe.edge_treatments[index];
                                match &operation.edges {
                                    EdgeTargets::All => {
                                        body.apply_edge_treatment(operation.treatment, None)?
                                    }
                                    EdgeTargets::Selected(indices) => {
                                        let references: Vec<_> = indices
                                            .iter()
                                            .map(|index| &self.recipe.edges[*index])
                                            .collect();
                                        body.apply_edge_treatment(
                                            operation.treatment,
                                            Some(&references),
                                        )?;
                                    }
                                }
                            }
                        }
                        bodies.push(body);
                    } else {
                        pending.extend([(index, true, started), (source, false, 0)]);
                        continue;
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
                        pending.extend([
                            (index, true, started),
                            (second, false, 0),
                            (first, false, 0),
                        ]);
                        continue;
                    }
                }
            }
            if index != output {
                if let Some(cache) = cache.as_deref_mut() {
                    cache.insert(
                        &self.recipe,
                        index,
                        *count - started,
                        bodies.last().ok_or("Missing cached operation body")?,
                    )?;
                }
            }
        }
        assert_eq!(bodies.len(), 1);
        bodies.pop().ok_or("Model produced no solid".into())
    }
}

#[cfg(test)]
mod tests {
    use super::{Cache, Kernel, compile};
    #[test]
    fn skins_cap_warped_and_concave_sections_and_reuse_cached_geometry() -> Result<(), String> {
        let source = r#"
local sections={}
for level=0,1 do
    local section={}
    for _,p in {{x=0,y=0},{x=10,y=0},{x=10,y=10},{x=0,y=10}} do
        table.insert(section,{x=p.x,y=p.y,z=level*10+p.x*p.y*0.1})
    end
    table.insert(sections,section)
end
cad.show('skin',cad.skin({sections=sections}),{shading={mode='flat'}})
"#;
        let kernel = Kernel::new()?;
        let mut cache = Cache::new(&kernel);
        let model = cache.compile(source)?;
        let first = cache.prepare(&model)?;
        assert!((first.bodies[0].volume()? - 1000.0).abs() < 1e-6);
        first.geometry[0].mesh.validate()?;
        let mut body = first.bodies[0].try_clone()?;
        assert!(!body.step_data("skin")?.is_empty());
        let meshes = model.meshes(&mut model.build(&kernel)?, true)?;
        meshes[0].validate()?;
        assert!(!crate::game_asset::glb(&model, &meshes)?.is_empty());
        let repeated = cache.prepare(&model)?;
        assert_eq!(repeated.hits, 1);
        assert!(std::sync::Arc::ptr_eq(
            &first.geometry[0],
            &repeated.geometry[0]
        ));
        let moved = cache.compile(&source.replace(
            "shading={mode='flat'}",
            "shading={mode='flat'},assembly={position={x=20}}",
        ))?;
        let prepared = cache.prepare(&moved)?;
        assert_eq!(prepared.hits, 1);
        assert!(std::sync::Arc::ptr_eq(
            &first.geometry[0],
            &prepared.geometry[0]
        ));
        let changed = cache.compile(&source.replace("level*10", "level*15"))?;
        let prepared = cache.prepare(&changed)?;
        assert_eq!(prepared.hits, 0);
        assert!((prepared.bodies[0].volume()? - 1500.0).abs() < 1e-6);
        for replacement in [
            "{x=level*10+p.x*p.y*0.1,y=p.x,z=p.y}",
            "{x=p.y,y=level*10+p.x*p.y*0.1,z=p.x}",
            "{x=p.x+1990,y=p.y+1990,z=level*10+p.x*p.y*0.1+1970}",
        ] {
            let model = cache
                .compile(&source.replace("{x=p.x,y=p.y,z=level*10+p.x*p.y*0.1}", replacement))?;
            let prepared = cache.prepare(&model)?;
            assert!((prepared.bodies[0].volume()? - 1000.0).abs() < 1e-6);
            prepared.geometry[0].mesh.validate()?;
        }
        for (source, expected) in [
            (
                "local sections={} for level=0,1 do local section={} for _,p in {{x=0,y=0},{x=2,y=0},{x=2,y=1},{x=1,y=1},{x=1,y=2},{x=0,y=2}} do table.insert(section,{x=p.x,y=p.y,z=level*10}) end table.insert(sections,section) end cad.show('L',cad.skin({sections=sections}))",
                30.0,
            ),
            (
                "local sections={} for level=0,1 do local section={} for corner=0,7 do local a=corner*math.pi/4 local r=level==0 and 5 or 0 table.insert(section,{x=r*math.cos(a),y=r*math.sin(a),z=level*10}) end table.insert(sections,section) end cad.show('cone',cad.skin({sections=sections}))",
                8.0 * std::f64::consts::FRAC_PI_4.sin() * 25.0 * 10.0 / 6.0,
            ),
            (
                "local sections={} for sector=0,15 do local section={} local a=sector*math.pi/8 for _,p in {{r=8,z=-2},{r=12,z=-2},{r=12,z=2},{r=8,z=2}} do table.insert(section,{x=p.r*math.cos(a),y=p.r*math.sin(a),z=p.z}) end table.insert(sections,section) end cad.show('ring',cad.skin({sections=sections,closed=true}))",
                16.0 * std::f64::consts::FRAC_PI_8.sin() * (144.0 - 64.0) * 4.0 / 2.0,
            ),
        ] {
            let model = cache.compile(source)?;
            let prepared = cache.prepare(&model)?;
            assert!((prepared.bodies[0].volume()? - expected).abs() < 1e-6);
            prepared.geometry[0].mesh.validate()?;
            model.meshes(&mut model.build(&kernel)?, true)?[0].validate()?;
        }
        for bad in [
            source.replace("{sections=sections}", "{sections=sections,unknown=true}"),
            source.replace("{sections=sections}", "{sections=sections,closed=true}"),
            source.replace("level*10", "level*0"),
            source.replace("x=p.x", "x=0/0"),
            source.replace("x=p.x", "x=2001"),
            source.replace(
                "z=level*10+p.x*p.y*0.1",
                "z=level*10+p.x*p.y*0.1,unknown=true",
            ),
        ] {
            assert!(compile(&bad).is_err(), "{bad}");
        }
        Ok(())
    }

    #[test]
    fn faceted_revolves_close_poles_preserve_volume_and_cache_segment_counts() -> Result<(), String>
    {
        let source = "local p=cad.polygon({{x=0,y=0},{x=5,y=0},{x=5,y=10},{x=0,y=10}}) cad.show('prism',cad.revolve(p,{segments=8}),{shading={mode='flat'}})";
        let kernel = Kernel::new()?;
        let mut cache = Cache::new(&kernel);
        let model = cache.compile(source)?;
        let first = cache.prepare(&model)?;
        let expected = 8.0 * std::f64::consts::FRAC_PI_4.sin() * 25.0 * 10.0 / 2.0;
        assert!((first.bodies[0].volume()? - expected).abs() < 1e-6);
        first.geometry[0].mesh.validate()?;
        let mut body = first.bodies[0].try_clone()?;
        assert!(!body.step_data("faceted")?.is_empty());
        let meshes = model.meshes(&mut model.build(&kernel)?, true)?;
        meshes[0].validate()?;
        assert!(!crate::game_asset::glb(&model, &meshes)?.is_empty());
        let repeated = cache.prepare(&model)?;
        assert_eq!(repeated.hits, 1);
        assert!(std::sync::Arc::ptr_eq(
            &first.geometry[0],
            &repeated.geometry[0]
        ));
        let changed = cache.compile(&source.replace("segments=8", "segments=12"))?;
        let second = cache.prepare(&changed)?;
        assert_eq!(second.hits, 0);
        assert!(!std::sync::Arc::ptr_eq(
            &first.geometry[0],
            &second.geometry[0]
        ));
        let sphere_volume = (0..8)
            .map(|index| {
                let start = std::f64::consts::PI * f64::from(index) / 8.0;
                let end = std::f64::consts::PI * f64::from(index + 1) / 8.0;
                let a = 5.0 * start.sin();
                let b = 5.0 * end.sin();
                let height = 5.0 * (start.cos() - end.cos());
                16.0 * std::f64::consts::FRAC_PI_8.sin() / 6.0 * height * (a * a + a * b + b * b)
            })
            .sum::<f64>();
        for (source, volume) in [
            (
                "local p=cad.polygon({{x=0,y=-5},{x=5,y=0},{x=0,y=5}}) cad.show('bicone',cad.revolve(p,{segments=16}))",
                16.0 * std::f64::consts::FRAC_PI_8.sin() * 25.0 * 10.0 / 6.0,
            ),
            (
                "local p=cad.polygon({{x=2,y=0},{x=5,y=0},{x=5,y=10},{x=2,y=10}}) cad.show('tube',cad.revolve(p,{segments=8}))",
                expected * 21.0 / 25.0,
            ),
            (
                "local points={} for i=0,8 do local a=i*math.pi/8 table.insert(points,{x=5*math.sin(a),y=-5*math.cos(a)}) end cad.show('head',cad.revolve(cad.polygon(points),{segments=16}))",
                sphere_volume,
            ),
            (
                "local p=cad.polygon({{x=0,y=0},{x=5,y=0},{x=5,y=10},{x=0,y=10}}) cad.show('many facets',cad.revolve(p,{segments=128}))",
                128.0 * (std::f64::consts::TAU / 128.0).sin() * 25.0 * 10.0 / 2.0,
            ),
        ] {
            let model = cache.compile(source)?;
            let prepared = cache.prepare(&model)?;
            prepared.geometry[0].mesh.validate()?;
            assert!((prepared.bodies[0].volume()? - volume).abs() < 1e-6);
            model.meshes(&mut model.build(&kernel)?, true)?[0].validate()?;
        }
        for options in [
            "{segments=2}",
            "{segments=129}",
            "{segments=3.5}",
            "{segments=0/0}",
            "{segments=8,angle=180}",
        ] {
            assert!(compile(&source.replace("{segments=8}", options)).is_err());
        }
        assert!(
            compile("cad.show('bad',cad.revolve(cad.circle({radius=5}),{segments=8}))").is_err()
        );
        Ok(())
    }

    #[test]
    fn rounded_primitives_have_exact_volume_closed_meshes_and_cached_geometry() -> Result<(), String>
    {
        let kernel = Kernel::new()?;
        let mut cache = Cache::new(&kernel);
        for (shape, expected) in [
            (
                "cad.sphere({radius=5})",
                4.0 / 3.0 * std::f64::consts::PI * 125.0,
            ),
            (
                "cad.capsule({radius=5,height=20})",
                std::f64::consts::PI * (250.0 + 500.0 / 3.0),
            ),
            (
                "cad.capsule({radius=5,height=10})",
                4.0 / 3.0 * std::f64::consts::PI * 125.0,
            ),
        ] {
            let source = format!("cad.show('rounded',{shape})");
            let model = cache.compile(&source)?;
            let prepared = cache.prepare(&model)?;
            assert!((prepared.bodies[0].volume()? - expected).abs() < 1e-4);
            prepared.geometry[0].mesh.validate()?;
            let mut working = prepared.bodies[0].try_clone()?;
            assert!(!working.step_data("rounded")?.is_empty());
            let repeated = cache.prepare(&model)?;
            assert_eq!(repeated.hits, 1);
            assert!(std::sync::Arc::ptr_eq(
                &prepared.geometry[0],
                &repeated.geometry[0]
            ));
            let mut bodies = model.build(&kernel)?;
            model.meshes(&mut bodies, true)?[0].validate()?;
        }
        for shape in [
            "cad.sphere({radius=0})",
            "cad.sphere({radius=501})",
            "cad.sphere({radius=0/0})",
            "cad.capsule({radius=5,height=9})",
            "cad.capsule({radius=5,height=1/0})",
            "cad.sphere({radius=5,height=20})",
            "cad.sphere({radius=5,x=1998})",
            "cad.capsule({radius=5,height=20,z=1990})",
        ] {
            assert!(
                compile(&format!("cad.show('bad',{shape})")).is_err(),
                "{shape}"
            );
        }
        Ok(())
    }

    #[test]
    fn touching_parts_preserve_faces_colors_edges_and_volume() -> Result<(), String> {
        let source = "local a=cad.box({width=10,depth=10,height=10});local b=cad.translate(a,{x=10});cad.show('red',a,{material = cad.material('Surface', {base_color = {r=1,g=0,b=0}})});cad.show('blue',b,{material = cad.material('Surface', {base_color = {r=0,g=0,b=1}})})";
        let snapshot = crate::build_script_snapshot(source, 1)?;
        assert_eq!(snapshot.parts.len(), 2);
        assert_eq!(snapshot.parts[0].name, "red");
        assert_eq!(snapshot.parts[1].name, "blue");
        assert_eq!(snapshot.scene.world_mesh().face_count, 12);
        assert_eq!(snapshot.scene.faces.len(), 12);
        assert_eq!(snapshot.scene.world_edges().items.len(), 24);
        assert!((snapshot.volume_mm3 - 2000.0).abs() < 1e-6);
        assert_eq!(
            snapshot.scene.bounds,
            [glam::Vec3::ZERO, glam::vec3(20.0, 10.0, 10.0)]
        );
        for vertex in &snapshot.scene.world_mesh().vertices {
            let index = snapshot
                .scene
                .display
                .parts
                .iter()
                .position(|part| part.faces.contains(&vertex.face_id))
                .expect("Part owns face");
            assert_eq!(
                vertex.color,
                crate::mesh::vertex_color(snapshot.parts[index].material.base_color)
            );
            assert!(snapshot.scene.faces.contains_key(&vertex.face_id));
        }
        for edge in &snapshot.scene.world_edges().items {
            assert!(edge.segments[0] < edge.segments[1]);
            assert!(edge.segments[1] as usize <= snapshot.scene.world_edges().segments.len());
        }
        assert!(snapshot.scene.world_mesh().validate().is_err());
        Ok(())
    }

    #[test]
    fn named_faces_resolve_on_their_parts_and_measure_across_parts() -> Result<(), String> {
        let source = "local a=cad.box({width=10,depth=10,height=10});local b=cad.translate(a,{z=20});local first=cad.face(a,{name='first top',normal='z+'});local second=cad.face(b,{name='second top',normal='z+'});cad.measure('top separation',{from=first,to=second,min=19,max=21});cad.show('lower',a);cad.show('upper',b)";
        let snapshot = crate::build_script_snapshot(source, 1)?;
        assert_eq!(snapshot.scene.annotations.faces.len(), 2);
        assert!(
            snapshot.scene.display.parts[0]
                .faces
                .contains(&snapshot.scene.annotations.faces[0].id)
        );
        assert!(
            snapshot.scene.display.parts[1]
                .faces
                .contains(&snapshot.scene.annotations.faces[1].id)
        );
        assert_eq!(snapshot.scene.annotations.measurements[0].distance_mm, 20.0);
        assert!(snapshot.scene.annotations.measurements[0].checked);
        let error = crate::build_script_snapshot(&source.replace("max=21", "max=19"), 1)
            .err()
            .expect("Failed cross-part check");
        assert!(error.contains("top separation"), "{error}");
        Ok(())
    }

    #[test]
    fn part_definitions_reject_invalid_colors_duplicates_and_excess_parts() {
        let prefix =
            "local a=cad.box({width=10,depth=10,height=10});local b=cad.translate(a,{x=20});";
        for suffix in [
            "cad.show('same',a);cad.show('same',b)",
            "cad.show('first',a);cad.show('second',a)",
            "cad.show('bad',a,{material = cad.material('Surface', {base_color = {r=-0.1,g=0,b=0}})})",
            "cad.show('bad',a,{material = cad.material('Surface', {base_color = {r=1.1,g=0,b=0}})})",
            "cad.show('bad',a,{material = cad.material('Surface', {base_color = {r=0/0,g=0,b=0}})})",
            "cad.show('bad',a,{material = cad.material('Surface', {base_color = {r=1/0,g=0,b=0}})})",
        ] {
            assert!(
                super::compile(&format!("{prefix}{suffix}")).is_err(),
                "{suffix}"
            );
        }
        let mut source = prefix.to_string();
        for index in 0..33 {
            source.push_str(&format!(
                "cad.show('part {index}',cad.translate(a,{{x={index}}}));"
            ));
        }
        let error = super::compile(&source).err().expect("Part limit");
        assert!(error.contains("32"), "{error}");
    }

    #[test]
    fn evaluation_budget_is_shared_by_all_parts() -> Result<(), String> {
        let mut source = "local a=cad.box({width=10,depth=10,height=10});".to_string();
        for _ in 0..20 {
            source.push_str("a=cad.translate(a,{x=1});");
        }
        for index in 0..32 {
            source.push_str(&format!(
                "cad.show('part {index}',cad.translate(a,{{y={index}}}));"
            ));
        }
        let model = super::compile(&source)?;
        let kernel = crate::kernel::Kernel::new()?;
        let error = model.build(&kernel).err().expect("Total evaluation limit");
        assert!(error.contains("512 evaluation"), "{error}");
        Ok(())
    }

    #[test]
    fn options_can_call_cad_functions_without_reentrant_recipe_borrows() -> Result<(), String> {
        let source = "local a=cad.box({width=10,depth=10,height=10});local options:any=setmetatable({},{__index=function(_,key) if key=='material' then cad.show('nested',cad.translate(a,{x=20}));return cad.material('red',{base_color={r=1,g=0,b=0}}) end;return nil end});cad.show('outer',a,options)";
        assert_eq!(crate::build_script_snapshot(source, 1)?.parts.len(), 2);
        let source = "local point:any=setmetatable({y=0},{__index=function(_,key) if key=='x' then cad.polygon({{x=0,y=0},{x=1,y=0},{x=0,y=1}});return 0 end;return nil end});local profile=cad.polygon({point,{x=10,y=0},{x=0,y=10}});cad.show('profile',cad.extrude(profile,{height=2}))";
        assert!((crate::build_script_snapshot(source, 1)?.volume_mm3 - 100.0).abs() < 0.001);
        let source = "local part=cad.box({width=40,depth=20,height=8});local options:any=setmetatable({radius=2},{__index=function(_,key) if key=='edges' then return {cad.edge(part,{name='corner',from={x=40,y=0,z=0},to={x=40,y=0,z=8}})} end;return nil end});cad.show('fillet',cad.fillet(part,options))";
        for source in [
            source.to_owned(),
            source
                .replace("radius=2", "width=2")
                .replace("cad.fillet", "cad.chamfer"),
        ] {
            assert!(crate::build_script_snapshot(&source, 1)?.volume_mm3 > 0.0);
        }
        Ok(())
    }
}
