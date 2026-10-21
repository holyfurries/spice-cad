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
    pub assembly: crate::placement::Placement,
    pub print: Option<crate::placement::Placement>,
    pub line: usize,
    pub name: String,
    pub color: [f32; 3],
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
    face_features: Vec<crate::face_feature::Recipe>,
    holes: Vec<crate::hole::Recipe>,
    edges: Vec<crate::edge_reference::Reference>,
    edge_treatments: Vec<EdgeTreatmentRecipe>,
    profiles: Vec<crate::profile::Profile>,
    operations: Vec<Operation>,
    outputs: Vec<Part>,
}

pub struct Model {
    recipe: Rc<Recipe>,
    definitions: crate::dimensions::Definitions,
    pub name: String,
    pub parts: Vec<Part>,
    pub analysis_ms: f64,
    pub script_ms: f64,
}

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

pub fn compile(source: &str) -> Result<Model, String> {
    compile_cached(source, &mut crate::analysis::Cache::default())
}

pub fn compile_cached(
    source: &str,
    analysis: &mut crate::analysis::Cache,
) -> Result<Model, String> {
    let started = Instant::now();
    analysis.validate(source)?;
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
    crate::parameters::register(&lua, &cad).map_err(|error| error.to_string())?;
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
    let state = Rc::clone(&recipe);
    cad.set(
        "polygon",
        lua.create_function(move |_, points: Table| {
            let polygon = crate::profile::Polygon::from_table(points)?;
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
                    options_fields(&options, &["angle"])?;
                    let angle_deg = options.get::<Option<f64>>("angle")?.unwrap_or(360.0);
                    if !angle_deg.is_finite() || !(0.1..=360.0).contains(&angle_deg) {
                        return Err(mlua::Error::RuntimeError(
                            "Revolve angle must be finite and between 0.1 and 360 degrees".into(),
                        ));
                    }
                    ProfileSweep::Revolve { angle_deg }
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
            let mut color = crate::mesh::DEFAULT_COLOR;
            let mut assembly = crate::placement::Placement::default();
            let mut print = None;
            if let Some(options) = options {
                options_fields(&options, &["color", "assembly", "print"])?;
                if let Some(options) = options.get::<Option<Table>>("assembly")? { assembly = crate::placement::Placement::from_table(options)?; }
                if let Some(options) = options.get::<Option<Table>>("print")? { print = Some(crate::placement::Placement::from_table(options)?); }
                if let Some(rgb) = options.get::<Option<Table>>("color")? {
                    options_fields(&rgb, &["r", "g", "b"])?;
                    for (index, key) in ["r", "g", "b"].into_iter().enumerate() {
                        let value: f64 = rgb.get(key)?;
                        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                            return Err(mlua::Error::RuntimeError("Part color channels must be finite and between 0 and 1".into()));
                        }
                        color[index] = value as f32;
                    }
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
            let line = lua.inspect_stack(1, |debug| debug.current_line()).flatten().unwrap_or(1).saturating_sub(1);
            state.outputs.push(Part { assembly, print, line, name, color, solid });
            Ok(())
        })
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let definitions = crate::dimensions::register(&lua, &cad).map_err(|error| error.to_string())?;
    let state = Rc::clone(&recipe);
    cad.set(
        "sweep",
        lua.create_function(move |_, (profile, options): (AnyUserData, Table)| {
            options_fields(&options, &["path"])?;
            let polygon = state.borrow().profiles[profile.borrow::<Profile>()?.0]
                .polygon()
                .map_err(mlua::Error::runtime)?
                .clone();
            let path = crate::shape_operation::path(options.get("path")?, &polygon)?;
            let mut recipe = state.borrow_mut();
            let index = recipe.shapes.len();
            recipe
                .shapes
                .push(crate::shape_operation::Recipe::Sweep { polygon, path });
            drop(recipe);
            push_operation(&state, Operation::Shape(index))
        })
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let state = Rc::clone(&recipe);
    cad.set(
        "loft",
        lua.create_function(move |_, sections: Table| {
            let count = crate::shape_operation::array_count(&sections, 2, 16)?;
            let mut values = Vec::<crate::shape_operation::Section>::with_capacity(count);
            for index in 1..=count {
                let section: Table = sections.raw_get(index)?;
                options_fields(&section, &["profile", "x", "y", "z"])?;
                let position = crate::shape_operation::position(&section, false)?;
                let polygon = state.borrow().profiles[section
                    .get::<AnyUserData>("profile")?
                    .borrow::<Profile>()?
                    .0]
                    .polygon()
                    .map_err(mlua::Error::runtime)?
                    .clone();
                if let Some(previous) = values.last() {
                    if position[2] - previous.position[2] < 0.01 {
                        return Err(mlua::Error::runtime(
                            "Loft section heights must strictly increase by at least 0.01 mm",
                        ));
                    }
                    if polygon.points.len() != previous.polygon.points.len() {
                        return Err(mlua::Error::runtime(
                            "Loft profiles must have equal vertex counts and matching vertex order",
                        ));
                    }
                }
                crate::shape_operation::validate_loft_polygon(
                    &polygon,
                    values.last().map(|section| &section.polygon),
                )?;
                values.push(crate::shape_operation::Section { polygon, position });
            }
            let mut recipe = state.borrow_mut();
            let index = recipe.shapes.len();
            recipe
                .shapes
                .push(crate::shape_operation::Recipe::Loft(values));
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
    lua.load(source)
        .set_name("@model.luau")
        .exec()
        .map_err(|error| error.to_string())?;
    let mut recipe = std::mem::take(&mut *recipe.borrow_mut());
    let parts = std::mem::take(&mut recipe.outputs);
    if parts.iter().any(|part| part.print.is_some()) {
        if let Some(part) = parts.iter().find(|part| part.print.is_none()) {
            return Err(format!(
                "Part '{}': declare a print placement for every part",
                part.name
            ));
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
    Ok(Model {
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
            mesh.smooth_normals();
            let color = crate::mesh::vertex_color(part.color);
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
                        crate::shape_operation::Recipe::Sweep { polygon, path } => {
                            kernel.path_sweep(polygon, path)?
                        }
                        crate::shape_operation::Recipe::Loft(sections) => kernel.loft(sections)?,
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
    #[test]
    fn touching_parts_preserve_faces_colors_edges_and_volume() -> Result<(), String> {
        let source = "local a=cad.box({width=10,depth=10,height=10});local b=cad.translate(a,{x=10});cad.show('red',a,{color={r=1,g=0,b=0}});cad.show('blue',b,{color={r=0,g=0,b=1}})";
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
                crate::mesh::vertex_color(snapshot.parts[index].color)
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
            "cad.show('bad',a,{color={r=-0.1,g=0,b=0}})",
            "cad.show('bad',a,{color={r=1.1,g=0,b=0}})",
            "cad.show('bad',a,{color={r=0/0,g=0,b=0}})",
            "cad.show('bad',a,{color={r=1/0,g=0,b=0}})",
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
        let source = "local a=cad.box({width=10,depth=10,height=10});local options:any=setmetatable({},{__index=function(_,key) if key=='color' then cad.show('nested',cad.translate(a,{x=20}));return {r=1,g=0,b=0} end;return nil end});cad.show('outer',a,options)";
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
