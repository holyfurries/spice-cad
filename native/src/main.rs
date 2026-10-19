#![expect(
    clippy::collapsible_if,
    reason = "Repository guidelines require explicit nested control flow"
)]
use eframe::{egui, egui_wgpu};
use spice_rack::{PARAMETERS, RackParameters};
#[cfg(any(test, feature = "rack-tools"))]
use spice_rack::{build_rack, printing};
use std::{
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
#[cfg(test)]
mod allocations;
mod analysis;
mod code_editor;
mod diagnostics;
mod dimensions;
mod document;
mod edge_reference;
mod export;
mod files;
mod game_asset;
mod inline_output;
mod inspection;
mod interface;
mod kernel;
mod lsp;
mod mesh;
mod modal;
mod model;
mod palette;
mod parameters;
mod performance;
mod picking;
mod placement;
mod preview;
mod profile;
mod save;
mod search;
mod syntax;
mod text_buffer;
mod thumbnail;
mod viewport;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    let mut arguments: Vec<String> = std::env::args().skip(1).take(7).collect();
    let mut profile = arguments
        .iter()
        .position(|argument| argument == "--profile")
        .map(|index| {
            arguments.remove(index);
            started
        });
    let benchmarking = arguments.as_slice() == ["--profile-preview"];
    if benchmarking {
        profile = Some(started);
    }
    let completed = benchmarking.then(|| Arc::new(std::sync::atomic::AtomicBool::new(false)));
    let sweep = completed
        .as_ref()
        .map(|completed| performance::Sweep::new(started, Arc::clone(completed)));
    #[cfg(feature = "rack-tools")]
    if arguments.as_slice() == ["--benchmark"] {
        return benchmark().map_err(Into::into);
    }
    let model_path = match arguments.as_slice() {
        [flag] if flag == "--profile-preview" => None,
        [] => Some(document::example_model(document::Example::RackParts)?),
        #[cfg(feature = "rack-tools")]
        [flag] if flag == "--rack" => None,
        [flag, path] if flag == "--model" => Some(std::path::PathBuf::from(path)),
        [flag, path] if flag == "--check-model" => {
            let source = model::read_source(std::path::Path::new(path))?;
            let snapshot = build_script_snapshot(&source, 1)?;
            println!(
                "{}: {:.3} mm³ · {} triangles · analyze {:.1} ms · script {:.1} ms · kernel {:.1} ms · mesh {:.1} ms · total {:.1} ms",
                snapshot.name,
                snapshot.volume_mm3,
                snapshot.scene.display.vertex_count() / 3,
                snapshot.analysis_ms,
                snapshot.script_ms,
                snapshot.build_ms,
                snapshot.tessellation_ms,
                snapshot.total_ms
            );
            return Ok(());
        }
        [flag, path, folder] if flag == "--export-model" || flag == "--export-game" => {
            let source = model::read_source(std::path::Path::new(path))?;
            let target = if flag == "--export-game" {
                export::Target::Game
            } else {
                export::Target::Print
            };
            export::model_files(&source, std::path::Path::new(folder), target)?;
            println!("Exported {} to {folder}", target.description());
            return Ok(());
        }
        _ => {
            return Err(
                (if cfg!(feature = "rack-tools") {
                    "Usage: spice-cad [--profile-preview | [--profile] [--rack | --benchmark | --model FILE.luau | --check-model FILE.luau | --export-model FILE.luau NEW_DIRECTORY | --export-game FILE.luau NEW_DIRECTORY]]"
                } else {
                    "Usage: spice-cad [--profile-preview | [--profile] [--model FILE.luau | --check-model FILE.luau | --export-model FILE.luau NEW_DIRECTORY | --export-game FILE.luau NEW_DIRECTORY]]"
                })
                    .into(),
            );
        }
    };
    let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    let device_descriptor = Arc::clone(&setup.device_descriptor);
    setup.device_descriptor = Arc::new(move |adapter| {
        performance::stage(profile, "adapter_selected");
        device_descriptor(adapter)
    });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_active(!benchmarking)
            .with_resizable(!benchmarking)
            .with_inner_size(if benchmarking {
                [1200.0, 650.0]
            } else {
                [1400.0, 900.0]
            })
            .with_min_inner_size([1100.0, 650.0]),
        renderer: eframe::Renderer::Wgpu,
        depth_buffer: 32,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            surface: egui_wgpu::SurfaceConfig::LOW_LATENCY,
            wgpu_setup: setup.into(),
            ..Default::default()
        },
        ..Default::default()
    };
    performance::stage(profile, "before_event_loop");
    eframe::run_native(
        "Spice CAD · native SMLib",
        options,
        Box::new(move |context| Ok(Box::new(Editor::new(context, model_path, profile, sweep)?))),
    )?;
    if completed.is_some_and(|completed| !completed.load(std::sync::atomic::Ordering::Acquire)) {
        return Err("Preview benchmark did not complete".into());
    }
    Ok(())
}

enum Command {
    ExportScript {
        target: export::Target,
        source: String,
        folder: std::path::PathBuf,
    },
    Script {
        input: Option<performance::Input>,
        source: ropey::Rope,
        editor_revision: u64,
        revision: u64,
        saved_at: Option<std::time::SystemTime>,
    },
    Build {
        parameters: RackParameters,
        revision: u64,
    },
    #[cfg(feature = "rack-tools")]
    Export { parameters: RackParameters },
}
enum Outcome {
    Model(Box<Snapshot>),
    Exported(String),
}
enum WorkerState {
    Idle,
    Busy(ModelRevision),
    Export,
    Unavailable,
}
#[derive(Clone, Copy)]
enum ModelRevision {
    Script(u64),
    Rack(RackParameters),
}

impl ModelRevision {
    fn current(&self, source: Option<u64>, parameters: RackParameters) -> bool {
        match self {
            Self::Script(revision) => source == Some(*revision),
            Self::Rack(previous) => source.is_none() && *previous == parameters,
        }
    }
}
struct Scene {
    display: Arc<viewport::Geometry>,
    instances: Arc<Vec<viewport::Instance>>,
    layout: placement::Layout,
    outputs: Arc<Vec<inline_output::Entry>>,
    part_bounds: Vec<[glam::Vec3; 2]>,
    faces: std::collections::BTreeMap<u32, inspection::Face>,
    bounds: [glam::Vec3; 2],
    annotations: dimensions::Annotations,
}

impl Scene {
    fn picking(&self) -> picking::View<'_> {
        picking::View {
            parts: &self.display.parts,
            instances: &self.instances,
        }
    }

    #[cfg(test)]
    fn world_mesh(&self) -> mesh::Mesh {
        self.picking().world_mesh()
    }

    #[cfg(test)]
    fn world_edges(&self) -> inspection::Edges {
        self.picking().world_edges()
    }

    fn selection(&self, selection: &inspection::Selection) -> Option<inspection::Selection> {
        use inspection::Selection;
        Some(match selection {
            Selection::Face(face) => Selection::Face(*self.faces.get(&face.id)?),
            Selection::Pair { first, second } => {
                let first = *self.faces.get(&first.id)?;
                let second = *self.faces.get(&second.id)?;
                Selection::select(Some(&Selection::Face(first)), second, true)
                    .unwrap_or(Selection::Face(second))
            }
            Selection::Edge(edge) => Selection::Edge(self.picking().edge(edge.segments)?),
            Selection::Part { index, .. } => {
                let range = &self.display.parts.get(*index as usize)?.faces;
                Selection::Part {
                    index: *index,
                    faces: [*range.start(), *range.end()],
                    bounds: *self.part_bounds.get(*index as usize)?,
                }
            }
        })
    }
}
struct Snapshot {
    input: Option<performance::Input>,
    cached_parts: usize,
    scene: Scene,
    alternate: Option<Scene>,
    checks: dimensions::Annotations,
    parts: Vec<model::Part>,
    editor_revision: Option<u64>,
    saved_at: Option<std::time::SystemTime>,
    analysis_ms: f64,
    script_ms: f64,
    total_ms: f64,
    source: ModelSource,
    name: String,
    volume_mm3: f64,
    build_ms: f64,
    tessellation_ms: f64,
    revision: u64,
}
impl Snapshot {
    fn set_layout(&mut self, layout: placement::Layout) -> bool {
        if self.scene.layout == layout {
            return false;
        }
        let Some(alternate) = self
            .alternate
            .as_mut()
            .filter(|scene| scene.layout == layout)
        else {
            return false;
        };
        std::mem::swap(&mut self.scene, alternate);
        true
    }
}
#[derive(PartialEq)]
enum ModelSource {
    Rack(RackParameters),
    Script(String),
}
struct Worker {
    commands: Option<mpsc::SyncSender<Command>>,
    results: Option<mpsc::Receiver<Result<Outcome, String>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    fn try_send(&self, command: Command) -> Result<(), mpsc::TrySendError<Command>> {
        self.commands
            .as_ref()
            .expect("Live geometry worker")
            .try_send(command)
    }

    fn try_recv(&self) -> Result<Result<Outcome, String>, mpsc::TryRecvError> {
        self.results
            .as_ref()
            .expect("Live geometry worker")
            .try_recv()
    }

    #[cfg(test)]
    fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<Result<Outcome, String>, mpsc::RecvTimeoutError> {
        self.results
            .as_ref()
            .expect("Live geometry worker")
            .recv_timeout(timeout)
    }

    fn new(context: egui::Context) -> Result<Self, String> {
        let (commands, requests) = mpsc::sync_channel(1);
        let (responses, results) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("cad-kernel".into())
            .spawn(move || {
                let kernel = kernel::Kernel::new();
                let mut cache = kernel.as_ref().map(model::Cache::new);
                while let Ok(command) = requests.recv() {
                    let result = cache.as_mut().map_err(|error| (*error).clone()).and_then(|cache| match command {
                        Command::ExportScript { source, folder, target } => export::model_files_cached(&source, &folder, target, cache).map(|()| Outcome::Exported(format!("Exported {} to {}", target.description(), folder.display()))),
                        Command::Script { source, editor_revision, revision, saved_at, mut input } => {
                            if let Some(input) = &mut input { input.worker_started_ms = input.elapsed_ms(Instant::now()); }
                            let source = source.to_string();
                            build_script_snapshot_cached(source, revision, cache).map(|mut snapshot| {
                                if let Some(input) = &mut input { input.worker_finished_ms = input.elapsed_ms(Instant::now()); }
                                snapshot.input = input; snapshot.saved_at = saved_at; snapshot.editor_revision = Some(editor_revision); Outcome::Model(Box::new(snapshot))
                            })
                        }
                        Command::Build {
                            parameters,
                            revision,
                        } => kernel.as_ref().map_err(Clone::clone)
                            .and_then(|kernel| build_snapshot(kernel, parameters, revision))
                            .map(|snapshot| Outcome::Model(Box::new(snapshot))),
                        #[cfg(feature = "rack-tools")]
                        Command::Export { parameters } => {
                            export_parts(parameters, std::path::Path::new("output/native"))
                                .map(|()| Outcome::Exported("Exported four rack parts, four fit samples, and assembled STEP to output/native".into()))
                        }
                    });
                    let result = result.map_err(|message| {
                        if message.len() <= 8192 { return message; }
                        let head = &message[..message.floor_char_boundary(4096)];
                        let tail = &message[message.ceil_char_boundary(message.len() - 2048)..];
                        format!("{head}\n[Build message truncated]\n{tail}")
                    });
                    if responses.send(result).is_err() {
                        break;
                    }
                    context.request_repaint();
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            commands: Some(commands),
            results: Some(results),
            thread: Some(thread),
        })
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.commands.take();
        self.results.take();
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                eprintln!("Geometry: worker panicked during shutdown");
            }
        }
    }
}

fn build_snapshot(
    kernel: &kernel::Kernel,
    parameters: RackParameters,
    revision: u64,
) -> Result<Snapshot, String> {
    assert!(revision > 0);
    parameters.layout()?;
    let start = Instant::now();
    let mut body = kernel.rack(parameters)?;
    let build_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let mut mesh = body.mesh()?;
    mesh.smooth_normals();
    let edges = body.edges()?;
    let faces = inspection::faces(&mesh)?;
    let tessellation_ms = start.elapsed().as_secs_f64() * 1000.0;
    let volume_mm3 = mesh.volume_mm3();
    let bounds = body.bounds(true)?.map(|point| point.as_vec3());
    let geometry = Arc::new(model::Geometry {
        mesh,
        edges,
        bounds,
        volume_mm3,
    });
    let display = Arc::new(viewport::Geometry::new(&[geometry])?);
    Ok(Snapshot {
        input: None,
        cached_parts: 0,
        scene: Scene {
            display,
            instances: Arc::new(vec![viewport::Instance::new(
                glam::Mat4::IDENTITY,
                mesh::DEFAULT_COLOR,
            )]),
            layout: placement::Layout::Assembly,
            outputs: Arc::new(Vec::new()),
            part_bounds: Vec::new(),
            faces,
            bounds,
            annotations: dimensions::Annotations::default(),
        },
        parts: Vec::new(),
        alternate: None,
        checks: dimensions::Annotations::default(),
        editor_revision: None,
        saved_at: None,
        analysis_ms: 0.0,
        script_ms: 0.0,
        total_ms: build_ms + tessellation_ms,
        source: ModelSource::Rack(parameters),
        name: "Spice rack".into(),
        volume_mm3,
        build_ms,
        tessellation_ms,
        revision,
    })
}

fn build_script_snapshot(source: &str, revision: u64) -> Result<Snapshot, String> {
    let kernel = kernel::Kernel::new()?;
    build_script_snapshot_cached(source.into(), revision, &mut model::Cache::new(&kernel))
}

fn build_script_snapshot_cached(
    source: String,
    revision: u64,
    cache: &mut model::Cache<'_>,
) -> Result<Snapshot, String> {
    assert!(revision > 0);
    let total = Instant::now();
    let model = cache.compile(&source)?;
    let prepared = cache.prepare(&model)?;
    let geometry = prepared.geometry;
    let display = cache.display(&geometry)?;
    let build_ms = prepared.build_ms;
    let start = Instant::now();
    let meshes: Vec<_> = geometry.iter().map(|geometry| &geometry.mesh).collect();
    let checks = model.annotations(&meshes)?;
    let volume_mm3 = geometry.iter().map(|geometry| geometry.volume_mm3).sum();
    let scene = build_scene(
        &model,
        &geometry,
        &cache.presentations(&model, placement::Layout::Assembly)?,
        Arc::clone(&display),
        placement::Layout::Assembly,
        &checks,
    )?;
    let alternate = if model.parts.iter().any(|part| part.print.is_some()) {
        Some(build_scene(
            &model,
            &geometry,
            &cache.presentations(&model, placement::Layout::Print)?,
            display,
            placement::Layout::Print,
            &checks,
        )?)
    } else {
        None
    };
    let tessellation_ms = prepared.tessellation_ms + start.elapsed().as_secs_f64() * 1000.0;
    Ok(Snapshot {
        input: None,
        cached_parts: prepared.hits,
        scene,
        alternate,
        checks,
        parts: model.parts,
        editor_revision: None,
        saved_at: None,
        analysis_ms: model.analysis_ms,
        script_ms: model.script_ms,
        total_ms: total.elapsed().as_secs_f64() * 1000.0,
        source: ModelSource::Script(source),
        name: model.name,
        volume_mm3,
        build_ms,
        tessellation_ms,
        revision,
    })
}

fn build_scene(
    model: &model::Model,
    geometry: &[Arc<model::Geometry>],
    presentations: &[Arc<model::Presentation>],
    display: Arc<viewport::Geometry>,
    layout: placement::Layout,
    checks: &dimensions::Annotations,
) -> Result<Scene, String> {
    assert_eq!(presentations.len(), model.parts.len());
    assert_eq!(geometry.len(), presentations.len());
    assert!(!presentations.is_empty());
    let mut outputs = Vec::with_capacity(model.parts.len() + 64);
    let mut part_bounds = Vec::with_capacity(model.parts.len());
    let mut bounds = [
        glam::Vec3::splat(f32::INFINITY),
        glam::Vec3::splat(f32::NEG_INFINITY),
    ];
    for ((part, presentation), geometry) in model.parts.iter().zip(presentations).zip(geometry) {
        let translation = part.placement(layout).translation();
        let part_box = presentation.bounds.map(|point| point + translation);
        bounds[0] = bounds[0].min(part_box[0]);
        bounds[1] = bounds[1].max(part_box[1]);
        part_bounds.push(part_box);
        outputs.push(inline_output::Entry {
            line: part.line,
            name: part.name.clone(),
            content: inline_output::Content::Part {
                image: presentation.image.clone(),
                dimensions: part_box[1] - part_box[0],
                volume_mm3: geometry.volume_mm3,
            },
        });
    }
    let mut faces = std::collections::BTreeMap::new();
    for ((part, presentation), canonical) in
        model.parts.iter().zip(presentations).zip(&display.parts)
    {
        for (id, face) in &presentation.faces {
            let mut face = face.translated(part.placement(layout).translation());
            face.id = id + canonical.faces.start() - 1;
            faces.insert(face.id, face);
        }
    }
    let annotations = if layout == placement::Layout::Assembly {
        checks.clone()
    } else {
        dimensions::Annotations::default()
    };
    for measurement in &checks.measurements {
        outputs.push(inline_output::Entry {
            line: measurement.line,
            name: if layout == placement::Layout::Print {
                format!("Assembly · {}", measurement.name)
            } else {
                measurement.name.clone()
            },
            content: inline_output::Content::Measurement {
                distance_mm: measurement.distance_mm,
                checked: measurement.checked,
            },
        });
    }
    outputs.sort_by_key(|output| output.line);
    Ok(Scene {
        display,
        instances: Arc::new(
            model
                .parts
                .iter()
                .map(|part| viewport::Instance::new(part.placement(layout).matrix(), part.color))
                .collect(),
        ),
        layout,
        outputs: Arc::new(outputs),
        part_bounds,
        faces,
        bounds,
        annotations,
    })
}

#[cfg(any(test, feature = "rack-tools"))]
fn export_parts(parameters: RackParameters, folder: &std::path::Path) -> Result<(), String> {
    parameters.layout()?;
    let parts = printing::build_parts(parameters)?;
    let samples = printing::build_fit_samples(parameters.thickness_mm)?;
    let mut exports = Vec::with_capacity(8);
    for part in parts.iter().chain(samples.iter()) {
        let (shape, _) = part.on_bed()?;
        let mesh = shape.mesh().map_err(|error| error.to_string())?;
        if mesh.indices.len() > 300000 {
            return Err("Print mesh exceeds its triangle budget".into());
        }
        let mut vertices = Vec::with_capacity(mesh.indices.len());
        for triangle in mesh.indices.as_chunks::<3>().0.iter() {
            let points: [glam::DVec3; 3] =
                std::array::from_fn(|index| mesh.vertices[triangle[index]]);
            let normal = (points[1] - points[0])
                .cross(points[2] - points[0])
                .normalize_or_zero();
            if normal.length_squared() < 0.5 {
                return Err(format!("{} has a degenerate face", part.name));
            }
            if points.iter().all(|point| point.z > 0.01) {
                if normal.z < -std::f64::consts::FRAC_1_SQRT_2 - 1e-6 {
                    return Err(format!(
                        "{} needs support in its print orientation",
                        part.name
                    ));
                }
            }
            vertices.extend(points.map(|point| crate::mesh::Vertex {
                position: point.as_vec3().to_array(),
                normal: normal.as_vec3().to_array(),
                face_id: 1,
                color: crate::mesh::vertex_color(crate::mesh::DEFAULT_COLOR),
            }));
        }
        crate::mesh::Mesh {
            vertices,
            face_count: 1,
        }
        .validate()?;
        exports.push((part.name, shape));
    }
    std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    printing::assemble_parts(&parts)
        .write_step(folder.join("spice-rack.step"))
        .map_err(|error| error.to_string())?;
    for (name, shape) in exports {
        shape
            .write_step(folder.join(format!("spice-rack-{name}.step")))
            .map_err(|error| error.to_string())?;
        shape
            .write_stl_with_tolerance(folder.join(format!("spice-rack-{name}.stl")), 0.1)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

struct Editor {
    sweep: Option<performance::Sweep>,
    preview_input: Option<performance::Input>,
    save_dialog: Option<save::Dialog>,
    preview: preview::Schedule,
    palette: palette::State,
    parameter_state: parameters::State,
    show_display_settings: bool,
    picker: Option<files::Picker>,
    pending_document: Option<document::Document>,
    pending_location: Option<lsp::Location>,
    language_popup: Option<(u64, lsp::Insight)>,
    completion_index: usize,
    language_request_cursor: Option<usize>,
    frame_next_model: bool,
    diagnostics: Option<diagnostics::Session>,
    watcher: Option<document::Watcher>,
    profile: Option<performance::Profile>,
    code_state: code_editor::State,
    script: Option<document::Document>,
    show_code: bool,
    show_details: bool,
    show_diagnostics: bool,
    show_edges: bool,
    show_dimensions: bool,
    show_inspector: bool,
    focus_editor: bool,
    show_performance: bool,
    edge_width_px: f32,
    exit: ExitState,
    parameters: RackParameters,
    snapshot: Option<Snapshot>,
    worker: Worker,
    work: WorkerState,
    dirty_since: Option<Instant>,
    revision: u64,
    camera: viewport::Camera,
    viewport_aspect: f32,
    selected: Option<inspection::Selection>,
    message: String,
    ui_ms: f64,
}
enum ExitState {
    Open,
    ConfirmDiscard,
    Closing,
}

impl Editor {
    fn new(
        context: &eframe::CreationContext<'_>,
        model_path: Option<std::path::PathBuf>,
        profile: Option<Instant>,
        sweep: Option<performance::Sweep>,
    ) -> Result<Self, String> {
        performance::stage(profile, "device_and_egui_ready");
        viewport::initialize(context)?;
        performance::stage(profile, "viewport_ready");
        interface::configure(&context.egui_ctx);
        let layout = PARAMETERS.layout()?;
        let worker = Worker::new(context.egui_ctx.clone())?;
        let script = if sweep.is_some() {
            Some(document::Document::loaded(
                std::env::temp_dir().join(format!("spice-cad-profile-{}.luau", std::process::id())),
                include_str!("../../models/rack-parts.luau").into(),
            ))
        } else {
            model_path.map(document::Document::new)
        };
        let diagnostics = script
            .as_ref()
            .map(|script| diagnostics::Session::new(script.path.clone(), context.egui_ctx.clone()))
            .transpose()?;
        let watcher = script
            .as_ref()
            .filter(|_| sweep.is_none())
            .map(|script| document::Watcher::new(script.path.clone(), context.egui_ctx.clone()))
            .transpose()?;
        let mut code_state = code_editor::State::default();
        code_state.modal.enable(true);
        let mut palette = palette::State::default();
        let dirty_since = script
            .is_none()
            .then_some(Instant::now() - Duration::from_secs(1));
        if sweep.is_none() {
            if let Some(script) = &script {
                palette.remember(script.path.clone());
            }
        }
        Ok(Self {
            sweep,
            preview_input: None,
            save_dialog: None,
            preview: preview::Schedule::default(),
            palette,
            parameter_state: parameters::State::default(),
            show_display_settings: false,
            picker: None,
            pending_document: None,
            pending_location: None,
            language_popup: None,
            completion_index: 0,
            language_request_cursor: None,
            frame_next_model: true,
            diagnostics,
            watcher,
            profile: profile.map(performance::Profile::new),
            code_state,
            script,
            show_code: true,
            show_details: false,
            show_diagnostics: false,
            show_edges: true,
            show_dimensions: false,
            show_inspector: true,
            focus_editor: false,
            show_performance: false,
            edge_width_px: 1.5,
            exit: ExitState::Open,
            parameters: PARAMETERS,
            snapshot: None,
            worker,
            work: WorkerState::Idle,
            dirty_since,
            revision: 0,
            camera: viewport::Camera::rack(layout.dimensions_mm.to_array()),
            viewport_aspect: 1.0,
            selected: None,
            message: "Initializing native SMLib…".into(),
            ui_ms: 0.0,
        })
    }

    fn adopt_document(
        &mut self,
        document: document::Document,
        context: &egui::Context,
    ) -> Result<(), String> {
        let watcher = document::Watcher::new(document.path.clone(), context.clone())?;
        let diagnostics = diagnostics::Session::new(document.path.clone(), context.clone())?;
        let vim = self.code_state.modal.enabled;
        self.code_state = code_editor::State::default();
        self.code_state.modal.enable(vim);
        self.code_state.focus();
        self.palette.remember(document.path.clone());
        self.parameter_state = parameters::State::default();
        if let Some(location) = self.pending_location.take() {
            self.code_state
                .jump_to(location.range(&document.buffer.rope));
        }
        self.script = Some(document);
        self.snapshot = None;
        self.preview_input = None;
        self.watcher = Some(watcher);
        self.diagnostics = Some(diagnostics);
        self.language_popup = None;
        self.frame_next_model = true;
        self.dirty_since = None;
        self.preview.run(
            self.script.as_ref().expect("Adopted model").buffer.revision,
            Instant::now(),
        );
        self.show_code = true;
        self.message = "Model loaded · checking and building geometry".into();
        context.request_repaint();
        Ok(())
    }

    fn observe_preview(&mut self) {
        if let Some(script) = &self.script {
            if script.source_ready() {
                let revision = script.buffer.revision;
                let changed = if self.preview.observed().is_none() {
                    self.preview.run(revision, Instant::now());
                    true
                } else {
                    self.preview.observe(revision, Instant::now())
                };
                if self
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.editor_revision == Some(revision))
                {
                    let reused = self.preview.reuse(revision);
                    if changed || reused {
                        self.message = "Preview ready · geometry reused".into();
                    }
                }
            }
        }
    }

    fn can_export(&self) -> bool {
        matches!(self.work, WorkerState::Idle)
            && !self.preview.pending()
            && self.dirty_since.is_none()
            && self
                .snapshot
                .as_ref()
                .is_some_and(|model| match (&model.source, &self.script) {
                    (ModelSource::Rack(parameters), None) => {
                        cfg!(feature = "rack-tools") && *parameters == self.parameters
                    }
                    (ModelSource::Script(_), Some(script)) => {
                        model.editor_revision == Some(script.buffer.revision)
                    }
                    _ => false,
                })
    }

    fn export(&mut self, target: export::Target) -> Result<(), String> {
        if !self.can_export() {
            return Err("Run the current code successfully before exporting".into());
        }
        let command = match &self.snapshot.as_ref().expect("Validated export").source {
            #[cfg(feature = "rack-tools")]
            ModelSource::Rack(parameters) => Command::Export {
                parameters: *parameters,
            },
            #[cfg(not(feature = "rack-tools"))]
            ModelSource::Rack(_) => return Err("Rack tools are not enabled".into()),
            ModelSource::Script(source) => {
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|error| error.to_string())?
                    .as_nanos();
                Command::ExportScript {
                    target,
                    source: source.clone(),
                    folder: format!("output/native/model-{stamp}").into(),
                }
            }
        };
        match self.worker.try_send(command) {
            Ok(()) => {
                self.work = WorkerState::Export;
                self.message = format!("Generating and validating {}…", target.description());
                Ok(())
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                self.work = WorkerState::Unavailable;
                Err("Geometry worker is unavailable".into())
            }
            Err(mpsc::TrySendError::Full(_)) => {
                Err("Geometry worker is busy; try exporting again".into())
            }
        }
    }

    fn open_path(
        &mut self,
        path: std::path::PathBuf,
        context: &egui::Context,
    ) -> Result<(), String> {
        self.pending_location = None;
        if self
            .script
            .as_ref()
            .is_some_and(|script| script.path == path)
        {
            self.show_code = true;
            self.code_state.focus();
            return Ok(());
        }
        let mut picker = files::Picker::new(
            path.parent()
                .unwrap_or(std::path::Path::new("."))
                .to_owned(),
            context.clone(),
        )?;
        picker.open_path(path);
        self.picker = Some(picker);
        Ok(())
    }

    fn start_preview(&mut self) {
        if !matches!(self.work, WorkerState::Idle) {
            return;
        }
        let Some(revision) = self.preview.ready(Instant::now()) else {
            return;
        };
        let Some(script) = &self.script else {
            return;
        };
        assert_eq!(revision, script.buffer.revision);
        self.revision += 1;
        match self.worker.try_send(Command::Script {
            input: self
                .preview_input
                .filter(|input| input.revision == revision)
                .map(|mut input| {
                    input.dispatched_ms = input.elapsed_ms(Instant::now());
                    input
                }),
            source: script.buffer.rope.clone(),
            editor_revision: revision,
            revision: self.revision,
            saved_at: if script.edited() {
                None
            } else {
                script.saved_at
            },
        }) {
            Ok(()) => {
                self.preview.complete(revision);
                self.work = WorkerState::Busy(ModelRevision::Script(revision));
                if let Some(diagnostics) = &mut self.diagnostics {
                    diagnostics.run_started();
                }
                self.message = "Updating preview…".into();
            }
            Err(error) => {
                self.work = WorkerState::Unavailable;
                self.message = error.to_string();
            }
        }
    }

    fn poll(&mut self, context: &egui::Context) {
        let file_changed = match self
            .watcher
            .as_ref()
            .map(|watcher| watcher.events.try_recv())
        {
            Some(Ok(())) => true,
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.message = "File watcher stopped; external changes will not reload. Restart the application.".into();
                self.watcher = None;
                false
            }
            _ => false,
        };
        if let Some(script) = &mut self.script {
            if file_changed {
                if script.poll() {
                    self.message = "Source updated from disk".into();
                }
                if let Some(error) = &script.read_error {
                    self.message.clone_from(error);
                    self.dirty_since = None;
                }
            }
        }
        self.observe_preview();
        if let (Some(diagnostics), Some(script)) = (&mut self.diagnostics, &self.script) {
            if script.source_ready() {
                diagnostics.sync(&script.buffer, context);
            }
        }
        match self.worker.try_recv() {
            Ok(result) => {
                let active = match std::mem::replace(&mut self.work, WorkerState::Idle) {
                    WorkerState::Busy(revision) => Some(revision),
                    _ => None,
                };
                let current = active.is_none_or(|revision| {
                    revision.current(
                        self.script.as_ref().map(|script| script.buffer.revision),
                        self.parameters,
                    )
                });
                let run_revision = match active {
                    Some(ModelRevision::Script(revision)) => Some(revision),
                    _ => None,
                };
                if let Some(revision) = run_revision {
                    self.preview.complete(revision);
                }
                if !current {
                    context.request_repaint();
                } else {
                    match result {
                        Ok(Outcome::Model(mut snapshot)) => {
                            if let Some(input) = &mut snapshot.input {
                                input.received_ms = input.elapsed_ms(Instant::now());
                            }
                            if self.snapshot.as_ref().is_some_and(|previous| {
                                previous.scene.layout == placement::Layout::Print
                            }) {
                                snapshot.set_layout(placement::Layout::Print);
                            }
                            if let Some(revision) = snapshot.editor_revision {
                                self.code_state
                                    .outputs
                                    .set(revision, Arc::clone(&snapshot.scene.outputs));
                            }
                            if self.script.is_some()
                                && self.frame_next_model
                                && self.script.as_ref().is_some_and(|script| {
                                    snapshot.editor_revision == Some(script.buffer.revision)
                                })
                            {
                                self.camera = snapshot_camera(&snapshot, self.viewport_aspect);
                                self.frame_next_model = false;
                            }
                            self.message = "Preview ready".into();
                            self.selected = self
                                .snapshot
                                .as_ref()
                                .filter(|previous| {
                                    Arc::ptr_eq(&previous.scene.display, &snapshot.scene.display)
                                })
                                .and(self.selected.as_ref())
                                .and_then(|selection| snapshot.scene.selection(selection));
                            self.snapshot = Some(*snapshot);
                        }
                        Ok(Outcome::Exported(message)) => self.message = message,
                        Err(error) => {
                            if let (Some(diagnostics), Some(revision)) =
                                (&mut self.diagnostics, run_revision)
                            {
                                diagnostics.run_failed(revision, &error);
                            }
                            self.message = error;
                        }
                    }
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.dirty_since = None;
                if !matches!(self.work, WorkerState::Unavailable) {
                    self.message = format!(
                        "Geometry worker stopped: {}. Restart the application.",
                        self.message
                    );
                }
                self.work = WorkerState::Unavailable;
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if matches!(self.work, WorkerState::Idle) {
            if let Some(changed) = self.dirty_since {
                if changed.elapsed() >= Duration::from_millis(150) {
                    self.dirty_since = None;
                    match self.parameters.layout() {
                        Ok(_) => {
                            if let Some(snapshot) = &mut self.snapshot {
                                if let ModelSource::Rack(parameters) = snapshot.source {
                                    let equivalent = RackParameters {
                                        jar_height_mm: self.parameters.jar_height_mm,
                                        cabinet_clearance_mm: self.parameters.cabinet_clearance_mm,
                                        ..parameters
                                    };
                                    if equivalent == self.parameters {
                                        snapshot.source = ModelSource::Rack(self.parameters);
                                        self.message =
                                            "Clearances checked; existing geometry reused".into();
                                        return;
                                    }
                                }
                            }
                            self.revision += 1;
                            match self.worker.try_send(Command::Build {
                                parameters: self.parameters,
                                revision: self.revision,
                            }) {
                                Ok(()) => {
                                    self.work =
                                        WorkerState::Busy(ModelRevision::Rack(self.parameters));
                                    self.message =
                                        "Rebuilding geometry; viewport remains interactive…".into();
                                }
                                Err(error) => {
                                    self.work = WorkerState::Unavailable;
                                    self.message = error.to_string();
                                }
                            }
                        }
                        Err(error) => self.message = error,
                    }
                }
            }
        }
    }
}

impl eframe::App for Editor {
    fn logic(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(sweep) = &self.sweep {
            let now = Instant::now();
            let closing = context.input(|input| input.viewport().close_requested());
            if closing || sweep.expired(now) {
                if closing {
                    eprintln!("Profile: preview_sweep status=cancelled");
                } else {
                    sweep.finish(None, now);
                }
                self.sweep = None;
                self.exit = ExitState::Closing;
                context.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                context.request_repaint_after(sweep.delay(now));
            }
        }
    }

    fn raw_input_hook(&mut self, _context: &egui::Context, input: &mut egui::RawInput) {
        if self.sweep.is_some() {
            input.events.clear();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let started = Instant::now();
        self.poll(ui.ctx());
        let navigation_allowed = self.pending_document.is_none()
            && self.save_dialog.is_none()
            && !self.picker.as_ref().is_some_and(|picker| picker.open)
            && matches!(self.exit, ExitState::Open);
        let mut palette_requested = None;
        if navigation_allowed {
            for (modifiers, mode) in [
                (
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    palette::Mode::Commands,
                ),
                (egui::Modifiers::COMMAND, palette::Mode::Files),
            ] {
                if ui.input_mut(|input| {
                    input.consume_shortcut(&egui::KeyboardShortcut::new(modifiers, egui::Key::P))
                }) {
                    palette_requested = Some(mode);
                }
            }
        }
        if let Some(mode) = palette_requested.take() {
            let root = self
                .script
                .as_ref()
                .and_then(|script| script.path.parent())
                .unwrap_or(std::path::Path::new("."))
                .to_owned();
            self.palette.open(mode, root, ui.ctx());
            self.language_popup = None;
            self.language_request_cursor = None;
        }
        let palette_was_open = self.palette.is_open();
        let palette_selection = self.palette.show(
            ui.ctx(),
            palette::Availability {
                source: self.script.is_some(),
                export: self.can_export(),
                model: self.snapshot.is_some(),
            },
        );
        let shortcuts_enabled = navigation_allowed && !palette_was_open;
        let mut completion_accept = false;
        let mut completion_navigate = false;
        if matches!(self.language_popup, Some((_, lsp::Insight::Completions(_)))) {
            ui.input_mut(|input| {
                completion_accept = input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                    || input.consume_key(egui::Modifiers::NONE, egui::Key::Tab);
                if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                    completion_navigate = true;
                    self.completion_index = self.completion_index.saturating_add(1);
                }
                if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                    completion_navigate = true;
                    self.completion_index = self.completion_index.saturating_sub(1);
                }
            });
        }
        if self.language_popup.is_some()
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.language_popup = None;
        }
        let mut open_requested = shortcuts_enabled
            && ui.input_mut(|input| {
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::COMMAND,
                    egui::Key::O,
                ))
            });
        let mut close_after_save = false;
        let mut file_dialog_requested = None;
        if shortcuts_enabled {
            if ui.input_mut(|input| {
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::S,
                ))
            }) {
                file_dialog_requested = Some(save::Mode::SaveAs);
            }
            if ui.input_mut(|input| {
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::COMMAND,
                    egui::Key::N,
                ))
            }) {
                file_dialog_requested = Some(save::Mode::NewModel);
            }
        }
        let mut layout_requested = None;
        let mut frame_layout = false;
        let mut save_requested = shortcuts_enabled
            && ui.input_mut(|input| {
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::COMMAND,
                    egui::Key::S,
                ))
            });
        let mut run_requested = shortcuts_enabled
            && ui.input_mut(|input| {
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::COMMAND,
                    egui::Key::Enter,
                ))
            });
        if self.script.is_some() && shortcuts_enabled {
            for (key, code, focus) in [
                (egui::Key::Num1, true, true),
                (egui::Key::Num2, true, false),
                (egui::Key::Num3, false, false),
            ] {
                if ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, key)) {
                    self.show_code = code;
                    self.focus_editor = focus;
                }
            }
        }
        let mut example_requested = None;
        let mut export_requested = false;
        let mut export_target = export::Target::Print;
        if let Some(selection) = palette_selection {
            match selection {
                palette::Selection::Cancel => self.code_state.focus(),
                palette::Selection::File(path) => {
                    if let Err(error) = self.open_path(path, ui.ctx()) {
                        self.message = error;
                    }
                }
                palette::Selection::Command(command) => {
                    use palette::Command as Navigation;
                    if !matches!(command, Navigation::Find | Navigation::Replace) {
                        self.code_state.focus();
                    }
                    match command {
                        Navigation::Open => open_requested = true,
                        Navigation::Run => run_requested = true,
                        Navigation::Save => save_requested = true,
                        Navigation::NewModel => file_dialog_requested = Some(save::Mode::NewModel),
                        Navigation::SaveAs => file_dialog_requested = Some(save::Mode::SaveAs),
                        Navigation::AutoPreview => {
                            if let Some(script) = &self.script {
                                self.preview.set_automatic(
                                    !self.preview.automatic(),
                                    script.buffer.revision,
                                    Instant::now(),
                                );
                            }
                        }
                        Navigation::Export => export_requested = true,
                        Navigation::ExportGame => {
                            export_requested = true;
                            export_target = export::Target::Game;
                        }
                        Navigation::Find | Navigation::Replace => {
                            self.show_code = true;
                            self.code_state
                                .open_search(false, command == Navigation::Replace);
                        }
                        Navigation::Code | Navigation::Split | Navigation::Model => {
                            self.show_code = command != Navigation::Model;
                            self.focus_editor = command == Navigation::Code;
                        }
                        Navigation::Parameters => {
                            self.show_inspector = true;
                            self.focus_editor = false;
                        }
                        Navigation::Diagnostics => self.show_diagnostics = !self.show_diagnostics,
                        Navigation::Display => self.show_display_settings = true,
                        Navigation::Edges => self.show_edges = !self.show_edges,
                        Navigation::Vim => {
                            self.code_state.modal.enable(!self.code_state.modal.enabled)
                        }
                        Navigation::Performance => self.show_performance = !self.show_performance,
                        Navigation::Frame => {
                            if let Some(snapshot) = &self.snapshot {
                                self.camera = snapshot_camera(snapshot, self.viewport_aspect);
                            }
                        }
                        Navigation::Front => {
                            self.camera.yaw = -std::f32::consts::FRAC_PI_2;
                            self.camera.pitch = 0.0;
                        }
                        Navigation::Top => self.camera.pitch = 1.55,
                        Navigation::Rack => example_requested = Some(document::Example::Rack),
                        Navigation::RackParts => {
                            example_requested = Some(document::Example::RackParts)
                        }
                        Navigation::Bracket => {
                            example_requested = Some(document::Example::MountingBracket)
                        }
                        Navigation::ExtrudedBracket => {
                            example_requested = Some(document::Example::ExtrudedBracket)
                        }
                        Navigation::EdgeFillets => {
                            example_requested = Some(document::Example::EdgeFillets)
                        }
                        Navigation::EdgeChamfers => {
                            example_requested = Some(document::Example::EdgeChamfers)
                        }
                        Navigation::RevolvedBushing => {
                            example_requested = Some(document::Example::RevolvedBushing)
                        }
                        Navigation::RocketEngine => {
                            example_requested = Some(document::Example::RocketEngine)
                        }
                        Navigation::AssemblyLayout => {
                            layout_requested = Some(placement::Layout::Assembly)
                        }
                        Navigation::PrintLayout => {
                            layout_requested = Some(placement::Layout::Print)
                        }
                    }
                }
            }
        }
        if shortcuts_enabled {
            for (key, replace) in [(egui::Key::F, false), (egui::Key::H, true)] {
                if ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, key)) {
                    self.show_code = true;
                    self.code_state.open_search(false, replace);
                    self.language_popup = None;
                    self.language_request_cursor = None;
                    ui.ctx().request_repaint();
                }
            }
        }
        if shortcuts_enabled
            && ui.input_mut(|input| {
                input.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::J,
                )
            })
        {
            self.show_diagnostics = !self.show_diagnostics;
        }
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                ui.style_mut().visuals.button_frame = false;
                interface::logo(ui);
                ui.label(egui::RichText::new("SPICE CAD").size(15.0).strong());
                ui.separator();
                ui.menu_button("File", |ui| {
                    if ui.button("New model · Ctrl+N").clicked() {
                        file_dialog_requested = Some(save::Mode::NewModel);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            self.script.is_some(),
                            egui::Button::new("Save as · Ctrl+Shift+S"),
                        )
                        .clicked()
                    {
                        file_dialog_requested = Some(save::Mode::SaveAs);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Quick Open · Ctrl+P").clicked() {
                        palette_requested = Some(palette::Mode::Files);
                        ui.close();
                    }
                    if ui.button("Browse files · Ctrl+O").clicked() {
                        open_requested = true;
                        ui.close();
                    }
                    if ui.button("Commands · Ctrl+Shift+P").clicked() {
                        palette_requested = Some(palette::Mode::Commands);
                        ui.close();
                    }
                });
                ui.menu_button("Examples", |ui| {
                    for (label, example) in [
                        ("Spice rack", document::Example::Rack),
                        ("Spice rack · four parts", document::Example::RackParts),
                        ("Mounting bracket", document::Example::MountingBracket),
                        ("Measured block", document::Example::MeasuredBlock),
                        ("Extruded bracket", document::Example::ExtrudedBracket),
                        ("Selected edge fillets", document::Example::EdgeFillets),
                        ("Selected edge chamfers", document::Example::EdgeChamfers),
                        ("Revolved bushing", document::Example::RevolvedBushing),
                        ("Rocket engine assembly", document::Example::RocketEngine),
                    ] {
                        if ui.button(label).clicked() {
                            example_requested = Some(example);
                            ui.close();
                        }
                    }
                });
                ui.menu_button("View", |ui| {
                    ui.add_enabled_ui(self.script.is_some(), |ui| {
                        if ui.checkbox(&mut self.show_code, "Code editor").changed()
                            && !self.show_code
                        {
                            self.focus_editor = false;
                        }
                        if ui
                            .checkbox(&mut self.focus_editor, "Focus editor")
                            .changed()
                            && self.focus_editor
                        {
                            self.show_code = true;
                        }
                    });
                    ui.checkbox(&mut self.show_inspector, "Parameters & properties");
                    ui.checkbox(&mut self.show_diagnostics, "Diagnostics");
                    ui.checkbox(&mut self.show_dimensions, "Measurements & face names");
                });
                ui.menu_button("Debug", |ui| {
                    ui.checkbox(&mut self.show_performance, "Performance");
                });
                ui.menu_button("Display", |ui| {
                    interface::display_settings(ui, &mut self.show_edges, &mut self.edge_width_px);
                });
                ui.menu_button("Editor", |ui| {
                    let mut vim = self.code_state.modal.enabled;
                    if ui.checkbox(&mut vim, "Vim-style keys").changed() {
                        self.code_state.modal.enable(vim);
                    }
                    for (label, action) in [
                        (
                            "Find · Ctrl+F / /",
                            modal::Action::Find {
                                backward: false,
                                replace: false,
                            },
                        ),
                        (
                            "Replace · Ctrl+H",
                            modal::Action::Find {
                                backward: false,
                                replace: true,
                            },
                        ),
                        ("Complete · Ctrl+Space", modal::Action::Complete),
                        ("Inspect type · F1 / K", modal::Action::Hover),
                        ("Go to definition · F12 / gd", modal::Action::Definition),
                    ] {
                        if ui.button(label).clicked() {
                            self.code_state.action = Some(action);
                        }
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (label, code, focus) in [
                        ("Model", false, false),
                        ("Split", true, false),
                        ("Code", true, true),
                    ] {
                        let available = self.script.is_some() || !code;
                        let selected = if self.script.is_some() {
                            self.show_code == code && self.focus_editor == focus
                        } else {
                            !code
                        };
                        if ui
                            .add_enabled(
                                available,
                                egui::Button::selectable(selected, label)
                                    .min_size(egui::vec2(72.0, interface::CONTROL_HEIGHT)),
                            )
                            .on_hover_text(match label {
                                "Code" => "Ctrl+1 · Focus the code editor",
                                "Split" => "Ctrl+2 · Code and model side by side",
                                _ => "Ctrl+3 · Focus the model",
                            })
                            .clicked()
                        {
                            self.show_code = code;
                            self.focus_editor = focus;
                        }
                    }
                });
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if let Some(script) = &self.script {
                    ui.allocate_ui_with_layout(
                        egui::vec2(180.0, interface::CONTROL_HEIGHT),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.add(
                                egui::Label::new(
                                    script
                                        .path
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy(),
                                )
                                .truncate(),
                            )
                            .on_hover_text(script.path.display().to_string());
                        },
                    );
                    if script.edited() {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(8.0, 12.0), egui::Sense::hover());
                        ui.painter()
                            .circle_filled(rect.center(), 2.5, interface::ACCENT);
                    }
                    ui.small(
                        egui::RichText::new(if script.edited() { "Unsaved" } else { "Saved" })
                            .color(interface::MUTED),
                    );
                    ui.add_space(8.0);
                    open_requested |= ui
                        .button("Open…")
                        .on_hover_text("Ctrl+O · Open a model file")
                        .clicked();
                    save_requested |= ui
                        .add(
                            egui::Button::new("Save")
                                .min_size(egui::vec2(64.0, interface::CONTROL_HEIGHT)),
                        )
                        .on_hover_text("Ctrl+S · Save and rebuild")
                        .clicked();
                    ui.separator();
                    run_requested |= ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Run").color(interface::ON_ACCENT),
                            )
                            .fill(interface::ACCENT)
                            .min_size(egui::vec2(80.0, interface::CONTROL_HEIGHT)),
                        )
                        .on_hover_text("Ctrl+Enter · Preview editor buffer")
                        .clicked();
                    let mut automatic = self.preview.automatic();
                    if ui
                        .checkbox(&mut automatic, "Auto preview")
                        .on_hover_text("Update geometry after a 250 ms typing pause")
                        .changed()
                    {
                        self.preview.set_automatic(
                            automatic,
                            script.buffer.revision,
                            Instant::now(),
                        );
                    }
                }
                ui.separator();
                if let Some(snapshot) = &self.snapshot {
                    if snapshot.alternate.is_some() {
                        ui.menu_button(
                            if snapshot.scene.layout == placement::Layout::Assembly {
                                "Layout · Assembly"
                            } else {
                                "Layout · Print"
                            },
                            |ui| {
                                for (layout, label) in [
                                    (placement::Layout::Assembly, "Assembly"),
                                    (placement::Layout::Print, "Print"),
                                ] {
                                    if ui
                                        .selectable_label(snapshot.scene.layout == layout, label)
                                        .clicked()
                                    {
                                        layout_requested = Some(layout);
                                        ui.close();
                                    }
                                }
                            },
                        )
                        .response
                        .on_hover_text(
                            "Switch cached placements without changing source or rebuilding solids",
                        );
                    }
                }
                let can_export = self.can_export();
                if self.script.is_some() {
                    ui.add_enabled_ui(can_export, |ui| {
                        ui.menu_button("Export", |ui| {
                            if ui.button("3D print · STEP + STL").clicked() {
                                export_requested = true;
                                ui.close();
                            }
                            if ui.button("Game asset · GLB").clicked() {
                                export_requested = true;
                                export_target = export::Target::Game;
                                ui.close();
                            }
                        });
                    })
                    .response
                    .on_disabled_hover_text("Run the current code successfully before exporting");
                } else {
                    export_requested |= ui
                        .add_enabled(can_export, egui::Button::new("Export P1S parts"))
                        .clicked();
                }
                if self.script.is_some() {
                    ui.separator();
                    if ui
                        .add(
                            egui::Button::selectable(
                                self.show_inspector && !self.focus_editor,
                                "Properties",
                            )
                            .min_size(egui::vec2(100.0, interface::CONTROL_HEIGHT)),
                        )
                        .on_hover_text("Show parts, source parameters, and selected geometry")
                        .clicked()
                    {
                        self.show_inspector = !self.show_inspector || self.focus_editor;
                        self.focus_editor = false;
                    }
                }
            });
            ui.add_space(3.0);
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.spacing_mut().interact_size.y = 28.0;
            ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
            ui.horizontal(|ui| {
                if let Some(diagnostics) = &self.diagnostics {
                    let (status, color) = diagnostics.status();
                    if ui
                        .selectable_label(
                            self.show_diagnostics,
                            egui::RichText::new(status).color(color),
                        )
                        .on_hover_text("Show or hide diagnostics")
                        .clicked()
                    {
                        self.show_diagnostics = !self.show_diagnostics;
                    }
                }
                if ui.selectable_label(self.show_details, "Details").clicked() {
                    self.show_details = !self.show_details;
                }
                ui.colored_label(interface::ACCENT, self.code_state.modal.label());
                if let Some(script) = &self.script {
                    let cursor = self.code_state.cursor().min(script.buffer.rope.len_chars());
                    let line = script.buffer.rope.char_to_line(cursor);
                    ui.small(format!(
                        "Ln {}, Col {}",
                        line + 1,
                        cursor - script.buffer.rope.line_to_char(line) + 1
                    ));
                }
                let message = self
                    .code_state
                    .error
                    .or(self.code_state.modal.error)
                    .unwrap_or(&self.message);
                ui.add(egui::Label::new(message).truncate())
                    .on_hover_text(message);
            });
        });
        if self.show_diagnostics {
            if let (Some(diagnostics), Some(script)) = (&self.diagnostics, &self.script) {
                egui::Panel::bottom("diagnostics")
                    .default_size(132.0)
                    .min_size(70.0)
                    .max_size(260.0)
                    .show(ui, |ui| {
                        if let Some(span) = diagnostics.show(ui) {
                            self.code_state.jump_to(span.range(&script.buffer));
                            self.show_code = true;
                        }
                    });
            }
        }
        if self.show_details {
            egui::Window::new("Build details")
                .open(&mut self.show_details)
                .default_width(600.0)
                .show(ui.ctx(), |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .show(ui, |ui| {
                            ui.label(&self.message);
                        });
                    if ui.button("Copy details").clicked() {
                        ui.ctx().copy_text(self.message.clone());
                    }
                });
        }
        if self.show_performance {
            egui::Window::new("Performance")
                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16.0, 70.0))
                .open(&mut self.show_performance)
                .show(ui.ctx(), |ui| {
                    let metrics = viewport::metrics(frame);
                    if let Some(snapshot) = &self.snapshot {
                        ui.label(format!(
                            "Analyze {:.1} ms · script {:.1} ms",
                            snapshot.analysis_ms, snapshot.script_ms
                        ));
                        ui.label(format!(
                            "Kernel {:.1} ms · mesh {:.1} ms",
                            snapshot.build_ms, snapshot.tessellation_ms
                        ));
                        ui.label(format!("Build pipeline {:.1} ms", snapshot.total_ms));
                        ui.small(format!("Preview revision {}", snapshot.revision));
                        ui.label(format!(
                            "{} / {} parts reused",
                            snapshot.cached_parts,
                            snapshot.parts.len()
                        ));
                        ui.label(format!("Upload CPU {:.2} ms", metrics.upload_ms));
                        if let Some(saved_ms) = metrics.saved_ms {
                            ui.label(format!("Save → upload {:.1} ms", saved_ms));
                        }
                    }
                    ui.label(format!(
                        "UI CPU {:.2} ms · {} uploads",
                        self.ui_ms, metrics.uploads
                    ));
                    ui.label(format!(
                        "{} geometry allocations · {:.2} MiB GPU capacity",
                        metrics.buffer_allocations,
                        metrics.geometry_bytes as f64 / 1048576.0
                    ));
                    ui.label(format!("{} uniform uploads", metrics.uniform_writes));
                    ui.label(format!(
                        "{} placement uploads · {} bytes total",
                        metrics.placement_writes, metrics.placement_bytes
                    ));
                    ui.small("CPU timings exclude GPU completion and presentation.");
                });
        }
        if self.show_display_settings {
            egui::Window::new("Display settings")
                .open(&mut self.show_display_settings)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    interface::display_settings(ui, &mut self.show_edges, &mut self.edge_width_px)
                });
        }
        if let Some(layout) = layout_requested {
            if let Some(snapshot) = &mut self.snapshot {
                if snapshot.set_layout(layout) {
                    if let Some(revision) = snapshot.editor_revision {
                        self.code_state
                            .outputs
                            .set(revision, Arc::clone(&snapshot.scene.outputs));
                    }
                    self.selected = None;
                    self.focus_editor = false;
                    frame_layout = true;
                    self.message = if layout == placement::Layout::Print {
                        "Print layout · export checks each part separately"
                    } else {
                        "Assembly layout"
                    }
                    .into();
                }
            }
        }
        let mut parameter_edit = None;
        if self.show_inspector && !self.focus_editor {
            egui::Panel::right("inspector").default_size(320.0).min_size(280.0).max_size(440.0).frame(egui::Frame::side_top_panel(ui.style()).inner_margin(14.0)).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("properties_scroll").show(ui, |ui| {
            if self.script.is_some() {
                interface::section(ui, "MODEL");
                ui.heading(self.snapshot.as_ref().map_or("Luau model", |snapshot| snapshot.name.as_str()));
                ui.small("Solid parts · millimetres");
                ui.separator();
                if let Some(snapshot) = &self.snapshot {
                    if let Some(selection) = &self.selected {
                        interface::section(ui, "SELECTION");
                        if let inspection::Selection::Part { index, .. } = selection {
                            ui.label(&snapshot.parts[*index as usize].name);
                            ui.small("Selected part · X/Y/Z bounds");
                        } else if let Some(latest) = selection.latest() {
                            ui.label(snapshot.scene.annotations.faces.iter().find(|named| named.id == latest.id).map_or("Unnamed face", |named| named.name.as_str()));
                            ui.small(if matches!(selection, inspection::Selection::Pair { .. }) { "Parallel-face separation · reference measurement" } else if selection.planar() { "Planar face spans · reference measurements" } else { "Curved face · X/Y/Z bounds, not surface lengths" });
                        } else {
                            ui.label("Selected edge");
                            ui.small("Reference measurements · evaluated CAD curve");
                            if selection.spans().iter().all(Option::is_none) {
                                ui.small("This curve is neither straight nor circular; no length or radius is reported.");
                            }
                        }
                        for span in selection.spans().into_iter().flatten() {
                            ui.label(format!("{} ≈{:.3} mm", span.direction, span.distance_mm));
                        }
                        if let inspection::Selection::Edge(edge) = selection {
                            if ui.button("Copy edge selector").on_hover_text("Straight edges only. Replace part with the source solid variable and choose a unique name. Use parameter expressions for endpoint coordinates that should follow edits.").clicked() {
                                match source_edge_selector(snapshot, edge) {
                                    Some(selector) => ui.ctx().copy_text(selector),
                                    None => self.message = "Only straight edges support modeling selectors; no selector was copied".into(),
                                }
                            }
                        }
                        if let Some(face) = selection.latest() {
                            if ui.button("Copy face selector").on_hover_text("Axis-aligned planar faces only. Replace part with your final solid variable and choose a unique name.").clicked() {
                                match source_face_selector(snapshot, face.id) {
                                    Some(selector) => ui.ctx().copy_text(selector),
                                    None => self.message = "This face is not axis-aligned and planar; no selector was copied".into(),
                                }
                            }
                        }
                        ui.small("Shift-click a parallel face to measure separation. Click empty space to clear.");
                    }
                    interface::section(ui, "PARTS");
                    egui::ScrollArea::vertical().id_salt("assembly_parts").max_height(160.0).show(ui, |ui| {
                    ui.spacing_mut().interact_size.y = 24.0;
                    ui.spacing_mut().item_spacing.y = 4.0;
                    for (part, canonical) in snapshot.parts.iter().zip(&snapshot.scene.display.parts) {
                        let faces = &canonical.faces;
                        let selected = self.selected.as_ref().is_some_and(|selection| match selection {
                            inspection::Selection::Part { faces: selected, .. } => selected == &[*faces.start(), *faces.end()],
                            _ => selection.latest().is_some_and(|face| faces.contains(&face.id)),
                        });
                        egui::Frame::NONE.fill(if selected { interface::SELECTION } else { egui::Color32::TRANSPARENT }).corner_radius(4.0).inner_margin(egui::vec2(6.0, 2.0)).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let color: egui::Color32 = egui::Rgba::from_rgb(part.color[0], part.color[1], part.color[2]).into();
                                let (swatch, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                                ui.painter().rect_filled(swatch, 3.0, color);
                                ui.add(egui::Label::new(&part.name).truncate()).on_hover_text(&part.name);
                            });
                        });
                    }
                    });
                }
                if let Some(script) = &self.script {
                    let parameters = self.code_state.parameters(&script.buffer, ui.ctx());
                    parameter_edit = self.parameter_state.show(ui, script.buffer.revision, parameters, save_requested || run_requested || export_requested || file_dialog_requested.is_some(), !matches!(self.work, WorkerState::Busy(_) | WorkerState::Export));
                }
                if let Some(snapshot) = &self.snapshot {
                    interface::section(ui, "GEOMETRY");
                    ui.label(format!("{} {:.3} mm³", if snapshot.parts.len() > 1 { "Parts volume" } else { "Volume" }, snapshot.volume_mm3));
                    ui.label(format!("{} faces · {} triangles", snapshot.scene.display.face_count(), snapshot.scene.display.vertex_count() / 3));
                    interface::section(ui, "ASSEMBLY CHECKS");
                    if self.script.as_ref().is_some_and(|script| snapshot.editor_revision != Some(script.buffer.revision)) {
                        ui.colored_label(interface::WARNING, "Measurements refer to the previous valid preview");
                    }
                    let checked = snapshot.checks.measurements.iter().filter(|measurement| measurement.checked).count();
                    ui.label(if checked == 0 { "No explicit distance checks".into() } else { format!("{checked} distance checks passed") });
                    ui.small("Code defines shape. Checks validate declared limits; no constraint solver is active.");
                    for measurement in &snapshot.checks.measurements {
                        ui.label(format!("{} ≈{:.3} mm · {}", measurement.name, measurement.distance_mm, if measurement.checked { "checked" } else { "reference" }));
                    }
                }
                return;
            }
            ui.heading("Rack dimensions"); ui.label("Millimetres"); ui.separator();
            let mut changed = false;
            egui::Grid::new("dimensions").num_columns(2).spacing([10.0, 12.0]).show(ui, |ui| {
                for (label, value, range) in [
                    ("Width", &mut self.parameters.rack_width_mm, 100.0..=480.0),
                    ("Step rise", &mut self.parameters.step_rise_mm, 10.0..=80.0),
                    ("Jar diameter", &mut self.parameters.jar_diameter_mm, 20.0..=100.0),
                    ("Jar height", &mut self.parameters.jar_height_mm, 20.0..=300.0),
                    ("Jar spacing", &mut self.parameters.clearance_mm, 0.5..=10.0),
                    ("Cabinet gap", &mut self.parameters.cabinet_clearance_mm, 1.0..=20.0),
                    ("Wall thickness", &mut self.parameters.thickness_mm, 1.0..=10.0),
                    ("Lip height", &mut self.parameters.lip_height_mm, 1.0..=20.0),
                ] {
                    ui.label(label); changed |= ui.add(egui::DragValue::new(value).range(range).speed(0.1)).changed(); ui.end_row();
                }
                ui.label("Steps"); changed |= ui.add(egui::DragValue::new(&mut self.parameters.step_count).range(1..=8)).changed(); ui.end_row();
            });
            ui.add_space(12.0);
            if ui.button("Reset dimensions").clicked() { self.parameters = PARAMETERS; changed = true; }
            if changed { self.dirty_since = Some(Instant::now()); }
            ui.separator();
            if let Err(error) = self.parameters.layout() { ui.colored_label(interface::WARNING, error); }
            if let Some(snapshot) = &self.snapshot {
                ui.label(format!("{} triangles · {} faces", snapshot.scene.display.vertex_count()/3, snapshot.scene.display.face_count()));
                ui.label(format!("Volume {:.0} mm³", snapshot.volume_mm3));
                if snapshot.source != ModelSource::Rack(self.parameters) { ui.colored_label(interface::WARNING, "Showing the previous valid geometry"); }
            }
            ui.separator(); ui.label(self.selected.as_ref().map_or("Click a face or edge to select".into(), |selection| selection.latest().map_or("Selected edge".into(), |face| format!("Selected face {}", face.id))));
            ui.separator(); ui.small("Preview: rack blank. Print export adds the splitting joints. Joint clearance: 0.2 mm.");
            ui.small("Export uses OpenCascade for STEP and verified print geometry. Print and dry-fit the samples first.");
        });
        });
        }
        let mut sweep_index = None;
        if let (Some(sweep), Some(script)) = (&mut self.sweep, &self.script) {
            if matches!(self.work, WorkerState::Idle)
                && self.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.editor_revision == Some(script.buffer.revision)
                })
            {
                let at = Instant::now();
                if let Some((index, value)) = sweep.next(at) {
                    match self.parameter_state.edit_literal(
                        script.buffer.revision,
                        "separation",
                        value.to_string(),
                    ) {
                        Ok(Some(mut edit)) => {
                            edit.input_at = Some(at);
                            parameter_edit = Some(edit);
                            sweep_index = Some(index);
                        }
                        Ok(None) => {}
                        Err(error) => self.message = error.into(),
                    }
                }
            }
        }
        if let Some(edit) = parameter_edit {
            if let Some(script) = &mut self.script {
                match self.code_state.replace_parameter(&mut script.buffer, &edit) {
                    Ok(()) => {
                        self.parameter_state.applied(&edit, script.buffer.revision);
                        if let (Some(sweep), Some(index)) = (&mut self.sweep, sweep_index) {
                            sweep.applied(index, script.buffer.revision);
                        }
                        self.preview_input = edit
                            .input_at
                            .filter(|_| self.profile.is_some())
                            .map(|at| performance::Input::new(script.buffer.revision, at));
                        self.preview
                            .parameter(script.buffer.revision, Instant::now());
                        self.message = if self.preview.automatic() {
                            "Parameter changed · Auto Preview pending"
                        } else {
                            "Parameter changed in source · Run to update preview"
                        }
                        .into();
                    }
                    Err(error) => {
                        self.message = error.into();
                        save_requested = false;
                        run_requested = false;
                        export_requested = false;
                    }
                }
            }
        }
        if self.show_code || self.focus_editor {
            if let Some(script) = &mut self.script {
                egui::Panel::left(if self.focus_editor {
                    "code_focus"
                } else {
                    "code_editor"
                })
                .default_size(code_editor::preferred_width(ui, &script.buffer))
                .min_size(if self.focus_editor {
                    ui.available_width()
                } else {
                    240.0
                })
                .max_size(if self.focus_editor {
                    ui.available_width()
                } else {
                    (ui.available_width() - 320.0).max(240.0)
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        interface::panel_title(ui, "CODE");
                        ui.label("Luau")
                            .on_hover_text(script.path.display().to_string());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .selectable_label(!self.code_state.outputs.hidden, "Results")
                                .on_hover_text("Show inline part previews and measurements")
                                .clicked()
                            {
                                self.code_state.outputs.hidden = !self.code_state.outputs.hidden;
                            }
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} lines",
                                    script.buffer.rope.len_lines()
                                ))
                                .small()
                                .color(interface::MUTED),
                            );
                        });
                    })
                    .response
                    .on_hover_text(if self.code_state.modal.enabled {
                        "Ctrl+Space complete · Esc normal mode"
                    } else {
                        "Ctrl+Space complete · Ctrl+Z undo"
                    });
                    if self.code_state.modal.mode == modal::Mode::Command {
                        ui.monospace(format!(":{}", self.code_state.modal.command));
                    }
                    if script.conflict.is_some() {
                        ui.colored_label(
                            interface::WARNING,
                            "File changed on disk; your editor text is preserved.",
                        );
                        ui.horizontal(|ui| {
                            if ui.button("Reload disk (discard edits)").clicked() {
                                match script.reload() {
                                    Ok(()) => run_requested = true,
                                    Err(error) => self.message = error,
                                }
                            }
                            if ui.button("Keep editor").clicked() {
                                script.keep_editor();
                            }
                        });
                    }
                    ui.separator();
                    if code_editor::show(ui, &mut script.buffer, &mut self.code_state) {
                        self.message = if self.preview.automatic() {
                            "Source changed · Auto Preview pending"
                        } else {
                            "Source changed · Run previews without saving"
                        }
                        .into();
                    }
                });
            }
        }
        if self.code_state.outputs.focus_requested() {
            self.focus_editor = false;
            self.show_code = true;
            ui.ctx().request_repaint();
        }
        if let Some(action) = self.code_state.action.take() {
            match action {
                modal::Action::Save => save_requested = true,
                modal::Action::Close => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                modal::Action::SaveClose => {
                    save_requested = true;
                    close_after_save = true;
                }
                modal::Action::Run => run_requested = true,
                modal::Action::QuickOpen => palette_requested = Some(palette::Mode::Files),
                modal::Action::Find { backward, replace } => {
                    self.show_code = true;
                    self.code_state.open_search(backward, replace);
                    self.language_popup = None;
                    self.language_request_cursor = None;
                    ui.ctx().request_repaint();
                }
                modal::Action::FindNext(opposite) => {
                    if let Some(script) = &self.script {
                        self.code_state.search_next(&script.buffer, opposite);
                    }
                    ui.ctx().request_repaint();
                }
                modal::Action::ToggleDiagnostics => self.show_diagnostics = !self.show_diagnostics,
                modal::Action::NextDiagnostic | modal::Action::PreviousDiagnostic => {
                    if let (Some(diagnostics), Some(script)) = (&self.diagnostics, &self.script) {
                        if let Some(span) = diagnostics.navigate(
                            &script.buffer,
                            self.code_state.cursor(),
                            matches!(action, modal::Action::NextDiagnostic),
                        ) {
                            self.code_state.jump_to(span.range(&script.buffer));
                            self.show_code = true;
                        }
                    }
                }
                modal::Action::Open(path) => {
                    open_requested = true;
                    if let Some(path) = path {
                        let path = std::path::PathBuf::from(path);
                        let directory = self
                            .script
                            .as_ref()
                            .and_then(|script| script.path.parent())
                            .unwrap_or(std::path::Path::new("."));
                        let path = if path.is_absolute() {
                            path
                        } else {
                            directory.join(path)
                        };
                        if let Err(error) = self.open_path(path, ui.ctx()) {
                            self.message = error;
                        }
                        open_requested = false;
                    }
                }
                action => {
                    if let (Some(diagnostics), Some(script)) = (&mut self.diagnostics, &self.script)
                    {
                        let cursor = self.code_state.cursor();
                        self.language_request_cursor = Some(cursor);
                        let query = match action {
                            modal::Action::Complete => lsp::Query::Complete(cursor),
                            modal::Action::Hover => lsp::Query::Hover(cursor),
                            _ => lsp::Query::Definition(cursor),
                        };
                        diagnostics.request(&script.buffer, query, ui.ctx());
                    }
                }
            }
        }
        if save_requested
            || run_requested
            || export_requested
            || file_dialog_requested == Some(save::Mode::SaveAs)
        {
            if let Some(error) = self.parameter_state.error() {
                save_requested = false;
                run_requested = false;
                export_requested = false;
                if file_dialog_requested == Some(save::Mode::SaveAs) {
                    file_dialog_requested = None;
                }
                self.message = error.into();
            }
        }
        if let Some(script) = &mut self.script {
            if save_requested {
                match script.save() {
                    Ok(()) => {
                        if close_after_save {
                            self.exit = ExitState::Closing;
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if !self.snapshot.as_ref().is_some_and(|snapshot| {
                            snapshot.editor_revision == Some(script.buffer.revision)
                        }) {
                            run_requested = true;
                        } else {
                            self.message = "Saved model; validated geometry reused".into();
                        }
                    }
                    Err(error) => self.message = error,
                }
            }
            if run_requested {
                if let Some(diagnostics) = &mut self.diagnostics {
                    diagnostics.run_started();
                }
                self.preview.run(script.buffer.revision, Instant::now());
                self.message = "Checking editor buffer and building native geometry…".into();
            }
        }
        if export_requested {
            if let Err(error) = self.export(export_target) {
                self.message = error;
            }
        }
        if let (Some(diagnostics), Some(script)) = (&mut self.diagnostics, &self.script) {
            if script.source_ready() {
                diagnostics.sync(&script.buffer, ui.ctx());
            }
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(interface::VIEWPORT))
            .show(ui, |ui| {
                if self.focus_editor {
                    return;
                }
                let (rect, response) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
                if rect.width() <= 0.0 || rect.height() <= 0.0 { return; }
                self.viewport_aspect = rect.width().max(1.0) / rect.height().max(1.0);
                if frame_layout {
                    if let Some(snapshot) = &self.snapshot { self.camera = snapshot_camera(snapshot, self.viewport_aspect); }
                }
                if rect.width() >= 100.0 {
                    if let Some(request) = self.code_state.outputs.take_focus() {
                        if let (Some(snapshot), Some(script)) = (&self.snapshot, &self.script) {
                            if let Some((selection, camera)) = focus_part(snapshot, &request, script.buffer.revision, self.viewport_aspect, &self.camera) {
                                self.selected = Some(selection);
                                self.camera = camera;
                                self.message = format!("Selected part · {}", request.name);
                            }
                        }
                    }
                }
                self.camera.interact(&response, ui);
                if ui.input(|input| input.key_pressed(egui::Key::F))
                    && !ui.ctx().egui_wants_keyboard_input()
                {
                    if let Some(snapshot) = &self.snapshot {
                        self.camera = snapshot_camera(snapshot, self.viewport_aspect);
                    }
                }
                if let Some(snapshot) = &self.snapshot {
                    let matrix = self
                        .camera
                        .matrix(rect.width().max(1.0) / rect.height().max(1.0));
                    interface::grid(
                        ui.painter(),
                        rect,
                        matrix,
                        self.camera.target,
                        self.camera.distance,
                    );
                    if response.clicked() {
                        if let Some(position) = response.interact_pointer_pos() {
                            let extend = ui.input(|input| input.modifiers.shift);
                            let edge = if self.show_edges && !extend {
                                snapshot.scene.picking().pick_edge(matrix, [rect.width(), rect.height()], [position.x - rect.left(), position.y - rect.top()])
                            } else { None };
                            if let Some(edge) = edge {
                                self.selected = Some(inspection::Selection::Edge(edge));
                                self.message = "Edge dimensions · reference measurements".into();
                            } else if let Some(hit) = snapshot.scene.picking().pick(matrix, [
                                (position.x - rect.left()) / rect.width() * 2.0 - 1.0,
                                1.0 - (position.y - rect.top()) / rect.height() * 2.0,
                            ]) {
                                let face = *snapshot.scene.faces.get(&hit.face_id).expect("Mesh face inspection cache");
                                match inspection::Selection::select(self.selected.as_ref(), face, extend) {
                                    Ok(selection) => {
                                        self.message = if matches!(selection, inspection::Selection::Pair { .. }) { "Parallel-face separation · reference measurement".into() } else { "Face dimensions · reference measurements".into() };
                                        self.selected = Some(selection);
                                    }
                                    Err(error) => self.message = error,
                                }
                            } else { self.selected = None; self.message = "Selection cleared".into(); }
                        }
                    }
                    ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                        rect,
                        viewport::Draw {
                            input: snapshot.input,
                            edge_width_px: self.edge_width_px,
                            edges_enabled: self.show_edges,
                            eye: self.camera.eye(),
                            viewport_size: [
                                rect.width() * ui.ctx().pixels_per_point(),
                                rect.height() * ui.ctx().pixels_per_point(),
                            ],
                            saved_at: snapshot.saved_at,
                            geometry: Arc::clone(&snapshot.scene.display),
                            instances: Arc::clone(&snapshot.scene.instances),
                            matrix,
                            selected: self.selected.as_ref().map_or([0; 2], inspection::Selection::ids),
                            selected_part: match &self.selected { Some(inspection::Selection::Part { faces, .. }) => *faces, _ => [0; 2] },
                            selected_edge: match &self.selected { Some(inspection::Selection::Edge(edge)) => Some(edge.segments), _ => None },
                        },
                    ));
                    if self.show_dimensions || self.selected.is_some() {
                        dimensions::draw(ui, rect, matrix, snapshot.scene.bounds, &snapshot.scene.annotations, self.selected.as_ref());
                    }
                }
                ui.painter().text(
                    rect.left_bottom() + egui::vec2(12.0, -12.0),
                    egui::Align2::LEFT_BOTTOM,
                    "Drag: orbit · Shift-drag / right-drag: pan · Scroll: zoom",
                    egui::FontId::proportional(12.0),
                    interface::MUTED,
                );
                interface::axes(ui.painter(), rect, self.camera.eye() - self.camera.target);
                let toolbar = egui::Rect::from_min_size(
                    rect.left_top() + egui::vec2(16.0, 10.0),
                    egui::vec2((rect.width() - 32.0).max(0.0), 112.0),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(toolbar), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let (label, color) =
                            if matches!(self.work, WorkerState::Busy(_) | WorkerState::Export) {
                                ("Processing model", interface::MUTED)
                            } else if self.snapshot.is_none() {
                                ("No preview yet", interface::MUTED)
                            } else if self.script.as_ref().is_some_and(|script| {
                                self.snapshot.as_ref().is_some_and(|snapshot| {
                                    snapshot.editor_revision != Some(script.buffer.revision)
                                })
                            }) {
                                (
                                    if self.preview.automatic() {
                                        if self.preview.pending() {
                                            "Waiting for typing pause"
                                        } else {
                                            "Preview needs attention"
                                        }
                                    } else {
                                        "Code changed · Run to update preview"
                                    },
                                    interface::WARNING,
                                )
                            } else if self.script.is_none()
                                && self.snapshot.as_ref().is_some_and(|snapshot| {
                                    snapshot.source != ModelSource::Rack(self.parameters)
                                })
                            {
                                ("Preview needs update", interface::WARNING)
                            } else {
                                ("Preview up to date", interface::ACCENT)
                            };
                        interface::panel_title(ui, "MODEL");
                        ui.label(egui::RichText::new(label).size(12.0).color(color));
                    });
                    egui::Frame::NONE
                        .fill(interface::PANEL)
                        .stroke(egui::Stroke::new(1.0, interface::BORDER))
                        .corner_radius(8.0)
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.visuals_mut().button_frame = false;
                                if ui
                                    .add(
                                        egui::Button::new("Frame · F")
                                            .min_size(egui::vec2(88.0, interface::CONTROL_HEIGHT)),
                                    )
                                    .clicked()
                                {
                                    if let Some(snapshot) = &self.snapshot {
                                        self.camera =
                                            snapshot_camera(snapshot, self.viewport_aspect);
                                    }
                                }
                                if ui
                                    .add(
                                        egui::Button::new("Front")
                                            .min_size(egui::vec2(64.0, interface::CONTROL_HEIGHT)),
                                    )
                                    .clicked()
                                {
                                    self.camera.yaw = -std::f32::consts::FRAC_PI_2;
                                    self.camera.pitch = 0.0;
                                }
                                if ui
                                    .add(
                                        egui::Button::new("Top")
                                            .min_size(egui::vec2(64.0, interface::CONTROL_HEIGHT)),
                                    )
                                    .clicked()
                                {
                                    self.camera.pitch = 1.55;
                                }
                                ui.toggle_value(&mut self.show_dimensions, "Measures").on_hover_text("Show overall dimensions and declared checks. Selecting a face always shows local dimensions; Shift-click a parallel face for separation.");
                            });
                        });
                });
            });
        if let Some(diagnostics) = &mut self.diagnostics {
            if let Some(insight) = diagnostics.take_insight() {
                if self.language_request_cursor == Some(self.code_state.cursor()) {
                    self.language_popup = Some(insight);
                }
                self.completion_index = 0;
                ui.ctx().request_repaint();
            }
        }
        if self.language_popup.is_some()
            && self.language_request_cursor != Some(self.code_state.cursor())
        {
            self.language_popup = None;
        }
        if self.language_popup.as_ref().is_some_and(|(revision, _)| {
            !self
                .script
                .as_ref()
                .is_some_and(|script| script.buffer.revision == *revision)
        }) {
            self.language_popup = None;
        }
        let mut completion = None;
        let mut definition = None;
        let mut popup_open = true;
        if let Some((revision, insight)) = &self.language_popup {
            match insight {
                lsp::Insight::Completions(items) => {
                    self.completion_index =
                        self.completion_index.min(items.len().saturating_sub(1));
                    egui::Window::new("Completions")
                        .open(&mut popup_open)
                        .default_pos(egui::pos2(30.0, 160.0))
                        .default_width(440.0)
                        .show(ui.ctx(), |ui| {
                            if items.is_empty() {
                                ui.label("No completions at this position");
                            }
                            ui.small("Up / Down · Enter / Tab accept · Esc dismiss");
                            egui::ScrollArea::vertical()
                                .max_height(260.0)
                                .show(ui, |ui| {
                                    for (index, item) in items.iter().enumerate() {
                                        let response = ui
                                            .selectable_label(
                                                index == self.completion_index,
                                                &item.label,
                                            )
                                            .on_hover_text(&item.detail);
                                        if completion_navigate && index == self.completion_index {
                                            response.scroll_to_me(None);
                                        }
                                        if response.clicked()
                                            || completion_accept && index == self.completion_index
                                        {
                                            completion = Some((
                                                *revision,
                                                item.replacement.clone(),
                                                item.text.clone(),
                                            ));
                                        }
                                    }
                                });
                        });
                }
                lsp::Insight::Hover(text) => {
                    egui::Window::new("Type information")
                        .open(&mut popup_open)
                        .default_pos(egui::pos2(30.0, 160.0))
                        .default_width(520.0)
                        .show(ui.ctx(), |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(300.0)
                                .show(ui, |ui| {
                                    ui.monospace(if text.is_empty() {
                                        "No type information at this position"
                                    } else {
                                        text
                                    });
                                });
                        });
                }
                lsp::Insight::Definitions(locations) => {
                    if locations.len() == 1 {
                        definition = Some(locations[0].clone());
                    } else {
                        egui::Window::new("Definitions").open(&mut popup_open).show(
                            ui.ctx(),
                            |ui| {
                                if locations.is_empty() {
                                    ui.label("No definition at this position");
                                }
                                for location in locations {
                                    if ui
                                        .button(format!(
                                            "{}:{}",
                                            location.path.display(),
                                            location.start.0 + 1
                                        ))
                                        .clicked()
                                    {
                                        definition = Some(location.clone());
                                    }
                                }
                            },
                        );
                    }
                }
            }
        }
        if !popup_open {
            self.language_popup = None;
        }
        if let Some((revision, range, text)) = completion {
            if let Some(script) = &mut self.script {
                if script.buffer.revision == revision {
                    self.code_state
                        .replace_selection(&mut script.buffer, range, &text);
                }
            }
            self.language_popup = None;
            ui.ctx().request_repaint();
        }
        if let Some(location) = definition {
            self.language_popup = None;
            if let Some(script) = &self.script {
                if script.path == location.path {
                    self.code_state.jump_to(location.range(&script.buffer.rope));
                    self.show_code = true;
                } else {
                    match files::Picker::new(
                        location
                            .path
                            .parent()
                            .unwrap_or(std::path::Path::new("."))
                            .to_owned(),
                        ui.ctx().clone(),
                    ) {
                        Ok(mut picker) => {
                            picker.open_path(location.path.clone());
                            self.pending_location = Some(location);
                            self.picker = Some(picker);
                        }
                        Err(error) => self.message = error,
                    }
                }
            }
            ui.ctx().request_repaint();
        }
        if let Some(mode) = palette_requested {
            if navigation_allowed {
                let root = self
                    .script
                    .as_ref()
                    .and_then(|script| script.path.parent())
                    .unwrap_or(std::path::Path::new("."))
                    .to_owned();
                self.palette.open(mode, root, ui.ctx());
                self.language_popup = None;
                self.language_request_cursor = None;
            }
        }
        if open_requested {
            self.pending_location = None;
            let directory = self
                .script
                .as_ref()
                .and_then(|script| script.path.parent())
                .map(std::path::Path::to_owned)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));
            match files::Picker::new(directory, ui.ctx().clone()) {
                Ok(picker) => self.picker = Some(picker),
                Err(error) => self.message = error,
            }
        }
        if let Some(example) = example_requested {
            self.pending_location = None;
            match files::Picker::new(std::path::PathBuf::from("."), ui.ctx().clone()) {
                Ok(mut picker) => {
                    picker.open_example(example);
                    self.picker = Some(picker);
                }
                Err(error) => self.message = error,
            }
        }
        if let Some(mode) = file_dialog_requested {
            if navigation_allowed {
                let origin = self
                    .script
                    .as_ref()
                    .map(|script| script.path.clone())
                    .unwrap_or_else(|| {
                        std::env::current_dir()
                            .unwrap_or_else(|_| ".".into())
                            .join("untitled.luau")
                    });
                match save::Dialog::new(mode, origin, ui.ctx().clone()) {
                    Ok(dialog) => {
                        self.save_dialog = Some(dialog);
                        self.language_popup = None;
                        self.language_request_cursor = None;
                    }
                    Err(error) => self.message = error,
                }
            }
        }
        let save_result = self.save_dialog.as_mut().map(|dialog| {
            let (closed, selection) = dialog.show(
                ui.ctx(),
                self.script
                    .as_ref()
                    .map(|script| (&script.buffer.rope, script.buffer.revision)),
            );
            (closed, selection, dialog.origin.clone())
        });
        if let Some((closed, selection, origin)) = save_result {
            if closed {
                self.save_dialog = None;
                self.code_state.focus();
            }
            if let Some(selection) = selection {
                match selection {
                    save::Selection::New(path) => {
                        self.pending_document = Some(document::Document::draft(path))
                    }
                    save::Selection::Saved(saved) => {
                        if let Some(script) = &mut self.script {
                            if script.path == origin {
                                let mut warning = None;
                                script.accept_save(
                                    saved.path.clone(),
                                    saved.source,
                                    saved.revision,
                                );
                                self.palette.remember(saved.path.clone());
                                self.watcher = match document::Watcher::new(
                                    saved.path.clone(),
                                    ui.ctx().clone(),
                                ) {
                                    Ok(watcher) => Some(watcher),
                                    Err(error) => {
                                        warning = Some(error);
                                        None
                                    }
                                };
                                self.diagnostics = match diagnostics::Session::new(
                                    saved.path.clone(),
                                    ui.ctx().clone(),
                                ) {
                                    Ok(diagnostics) => Some(diagnostics),
                                    Err(error) => {
                                        warning = Some(error);
                                        None
                                    }
                                };
                                if script.buffer.revision == saved.revision
                                    && !self.snapshot.as_ref().is_some_and(|snapshot| {
                                        snapshot.editor_revision == Some(saved.revision)
                                    })
                                {
                                    self.preview.run(saved.revision, Instant::now());
                                }
                                self.message = format!(
                                    "Saved as {}{}",
                                    saved.path.display(),
                                    warning
                                        .map(|error| format!(" · {error}"))
                                        .unwrap_or_default()
                                );
                            } else {
                                self.message = format!(
                                    "Saved a copy to {} · active model preserved",
                                    saved.path.display()
                                );
                            }
                        }
                    }
                }
            }
        }
        if let Some(picker) = &mut self.picker {
            if let Some(document) = picker.show(ui.ctx()) {
                self.pending_document = Some(document);
            }
        }
        if self.pending_document.is_some()
            && !self.script.as_ref().is_some_and(document::Document::edited)
        {
            let document = self.pending_document.take().expect("Loaded document");
            if let Err(error) = self.adopt_document(document, ui.ctx()) {
                self.message = error;
            }
        }
        if self.pending_document.is_some() {
            let mut accept = false;
            let mut cancel = false;
            egui::Modal::new(egui::Id::unique("switch_document")).show(ui.ctx(), |ui| {
                ui.heading("Unsaved changes");
                ui.label("Save the current model before opening another file?");
                ui.horizontal(|ui| {
                    if ui.button("Save and open").clicked() {
                        if let Some(script) = &mut self.script {
                            match script.save() {
                                Ok(()) => accept = true,
                                Err(error) => self.message = error,
                            }
                        }
                    }
                    accept |= ui.button("Discard and open").clicked();
                    cancel |= ui.button("Cancel").clicked();
                });
            });
            if cancel {
                self.pending_document = None;
                self.pending_location = None;
                self.code_state.focus();
            }
            if accept {
                if let Some(document) = self.pending_document.take() {
                    if let Err(error) = self.adopt_document(document, ui.ctx()) {
                        self.message = error;
                    }
                }
            }
        }
        if !matches!(self.exit, ExitState::Closing) {
            if ui.input(|input| input.viewport().close_requested()) {
                if self.save_dialog.as_ref().is_some_and(save::Dialog::writing) {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    self.message = "Wait for Save As to finish before closing".into();
                    return;
                }
                self.save_dialog = None;
                self.pending_document = None;
                self.pending_location = None;
                if let Some(picker) = &mut self.picker {
                    picker.open = false;
                }
                if self.script.as_ref().is_some_and(document::Document::edited) {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    self.exit = ExitState::ConfirmDiscard;
                }
            }
        }
        if matches!(self.exit, ExitState::ConfirmDiscard) {
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ui.ctx(), |ui| {
                    ui.label("Save your Luau changes before closing?");
                    ui.horizontal(|ui| {
                        if ui.button("Save and close").clicked() {
                            if let Some(script) = &mut self.script {
                                match script.save() {
                                    Ok(()) => {
                                        self.exit = ExitState::Closing;
                                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                                    }
                                    Err(error) => self.message = error,
                                }
                            }
                        }
                        if ui.button("Discard and close").clicked() {
                            self.exit = ExitState::Closing;
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui.button("Cancel").clicked() {
                            self.exit = ExitState::Open;
                        }
                    });
                });
        }
        self.observe_preview();
        self.start_preview();
        self.ui_ms = started.elapsed().as_secs_f64() * 1000.0;
        if let Some(profile) = &mut self.profile {
            let size = ui.ctx().content_rect().size() * ui.ctx().pixels_per_point();
            profile.frame(
                [size.x as u32, size.y as u32],
                self.ui_ms,
                viewport::metrics(frame),
            );
        }
        if matches!(self.work, WorkerState::Idle) {
            if let Some(delay) = self.preview.delay(Instant::now()) {
                ui.ctx().request_repaint_after(delay);
            }
            if let Some(changed) = self.dirty_since {
                ui.ctx().request_repaint_after(
                    Duration::from_millis(150).saturating_sub(changed.elapsed()),
                );
            }
        }
        if let Some(sweep) = &self.sweep {
            let now = Instant::now();
            let metrics = viewport::metrics(frame);
            if sweep.finish(metrics.input_latency.map(|latency| latency.revision), now) {
                eprintln!(
                    "Profile: preview_sweep_gpu uploads={} geometry_allocations={} geometry_bytes={} placement_uploads={}",
                    metrics.uploads,
                    metrics.buffer_allocations,
                    metrics.geometry_bytes,
                    metrics.placement_writes
                );
                self.sweep = None;
                self.exit = ExitState::Closing;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ui.ctx().request_repaint_after(sweep.delay(now));
            }
        }
    }
}

fn source_face_selector(snapshot: &Snapshot, id: u32) -> Option<String> {
    let index = snapshot
        .scene
        .display
        .parts
        .iter()
        .position(|part| part.faces.contains(&id))?;
    snapshot.parts.get(index)?;
    let part = &snapshot.scene.display.parts[index];
    dimensions::selector(&part.geometry.mesh, id - part.faces.start() + 1)
}

fn source_edge_selector(snapshot: &Snapshot, edge: &inspection::Edge) -> Option<String> {
    let picking = snapshot.scene.picking();
    let (index, edge) = picking.source_edge(edge.segments)?;
    snapshot.parts.get(index)?;
    edge_reference::selector(edge)
}

fn focus_part(
    snapshot: &Snapshot,
    request: &inline_output::Focus,
    revision: u64,
    aspect: f32,
    previous: &viewport::Camera,
) -> Option<(inspection::Selection, viewport::Camera)> {
    assert!(aspect.is_finite());
    assert!(aspect > 0.0);
    if request.revision != revision || snapshot.editor_revision != Some(revision) {
        return None;
    }
    let index = snapshot
        .parts
        .iter()
        .position(|part| part.name == request.name)?;
    let faces = &snapshot.scene.display.parts.get(index)?.faces;
    let bounds = *snapshot.scene.part_bounds.get(index)?;
    let mut camera = viewport::Camera::rack((bounds[1] - bounds[0]).as_dvec3().to_array());
    camera.target = (bounds[0] + bounds[1]) / 2.0;
    camera.distance /= aspect.min(1.0);
    camera.yaw = previous.yaw;
    camera.pitch = previous.pitch;
    Some((
        inspection::Selection::Part {
            index: index as u32,
            faces: [*faces.start(), *faces.end()],
            bounds,
        },
        camera,
    ))
}

fn snapshot_camera(snapshot: &Snapshot, aspect: f32) -> viewport::Camera {
    assert!(aspect.is_finite());
    assert!(aspect > 0.0);
    let [minimum, maximum] = snapshot.scene.bounds;
    let mut camera = viewport::Camera::rack((maximum - minimum).as_dvec3().to_array());
    camera.target = (minimum + maximum) / 2.0;
    camera.distance /= aspect.min(1.0);
    camera
}

#[cfg(feature = "rack-tools")]
fn benchmark() -> Result<(), String> {
    let kernel = kernel::Kernel::new()?;
    let mut smlib = Vec::with_capacity(7);
    let mut occt = Vec::with_capacity(7);
    let mut mesh_ms = Vec::with_capacity(7);
    let mut volume = 0.0;
    for _ in 0..7 {
        let start = Instant::now();
        let mut shape = kernel.rack(PARAMETERS)?;
        smlib.push(start.elapsed().as_secs_f64() * 1000.0);
        let start = Instant::now();
        let mesh = shape.mesh()?;
        mesh_ms.push(start.elapsed().as_secs_f64() * 1000.0);
        volume = mesh.volume_mm3();
        let exact = shape.volume()?;
        if (exact - volume).abs() > 0.01 {
            return Err(format!(
                "SMLib exact/mesh volumes disagree: {exact} / {volume}"
            ));
        }
        let start = Instant::now();
        let reference = build_rack(PARAMETERS)?;
        occt.push(start.elapsed().as_secs_f64() * 1000.0);
        let mesh = reference.mesh().map_err(|error| error.to_string())?;
        let reference_volume: f64 = mesh
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|triangle| {
                mesh.vertices[triangle[0]]
                    .dot(mesh.vertices[triangle[1]].cross(mesh.vertices[triangle[2]]))
                    / 6.0
            })
            .sum();
        if (reference_volume - volume).abs() > 0.01 {
            return Err(format!(
                "Kernel volumes disagree: OCCT {reference_volume} / SMLib {volume}"
            ));
        }
    }
    for measurements in [&mut smlib, &mut occt, &mut mesh_ms] {
        measurements.remove(0);
        measurements.sort_by(f64::total_cmp);
    }
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let result = format!(
        "{{\n  \"profile\": \"{profile}\",\n  \"workload\": \"default rack blank, identical ordered box unions; OCCT clean included\",\n  \"warm_samples\": 6,\n  \"smlib_build_median_ms\": {},\n  \"occt_build_median_ms\": {},\n  \"smlib_mesh_and_audit_median_ms\": {},\n  \"volume_mm3\": {volume}\n}}\n",
        (smlib[2] + smlib[3]) / 2.0,
        (occt[2] + occt[3]) / 2.0,
        (mesh_ms[2] + mesh_ms[3]) / 2.0
    );
    std::fs::create_dir_all("output/native").map_err(|error| error.to_string())?;
    std::fs::write("output/native/benchmark.json", &result).map_err(|error| error.to_string())?;
    print!("{result}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_preparation_allocations_are_independent_of_tessellation_density() -> Result<(), String>
    {
        fn prepare(radius: u32) -> Result<(usize, allocations::Counts), String> {
            let kernel = kernel::Kernel::new()?;
            let mut cache = model::Cache::new(&kernel);
            let model = cache.compile(&format!(
                "cad.show('part',cad.cylinder({{radius={radius},height=20}}))"
            ))?;
            let prepared = cache.prepare(&model)?;
            let display = cache.display(&prepared.geometry)?;
            let presentations = cache.presentations(&model, placement::Layout::Assembly)?;
            let meshes: Vec<_> = prepared
                .geometry
                .iter()
                .map(|geometry| &geometry.mesh)
                .collect();
            let checks = model.annotations(&meshes)?;
            let (scene, counts) = allocations::measure(|| {
                build_scene(
                    &model,
                    &prepared.geometry,
                    &presentations,
                    display,
                    placement::Layout::Assembly,
                    &checks,
                )
            });
            scene?;
            Ok((prepared.geometry[0].mesh.vertices.len(), counts))
        }
        let (coarse, coarse_counts) = prepare(1)?;
        let (fine, fine_counts) = prepare(1000)?;
        assert!(fine > coarse * 4);
        assert_eq!(coarse_counts, fine_counts);
        eprintln!(
            "Scene preparation: {coarse}/{fine} vertices, {} Rust allocation calls and {} requested bytes in either case",
            fine_counts.calls, fine_counts.bytes
        );
        Ok(())
    }

    #[test]
    fn placement_checks_rerun_when_type_checks_and_geometry_are_reused() -> Result<(), String> {
        let source = "local gap=cad.number(10,{min=0,max=100});local a=cad.box({width=10,depth=10,height=2});local b=cad.box({width=10,depth=10,height=2});local top=cad.face(a,{name='top',normal='z+',at=2});local bottom=cad.face(b,{name='bottom',normal='z-',at=0});cad.measure('gap',{from=top,to=bottom,min=7.5,max=8.5});cad.show('a',a,{assembly={rotation={y=90},position={x=20}},print={}});cad.show('b',b,{assembly={rotation={y=90},position={x=20+gap}},print={position={x=40}}})";
        let kernel = kernel::Kernel::new()?;
        let mut cache = model::Cache::new(&kernel);
        let first = build_script_snapshot_cached(source.into(), 1, &mut cache)?;
        let error = build_script_snapshot_cached(
            source.replacen("number(10,", "number(20,", 1),
            2,
            &mut cache,
        )
        .err()
        .ok_or("Placement check must fail")?;
        assert!(error.contains("Constraint 'gap'"));
        let recovered = build_script_snapshot_cached(source.into(), 3, &mut cache)?;
        assert_eq!(recovered.cached_parts, 2);
        assert!(Arc::ptr_eq(&first.scene.display, &recovered.scene.display));
        assert!((recovered.checks.measurements[0].distance_mm - 8.0).abs() < 0.001);
        Ok(())
    }

    #[test]
    fn placement_edits_reuse_gpu_geometry_and_thumbnails_but_refresh_inspection()
    -> Result<(), String> {
        let kernel = kernel::Kernel::new()?;
        let mut cache = model::Cache::new(&kernel);
        let source = "local movement=cad.number(0,{min=0,max=100});local spin=cad.number(0,{min=0,max=90});local radius=cad.number(10,{min=1,max=20});local part=cad.cylinder({radius=radius,height=20});cad.show('part',part,{assembly={position={x=movement},rotation={y=spin}},print={rotation={x=90},position={x=movement,z=radius}}})";
        let first = build_script_snapshot_cached(source.into(), 1, &mut cache)?;
        let moved = source.replacen("number(0,", "number(25,", 1);
        let second = build_script_snapshot_cached(moved.clone(), 2, &mut cache)?;
        assert_eq!(second.cached_parts, 1);
        assert_eq!(second.build_ms, 0.0);
        assert!(Arc::ptr_eq(&first.scene.display, &second.scene.display));
        let face = *first.scene.faces.values().next().expect("Face");
        let selection = second
            .scene
            .selection(&inspection::Selection::Face(face))
            .expect("Moved selection");
        let inspection::Selection::Face(moved_face) = selection else {
            panic!("Selected face");
        };
        assert!((moved_face.center - face.center - glam::vec3(25.0, 0.0, 0.0)).length() < 0.001);
        let edge = first.scene.world_edges().items[0];
        let inspection::Selection::Edge(moved_edge) = second
            .scene
            .selection(&inspection::Selection::Edge(edge))
            .expect("Moved edge")
        else {
            panic!("Selected edge");
        };
        assert_eq!(moved_edge.segments, edge.segments);
        let part = inspection::Selection::Part {
            index: 0,
            faces: [1, first.scene.world_mesh().face_count],
            bounds: first.scene.bounds,
        };
        let inspection::Selection::Part { bounds, .. } =
            second.scene.selection(&part).expect("Moved part")
        else {
            panic!("Selected part");
        };
        assert_eq!(bounds, second.scene.part_bounds[0]);
        for (a, b) in [
            (&first.scene, &second.scene),
            (
                first.alternate.as_ref().expect("Print"),
                second.alternate.as_ref().expect("Print"),
            ),
        ] {
            let image = |scene: &Scene| match &scene.outputs[0].content {
                inline_output::Content::Part {
                    image: Some(image), ..
                } => Arc::clone(image),
                _ => panic!("Part thumbnail"),
            };
            assert!(Arc::ptr_eq(&image(a), &image(b)));
            assert!((b.bounds[0] - a.bounds[0] - glam::vec3(25.0, 0.0, 0.0)).length() < 0.001);
            for (id, face) in &a.faces {
                let moved = b.faces[id];
                assert!((moved.center - face.center - glam::vec3(25.0, 0.0, 0.0)).length() < 0.001);
                for (before, after) in face
                    .spans
                    .iter()
                    .flatten()
                    .zip(moved.spans.iter().flatten())
                {
                    assert_eq!(before.distance_mm, after.distance_mm);
                    assert!(
                        (after.points[0] - before.points[0] - glam::vec3(25.0, 0.0, 0.0)).length()
                            < 0.001
                    );
                }
            }
        }
        let rotated = moved.replace("spin=cad.number(0,", "spin=cad.number(45,");
        let third = build_script_snapshot_cached(rotated.clone(), 3, &mut cache)?;
        assert!(Arc::ptr_eq(&second.scene.display, &third.scene.display));
        assert!((third.scene.bounds[1] - second.scene.bounds[1]).length() > 1.0);
        let changed = build_script_snapshot_cached(
            rotated.replace("radius=cad.number(10,", "radius=cad.number(12,"),
            4,
            &mut cache,
        )?;
        assert_eq!(changed.cached_parts, 0);
        assert!(!Arc::ptr_eq(&third.scene.display, &changed.scene.display));
        assert!(changed.volume_mm3 > third.volume_mm3);
        Ok(())
    }

    #[test]
    fn canonical_curves_keep_tight_bounds_face_ids_and_rotated_normals() -> Result<(), String> {
        let source = "local part=cad.cylinder({radius=10,height=20});cad.show('curved',part,{assembly={rotation={x=30,y=45,z=15},position={x=50}},print={rotation={x=90},position={z=10}}})";
        let model = model::compile(source)?;
        let mut snapshot = build_script_snapshot(source, 1)?;
        let kernel = kernel::Kernel::new()?;
        let mut bodies = model.build(&kernel)?;
        let canonical = model.meshes(&mut bodies, false)?.remove(0);
        for layout in [placement::Layout::Assembly, placement::Layout::Print] {
            snapshot.set_layout(layout);
            let pose = model.parts[0].placement(layout);
            pose.apply(&mut bodies[0], false)?;
            let exact = bodies[0].bounds(true)?.map(|point| point.as_vec3());
            assert!((snapshot.scene.bounds[0] - exact[0]).abs().max_element() < 0.001);
            assert!((snapshot.scene.bounds[1] - exact[1]).abs().max_element() < 0.001);
            pose.apply(&mut bodies[0], true)?;
            assert_eq!(
                snapshot.scene.world_mesh().vertices.len(),
                canonical.vertices.len()
            );
            for (actual, source) in snapshot
                .scene
                .world_mesh()
                .vertices
                .iter()
                .zip(&canonical.vertices)
            {
                assert_eq!(actual.face_id, source.face_id);
                let position = pose
                    .matrix()
                    .transform_point3(glam::Vec3::from_array(source.position));
                let normal = pose
                    .matrix()
                    .transform_vector3(glam::Vec3::from_array(source.normal));
                assert!(position.distance(glam::Vec3::from_array(actual.position)) < 0.001);
                assert!(normal.dot(glam::Vec3::from_array(actual.normal)) > 0.9999);
            }
            assert!(
                snapshot
                    .scene
                    .world_edges()
                    .items
                    .iter()
                    .filter(|edge| matches!(edge.geometry, inspection::EdgeGeometry::Circle { .. }))
                    .count()
                    >= 2
            );
            for edge in &snapshot.scene.world_edges().items {
                if let inspection::EdgeGeometry::Circle { radial, .. } = edge.geometry {
                    assert!((radial.length() - 10.0).abs() < 0.001);
                }
            }
        }
        Ok(())
    }

    #[test]
    fn cached_layouts_preserve_assembly_checks_and_select_print_placements_for_export()
    -> Result<(), String> {
        let source = "local a=cad.box({width=10,depth=10,height=2})\nlocal b=cad.box({width=10,depth=10,height=2})\nlocal top=cad.face(a,{name='top',normal='z+',at=2})\nlocal bottom=cad.face(b,{name='bottom',normal='z-',at=0})\ncad.measure('gap',{from=top,to=bottom,min=7.5,max=8.5})\ncad.show('a',a,{assembly={rotation={y=90},position={x=20}},print={}})\ncad.show('b',b,{assembly={rotation={y=90},position={x=30}},print={position={x=40}}})";
        let mut snapshot = build_script_snapshot(source, 1)?;
        snapshot.editor_revision = Some(17);
        let assembly_mesh = Arc::clone(&snapshot.scene.display);
        let print_mesh = Arc::clone(
            &snapshot
                .alternate
                .as_ref()
                .ok_or("Missing print cache")?
                .display,
        );
        assert_eq!(snapshot.checks.measurements[0].distance_mm, 8.0);
        assert!(
            (snapshot.scene.part_bounds[0][0] - glam::vec3(20.0, 0.0, -10.0))
                .abs()
                .max_element()
                < 0.001
        );
        assert!(
            (snapshot.scene.part_bounds[0][1] - glam::vec3(22.0, 10.0, 0.0))
                .abs()
                .max_element()
                < 0.001
        );
        for _ in 0..8 {
            assert!(snapshot.set_layout(placement::Layout::Print));
            assert!(Arc::ptr_eq(&snapshot.scene.display, &print_mesh));
            assert!(snapshot.scene.annotations.measurements.is_empty());
            assert!(
                snapshot
                    .scene
                    .part_bounds
                    .iter()
                    .all(|bounds| bounds[0].z.abs() < 0.001)
            );
            assert_eq!(snapshot.checks.measurements[0].distance_mm, 8.0);
            assert_eq!(snapshot.editor_revision, Some(17));
            assert_eq!(snapshot.revision, 1);
            let request = inline_output::Focus {
                revision: 17,
                name: "b".into(),
            };
            let (_, camera) = focus_part(
                &snapshot,
                &request,
                17,
                1.0,
                &snapshot_camera(&snapshot, 1.0),
            )
            .ok_or("Print part focus")?;
            assert_eq!(camera.target, glam::vec3(45.0, 5.0, 1.0));
            assert!(snapshot.set_layout(placement::Layout::Assembly));
            assert!(Arc::ptr_eq(&snapshot.scene.display, &assembly_mesh));
            assert!(!snapshot.set_layout(placement::Layout::Assembly));
        }
        for layout in [placement::Layout::Assembly, placement::Layout::Print] {
            snapshot.set_layout(layout);
            let face = source_face_selector(&snapshot, snapshot.checks.faces[0].id)
                .ok_or("Source face selector")?;
            assert!(face.contains("normal = \"z+\""), "{face}");
            let copied = format!(
                "local part=cad.box({{width=10,depth=10,height=2}});local face={face};cad.show('copy',part)"
            );
            build_script_snapshot(&copied, 2)?;
            let edge = source_edge_selector(&snapshot, &snapshot.scene.world_edges().items[0])
                .ok_or("Source edge selector")?;
            let copied = format!(
                "local part=cad.box({{width=10,depth=10,height=2}});local edge={edge};part=cad.fillet(part,{{radius=0.2,edges={{edge}}}});cad.show('copy',part)"
            );
            build_script_snapshot(&copied, 3)?;
        }
        let folder = std::env::temp_dir().join(format!("spice-cad-layout-{}", std::process::id()));
        export::model_files(source, &folder, export::Target::Print)?;
        for index in 1..=2 {
            let shape = opencascade::primitives::Shape::read_step(
                folder.join(format!("part-{index:02}.step")),
            )
            .map_err(|error| error.to_string())?;
            let bounds = opencascade::bounding_box::aabb(&shape);
            assert!(bounds.min().z.abs() < 0.001);
            assert!(
                (bounds.max() - bounds.min() - glam::dvec3(10.0, 10.0, 2.0))
                    .abs()
                    .max_element()
                    < 0.001
            );
        }
        assert_eq!(
            std::fs::read_to_string(folder.join("model.luau")).map_err(|error| error.to_string())?,
            source
        );
        std::fs::remove_dir_all(&folder).map_err(|error| error.to_string())?;
        let invalid = source.replace("position={x=30}", "position={x=35}");
        assert!(
            build_script_snapshot(&invalid, 2)
                .err()
                .ok_or("Assembly check must fail")?
                .contains("Constraint 'gap'")
        );
        assert!(export::model_files(&invalid, &folder, export::Target::Print).is_err());
        assert!(!folder.exists());
        Ok(())
    }

    #[test]
    fn inline_focus_selects_the_named_part_and_frames_its_bounds_without_rebuilding()
    -> Result<(), String> {
        let mut snapshot = build_script_snapshot(
            "cad.show('near',cad.box({width=10,depth=10,height=10}))\ncad.show('far',cad.box({width=20,depth=20,height=20,x=200,y=-50,z=35}))",
            1,
        )?;
        snapshot.editor_revision = Some(41);
        let request = inline_output::Focus {
            revision: 41,
            name: "far".into(),
        };
        let mesh = Arc::clone(&snapshot.scene.display);
        let mut previous = snapshot_camera(&snapshot, 1.0);
        previous.yaw = 0.4;
        previous.pitch = -0.15;
        for aspect in [0.25, 0.5, 1.0, 2.0] {
            let (selection, camera) = focus_part(&snapshot, &request, 41, aspect, &previous)
                .ok_or("Missing part focus")?;
            let inspection::Selection::Part {
                index,
                faces,
                bounds,
            } = selection
            else {
                panic!("Expected part selection")
            };
            assert_eq!(index, 1);
            assert_eq!(
                faces,
                [
                    *snapshot.scene.display.parts[1].faces.start(),
                    *snapshot.scene.display.parts[1].faces.end()
                ]
            );
            assert!(faces[0] > *snapshot.scene.display.parts[0].faces.end());
            assert_eq!(camera.target, glam::vec3(210.0, -40.0, 45.0));
            assert_eq!([camera.yaw, camera.pitch], [previous.yaw, previous.pitch]);
            assert_eq!(selection.ids(), [0; 2]);
            assert!(selection.latest().is_none());
            assert!(
                selection
                    .spans()
                    .into_iter()
                    .flatten()
                    .all(|span| span.distance_mm == 20.0)
            );
            let matrix = camera.matrix(aspect);
            for x in [bounds[0].x, bounds[1].x] {
                for y in [bounds[0].y, bounds[1].y] {
                    for z in [bounds[0].z, bounds[1].z] {
                        let clip = matrix * glam::vec4(x, y, z, 1.0);
                        assert!(clip.w > 0.0);
                        assert!(clip.x.abs() <= clip.w);
                        assert!(clip.y.abs() <= clip.w);
                    }
                }
            }
            let face = snapshot.scene.faces[&faces[0]];
            assert_eq!(
                inspection::Selection::select(Some(&selection), face, true)?.ids(),
                [face.id, 0]
            );
        }
        assert!(Arc::ptr_eq(&mesh, &snapshot.scene.display));
        assert!(focus_part(&snapshot, &request, 42, 1.0, &previous).is_none());
        let missing = inline_output::Focus {
            revision: 41,
            name: "missing".into(),
        };
        assert!(focus_part(&snapshot, &missing, 41, 1.0, &previous).is_none());
        snapshot.editor_revision = Some(42);
        assert!(focus_part(&snapshot, &request, 41, 1.0, &previous).is_none());
        Ok(())
    }

    #[test]
    fn build_identity_rejects_results_from_older_edits_and_other_documents() {
        let first = text_buffer::Buffer::from("first");
        let second = text_buffer::Buffer::from("second");
        let key = ModelRevision::Script(first.revision);
        assert!(key.current(Some(first.revision), PARAMETERS));
        assert!(!key.current(Some(second.revision), PARAMETERS));
        assert!(!key.current(None, PARAMETERS));
        let rack = ModelRevision::Rack(PARAMETERS);
        assert!(rack.current(None, PARAMETERS));
        assert!(!rack.current(Some(second.revision), PARAMETERS));
        assert!(!rack.current(
            None,
            RackParameters {
                rack_width_mm: PARAMETERS.rack_width_mm - 1.0,
                ..PARAMETERS
            }
        ));
    }

    #[test]
    fn display_edges_follow_cad_curves_without_triangle_diagonals_or_periodic_seams()
    -> Result<(), String> {
        let kernel = kernel::Kernel::new()?;
        let mut body = kernel.box_shape([0.0; 3], [10.0, 20.0, 30.0])?;
        assert_eq!(body.edges()?.segments.len(), 12);
        let mut mesh = body.mesh()?;
        mesh.smooth_normals();
        for vertex in &mesh.vertices {
            assert!((glam::Vec3::from_array(vertex.normal).abs().max_element() - 1.0).abs() < 1e-6);
        }
        let cylinder = kernel.cylinder([0.0; 3], 3.0, 8.0)?;
        let edges = cylinder.edges()?.segments;
        assert!(edges.len() >= 32);
        assert!(edges.len() < 256);
        for edge in edges {
            assert!((edge.start[2] - edge.end[2]).abs() < 1e-5);
            for point in [edge.start, edge.end] {
                assert!((glam::Vec2::new(point[0], point[1]).length() - 3.0).abs() < 1e-5);
            }
        }
        Ok(())
    }

    #[test]
    fn curved_models_export_native_step_and_closed_stl() -> Result<(), String> {
        let bracket = include_str!("../../models/mounting-bracket.luau");
        let changed = bracket.replace("cad.number(100,", "cad.number(120,");
        let cavity = "local a = cad.box({width=10, depth=10, height=10}); local b = cad.box({width=6, depth=6, height=6,x=2,y=2,z=2}); cad.show('hollow', cad.subtract(a,b))";
        let bushing = include_str!("../../models/revolved-bushing.luau").replace(
            "cad.revolve(section, { angle = sweep_angle })",
            "cad.translate(cad.revolve(section, { angle = sweep_angle }), {x=16,y=16})",
        );
        for (index, source, dimensions) in [
            (0, bracket, [100.0, 50.0, 40.0]),
            (1, changed.as_str(), [120.0, 50.0, 40.0]),
            (2, cavity, [10.0; 3]),
            (
                3,
                include_str!("../../models/extruded-bracket.luau"),
                [64.0, 42.0, 6.0],
            ),
            (
                4,
                include_str!("../../models/edge-fillets.luau"),
                [60.0, 30.0, 8.0],
            ),
            (
                5,
                include_str!("../../models/edge-chamfers.luau"),
                [60.0, 30.0, 8.0],
            ),
            (
                6,
                "local p=cad.polygon({{x=0,y=0},{x=10,y=0},{x=10,y=20},{x=0,y=20}});cad.show('revolve',cad.translate(cad.revolve(p,{}),{x=10,y=10}))",
                [20.0, 20.0, 20.0],
            ),
            (7, bushing.as_str(), [32.0, 32.0, 20.0]),
            (
                8,
                "local p=cad.polygon({{x=5,y=0},{x=10,y=0},{x=10,y=20},{x=5,y=20}});cad.show('sector',cad.revolve(p,{angle=90}))",
                [10.0, 10.0, 20.0],
            ),
        ] {
            let folder =
                std::env::temp_dir().join(format!("spice-cad-curved-{}", std::process::id()));
            let folder = folder.with_extension(index.to_string());
            export::model_files(source, &folder, crate::export::Target::Print)?;
            let expected = build_script_snapshot(source, 1)?;
            let shape = opencascade::primitives::Shape::read_step(folder.join("model.step"))
                .map_err(|error| error.to_string())?;
            let mesh = shape.mesh().map_err(|error| error.to_string())?;
            let volume: f64 = mesh
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|triangle| {
                    mesh.vertices[triangle[0]]
                        .dot(mesh.vertices[triangle[1]].cross(mesh.vertices[triangle[2]]))
                        / 6.0
                })
                .sum();
            assert!(
                (volume - expected.volume_mm3).abs() < expected.volume_mm3 * 0.002,
                "STEP imported volume {volume} differs from {}",
                expected.volume_mm3
            );
            let bounds = opencascade::bounding_box::aabb(&shape);
            assert!(
                bounds.min().abs().max_element() < 0.2,
                "STEP bounds minimum {:?}",
                bounds.min()
            );
            assert!(
                (bounds.max() - glam::DVec3::from_array(dimensions))
                    .abs()
                    .max_element()
                    < 0.2
            );
            let mut imported = Vec::with_capacity(mesh.indices.len());
            for triangle in mesh.indices.as_chunks::<3>().0 {
                let points = triangle.map(|index| mesh.vertices[index]);
                // OpenCascade can emit pole triangles that collapse under the audit's 0.001 mm weld.
                let welded = points.map(|point| {
                    point
                        .to_array()
                        .map(|value| (value * 1000.0).round() as i64)
                });
                if welded[0] == welded[1] {
                    continue;
                }
                if welded[1] == welded[2] {
                    continue;
                }
                if welded[2] == welded[0] {
                    continue;
                }
                let normal = (points[1] - points[0])
                    .cross(points[2] - points[0])
                    .normalize_or_zero()
                    .as_vec3()
                    .to_array();
                imported.extend(points.map(|point| crate::mesh::Vertex {
                    position: point.as_vec3().to_array(),
                    normal,
                    face_id: 1,
                    color: crate::mesh::vertex_color(crate::mesh::DEFAULT_COLOR),
                }));
            }
            crate::mesh::Mesh {
                vertices: imported,
                face_count: 1,
            }
            .validate()
            .map_err(|error| format!("STEP imported mesh {index}: {error}"))?;
            let bytes =
                std::fs::read(folder.join("model.stl")).map_err(|error| error.to_string())?;
            let count = u32::from_le_bytes(bytes[80..84].try_into().expect("STL count"));
            assert_eq!(bytes.len(), 84 + count as usize * 50);
            assert_eq!(
                count as usize * 3,
                expected.scene.world_mesh().vertices.len()
            );
            let mut decoded = Vec::with_capacity(count as usize * 3);
            for triangle in bytes[84..].as_chunks::<50>().0 {
                let values: [f32; 12] = std::array::from_fn(|index| {
                    f32::from_le_bytes(
                        triangle[index * 4..index * 4 + 4]
                            .try_into()
                            .expect("STL float"),
                    )
                });
                for position in values[3..].as_chunks::<3>().0 {
                    decoded.push(crate::mesh::Vertex {
                        position: *position,
                        normal: values[..3].try_into().expect("STL normal"),
                        face_id: 1,
                        color: crate::mesh::vertex_color(crate::mesh::DEFAULT_COLOR),
                    });
                }
            }
            let decoded = crate::mesh::Mesh {
                vertices: decoded,
                face_count: 1,
            };
            decoded.validate()?;
            assert!((decoded.volume_mm3() - expected.scene.world_mesh().volume_mm3()).abs() < 0.01);
            assert!(export::model_files(source, &folder, crate::export::Target::Print).is_err());
            std::fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    #[test]
    fn bracket_holes_remove_four_cylinders_and_transforms_preserve_volume() -> Result<(), String> {
        let source = include_str!("../../models/mounting-bracket.luau");
        let blank = format!(
            "{}\ncad.show('blank', bracket)",
            source
                .split("for _, x")
                .next()
                .ok_or("Missing bracket feature")?
        );
        let blank = build_script_snapshot(&blank, 1)?;
        let bracket = build_script_snapshot(source, 2)?;
        assert!(
            (blank.volume_mm3 - bracket.volume_mm3 - 4.0 * std::f64::consts::PI * 9.0 * 6.0).abs()
                < 0.01
        );
        let cylinder = build_script_snapshot(
            "local a=cad.cylinder({radius=3,height=8}); a=cad.rotate(a,{axis='x',angle=-90}); cad.show('bore',cad.translate(a,{x=15,y=-1,z=25}))",
            3,
        )?;
        assert!((cylinder.volume_mm3 - std::f64::consts::PI * 9.0 * 8.0).abs() < 0.001);
        let bounds = snapshot_camera(&cylinder, 1.0);
        assert!((bounds.target - glam::vec3(15.0, 3.0, 25.0)).length() < 0.01);
        for snapshot in [&bracket, &cylinder] {
            for aspect in [0.25, 0.5, 1.0, 2.0] {
                let matrix = snapshot_camera(snapshot, aspect).matrix(aspect);
                for vertex in &snapshot.scene.world_mesh().vertices {
                    let point = matrix * glam::Vec3::from_array(vertex.position).extend(1.0);
                    assert!(point.w > 0.0);
                    assert!(point.x.abs() <= point.w);
                    assert!(point.y.abs() <= point.w);
                }
            }
        }
        Ok(())
    }

    #[test]
    fn model_export_rejects_invalid_bed_placement_without_writing() {
        let folder =
            std::env::temp_dir().join(format!("spice-cad-export-rejected-{}", std::process::id()));
        for (source, message) in [
            (
                "cad.show('large',cad.box({width=300,depth=10,height=10}))",
                "P1S",
            ),
            (
                "cad.show('floating',cad.box({width=10,depth=10,height=10,z=100}))",
                "rest on Z=0",
            ),
            (
                "cad.show('below',cad.box({width=10,depth=10,height=10,z=-1}))",
                "rest on Z=0",
            ),
        ] {
            let error = export::model_files(source, &folder, crate::export::Target::Print)
                .expect_err("Unsafe export must fail");
            assert!(error.contains(message), "{error}");
            assert!(!folder.exists());
        }
    }

    #[test]
    fn rack_parts_match_the_reference_and_export_on_the_p1s_bed() -> Result<(), String> {
        let source = include_str!("../../models/rack-parts.luau");
        let reference = spice_rack::printing::build_parts(PARAMETERS)?;
        for layout in [placement::Layout::Assembly, placement::Layout::Print] {
            let kernel = kernel::Kernel::new()?;
            let source = source.replace("cad.number(12,", "cad.number(0,");
            let model = model::compile(&source)?;
            let mut bodies = model.build(&kernel)?;
            for (part, body) in model.parts.iter().zip(&mut bodies) {
                part.placement(layout).apply(body, false)?;
            }
            assert_eq!(model.parts.len(), 4);
            assert_eq!(bodies.len(), reference.len());
            let meshes = model.meshes(&mut bodies, false)?;
            for (index, (body, reference)) in bodies.iter_mut().zip(&reference).enumerate() {
                meshes[index].validate()?;
                let mesh = reference.shape.mesh().map_err(|error| error.to_string())?;
                let volume: f64 = mesh
                    .indices
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|triangle| {
                        let [a, b, c] = triangle.map(|index| mesh.vertices[index]);
                        a.dot(b.cross(c)) / 6.0
                    })
                    .sum();
                assert!((body.volume()? - volume).abs() < 0.01, "Part {index}");
                let [minimum, maximum] = body.bounds(true)?;
                if layout == placement::Layout::Print {
                    assert!(minimum.z.abs() < 0.001);
                    let (_, dimensions) = reference.on_bed()?;
                    assert!(((maximum - minimum) - dimensions).abs().max_element() < 0.001);
                    assert!((maximum - minimum).max_element() < 246.001);
                } else {
                    let bounds = opencascade::bounding_box::aabb(&reference.shape);
                    assert!((minimum - bounds.min()).abs().max_element() < 0.001);
                    assert!((maximum - bounds.max()).abs().max_element() < 0.001);
                }
            }
            drop(bodies);
            drop(kernel);
            if layout == placement::Layout::Print {
                let folder = std::env::temp_dir()
                    .join(format!("spice-cad-rack-parts-{}", std::process::id()));
                export::model_files(&source, &folder, export::Target::Print)?;
                let manifest: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(folder.join("parts.json")).map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                let parts = manifest.as_array().ok_or("Expected parts manifest")?;
                assert_eq!(parts.len(), 4);
                for (part, expected) in parts.iter().zip(&model.parts) {
                    assert_eq!(part["name"], expected.name);
                    for format in ["step", "stl"] {
                        let name = part[format].as_str().ok_or("Missing export filename")?;
                        assert!(
                            std::fs::metadata(folder.join(name))
                                .map_err(|error| error.to_string())?
                                .len()
                                > 84
                        );
                    }
                }
                std::fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    #[test]
    fn luau_models_preserve_native_geometry_and_allow_reused_operands() -> Result<(), String> {
        let bracket = build_script_snapshot(include_str!("../../models/bracket.luau"), 1)?;
        assert_eq!(bracket.name, "bracket");
        assert!((bracket.volume_mm3 - 64080.0).abs() < 0.01);
        let rack = build_script_snapshot(include_str!("../../models/rack.luau"), 2)?;
        assert!((rack.volume_mm3 - 972678.0).abs() < 0.01);
        let model = model::compile(
            "local a = cad.box({width=10, depth=10, height=10})\nlocal b = cad.box({width=5, depth=5, height=12, z=-1})\ncad.show('reused', cad.union(cad.subtract(a, b), b))",
        )?;
        let kernel = kernel::Kernel::new()?;
        let body = model.build(&kernel)?.remove(0);
        assert!((body.volume()? - 1050.0).abs() < 0.01);
        Ok(())
    }

    #[test]
    fn large_model_sources_run_with_the_existing_geometry_and_vm_budgets() -> Result<(), String> {
        let source = format!(
            "{}cad.show('large', cad.box({{width=10,depth=10,height=10}}))",
            "-- large source comment\n".repeat(100_000)
        );
        let snapshot = build_script_snapshot(&source, 1)?;
        assert_eq!(snapshot.name, "large");
        assert!((snapshot.volume_mm3 - 1000.0).abs() < 0.001);
        assert!(source.len() > 100_000);
        Ok(())
    }

    #[test]
    fn luau_rejects_type_errors_invalid_dimensions_and_unbounded_scripts() {
        let error = model::compile("--!strict\ncad.show('wrong', 42)")
            .err()
            .expect("Wrong operand must fail type checking");
        assert!(error.contains("model.luau:2.19-2.20"));
        assert!(!error.contains("CommentDirective"));
        for (source, expected) in [
            (
                "cad.show('bad',cad.cylinder({radius=0,height=1}))",
                "radius must",
            ),
            (
                "cad.show('bad',cad.cylinder({radius=1,height=0/0}))",
                "height must",
            ),
            (
                "local a=cad.box({width=2,depth=2,height=2}); cad.show('bad',cad.rotate(a,{axis='w',angle=1}))",
                "type check failed",
            ),
            (
                "local a=cad.box({width=2,depth=2,height=2}); cad.show('bad',cad.fillet(a,{radius=-1}))",
                "radius must",
            ),
            (
                "local a=cad.box({width=2,depth=2,height=2}); cad.show('bad',cad.translate(a,{x=0/0}))",
                "Coordinates must",
            ),
            ("cad.show('wrong', 42)", "type check failed"),
            (
                "cad.show('wrong', cad.box({width='bad', depth=2, height=3}))",
                "type check failed",
            ),
            (
                "cad.show('invalid', cad.box({width=-1, depth=2, height=3}))",
                "Box dimensions",
            ),
            (
                "cad.show('invalid', cad.box({width=0/0, depth=2, height=3}))",
                "Box dimensions",
            ),
            (
                "local a = cad.box({width=1, depth=1, height=1})",
                "must call cad.show",
            ),
            ("while true do end", "script budget"),
            ("local _large = string.rep('x', 16 * 1024 * 1024)", "memory"),
            (
                "for _ = 1, 257 do cad.box({width=1, depth=1, height=1}) end",
                "256 operations",
            ),
            ("--!nocheck\ncad.show('wrong', 42)", "strict type checking"),
        ] {
            let error = model::compile(source)
                .err()
                .expect("Invalid model must fail");
            assert!(
                error.contains(expected),
                "Expected {expected}, got: {error}"
            );
        }
    }

    #[test]
    fn worker_completion_wakes_ui_immediately_and_preserves_input_timestamps() -> Result<(), String>
    {
        let context = egui::Context::default();
        for _ in 0..3 {
            let mut output = context.run_ui(Default::default(), |_| {});
            output.textures_delta.clear();
        }
        let (wake, notified) = mpsc::sync_channel(1);
        context.set_request_repaint_callback(move |info| {
            let _ = wake.try_send(info.delay);
        });
        let worker = Worker::new(context)?;
        let input = performance::Input::new(1, Instant::now());
        worker
            .try_send(Command::Script {
                input: Some(input),
                source: "cad.show('box',cad.box({width=1,depth=1,height=1}))".into(),
                editor_revision: 1,
                revision: 1,
                saved_at: None,
            })
            .map_err(|error| error.to_string())?;
        let Outcome::Model(snapshot) = worker
            .recv_timeout(Duration::from_secs(20))
            .map_err(|error| error.to_string())??
        else {
            return Err("Expected model".into());
        };
        assert_eq!(
            notified.recv_timeout(Duration::from_secs(1)),
            Ok(Duration::ZERO)
        );
        let mut traced = snapshot.input.expect("Traced model");
        assert_eq!(traced.at, input.at);
        assert_eq!(traced.revision, 1);
        assert!(traced.worker_started_ms >= input.dispatched_ms);
        assert!(traced.worker_finished_ms >= traced.worker_started_ms);
        traced.received_ms = traced.elapsed_ms(Instant::now());
        let latency = traced.prepared(Instant::now());
        assert!(latency.milliseconds[0] > 0.0);
        Ok(())
    }

    #[test]
    fn worker_bounds_error_messages_without_losing_runtime_locations() -> Result<(), String> {
        let worker = Worker::new(egui::Context::default())?;
        worker
            .try_send(Command::Script {
                input: None,
                source: "--!strict\nerror(string.rep('λ', 100000))".into(),
                editor_revision: 1,
                revision: 1,
                saved_at: None,
            })
            .map_err(|error| error.to_string())?;
        let response = worker
            .recv_timeout(Duration::from_secs(10))
            .map_err(|error| error.to_string())?;
        let message = response.err().expect("Runtime error");
        assert!(message.len() <= 8192);
        assert!(message.contains("Build message truncated"));
        assert_eq!(
            analysis::Diagnostic::build_error(&message)
                .span
                .expect("Preserved call site")
                .start
                .line,
            2
        );
        Ok(())
    }

    #[test]
    fn worker_shutdown_closes_full_channels_and_joins_before_returning() {
        let (commands, requests) = mpsc::sync_channel(1);
        let (responses, results) = mpsc::sync_channel(1);
        let (ready, started) = mpsc::sync_channel(1);
        let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped = Arc::clone(&completed);
        let thread = std::thread::spawn(move || {
            responses.send(Err("first".into())).expect("Initial result");
            ready.send(()).expect("Full result channel");
            let deadline = Instant::now() + Duration::from_secs(1);
            let mut result = Err("second".into());
            loop {
                match responses.try_send(result) {
                    Err(mpsc::TrySendError::Disconnected(_)) => break,
                    Err(mpsc::TrySendError::Full(pending)) => result = pending,
                    Ok(()) => panic!("Shutdown must close the result channel"),
                }
                assert!(Instant::now() < deadline, "Result channel remained open");
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(matches!(
                requests.recv_timeout(Duration::from_secs(1)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            ));
            stopped.store(true, std::sync::atomic::Ordering::Release);
        });
        let worker = Worker {
            commands: Some(commands),
            results: Some(results),
            thread: Some(thread),
        };
        started
            .recv_timeout(Duration::from_secs(1))
            .expect("Result published");
        drop(worker);
        assert!(completed.load(std::sync::atomic::Ordering::Acquire));
    }

    #[test]
    fn worker_reuses_parts_after_edits_and_exports_without_mutating_cache() -> Result<(), String> {
        let worker = Worker::new(egui::Context::default())?;
        let source = "local a=cad.box({width=10,depth=10,height=2});local b=cad.face(a,{name='bottom',normal='z-'});local t=cad.face(a,{name='top',normal='z+'});cad.measure('height',{from=b,to=t,min=1,max=3});cad.show('part',a,{print={}})";
        let folder =
            std::env::temp_dir().join(format!("spice-cad-cached-export-{}", std::process::id()));
        for revision in 1..=4 {
            let source = if revision == 2 {
                source.replace("max=3", "max=1")
            } else {
                format!("-- revision {revision}\n{source}")
            };
            worker
                .try_send(Command::Script {
                    input: None,
                    source: source.into(),
                    editor_revision: revision,
                    revision,
                    saved_at: None,
                })
                .map_err(|error| error.to_string())?;
            let result = worker
                .recv_timeout(Duration::from_secs(20))
                .map_err(|error| error.to_string())?;
            if revision == 2 {
                assert!(
                    result
                        .err()
                        .ok_or("Constraint must fail on cache hit")?
                        .contains("Constraint 'height'")
                );
                continue;
            }
            let Outcome::Model(snapshot) = result? else {
                return Err("Expected model".into());
            };
            assert_eq!(snapshot.cached_parts, usize::from(revision != 1));
            assert!((snapshot.volume_mm3 - 200.0).abs() < 0.001);
            if revision == 3 {
                for (target, directory, filename) in [
                    (export::Target::Print, "print", "model.step"),
                    (export::Target::Game, "game", "model.glb"),
                ] {
                    worker
                        .try_send(Command::ExportScript {
                            source: match &snapshot.source {
                                ModelSource::Script(source) => source.clone(),
                                _ => unreachable!(),
                            },
                            folder: folder.join(directory),
                            target,
                        })
                        .map_err(|error| error.to_string())?;
                    assert!(matches!(
                        worker
                            .recv_timeout(Duration::from_secs(20))
                            .map_err(|error| error.to_string())??,
                        Outcome::Exported(_)
                    ));
                    assert!(folder.join(directory).join(filename).is_file());
                }
            }
        }
        std::fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn script_worker_recovers_after_failed_model() -> Result<(), String> {
        let worker = Worker::new(egui::Context::default())?;
        for (source, revision) in [
            ("cad.show('bad', 2)", 1),
            (
                "local a = cad.box({width=10, depth=10, height=10}); cad.show('empty', cad.subtract(a, a))",
                2,
            ),
            (
                "local a=cad.box({width=10,depth=10,height=10}); cad.show('bad',cad.fillet(a,{radius=100}))",
                3,
            ),
            (include_str!("../../models/bracket.luau"), 4),
        ] {
            worker
                .try_send(Command::Script {
                    input: None,
                    source: source.into(),
                    editor_revision: revision,
                    revision,
                    saved_at: None,
                })
                .map_err(|error| error.to_string())?;
            let result = worker
                .recv_timeout(Duration::from_secs(20))
                .map_err(|error| error.to_string())?;
            if revision < 4 {
                assert!(result.is_err());
            } else {
                match result? {
                    Outcome::Model(snapshot) => {
                        assert_eq!(snapshot.revision, 4);
                        assert_eq!(snapshot.name, "bracket");
                    }
                    Outcome::Exported(_) => return Err("Expected script model".into()),
                }
            }
        }
        Ok(())
    }

    #[test]
    fn native_mesh_preserves_cavities_volume_and_face_selection() -> Result<(), String> {
        let kernel = kernel::Kernel::new()?;
        for parameters in [
            PARAMETERS,
            RackParameters {
                rack_width_mm: 300.5,
                step_count: 3,
                step_rise_mm: 70.0,
                thickness_mm: 4.0,
                ..PARAMETERS
            },
        ] {
            let snapshot = build_snapshot(&kernel, parameters, 1)?;
            let reference = build_rack(parameters)?
                .mesh()
                .map_err(|error| error.to_string())?;
            let volume: f64 = reference
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|triangle| {
                    reference.vertices[triangle[0]]
                        .dot(reference.vertices[triangle[1]].cross(reference.vertices[triangle[2]]))
                        / 6.0
                })
                .sum();
            assert!((snapshot.volume_mm3 - volume).abs() < volume * 1e-6);
            assert!(snapshot.scene.world_mesh().face_count > 6);
            let layout = parameters.layout()?;
            let camera = viewport::Camera::rack(layout.dimensions_mm.to_array());
            let matrix = camera.matrix(1.5);
            assert!(
                snapshot
                    .scene
                    .world_mesh()
                    .pick(matrix, [0.0, 0.0])
                    .is_some()
            );
            assert_eq!(
                snapshot.scene.world_mesh().pick(matrix, [-0.99, 0.99]),
                None
            );
        }
        Ok(())
    }

    #[test]
    fn worker_recovers_after_invalid_request() -> Result<(), String> {
        let worker = Worker::new(egui::Context::default())?;
        worker
            .try_send(Command::Build {
                parameters: RackParameters {
                    step_count: 0,
                    ..PARAMETERS
                },
                revision: 1,
            })
            .map_err(|error| error.to_string())?;
        assert!(
            worker
                .recv_timeout(Duration::from_secs(20))
                .map_err(|error| error.to_string())?
                .is_err()
        );
        worker
            .try_send(Command::Build {
                parameters: PARAMETERS,
                revision: 2,
            })
            .map_err(|error| error.to_string())?;
        let result = worker
            .recv_timeout(Duration::from_secs(20))
            .map_err(|error| error.to_string())??;
        match result {
            Outcome::Model(snapshot) => {
                assert_eq!(snapshot.revision, 2);
                assert!(snapshot.volume_mm3 > 0.0);
            }
            Outcome::Exported(_) => return Err("Expected rebuilt geometry".into()),
        }
        Ok(())
    }

    #[test]
    fn print_export_checks_supports_before_writing_and_emits_all_parts() -> Result<(), String> {
        let folder =
            std::env::temp_dir().join(format!("spice-cad-print-test-{}", std::process::id()));
        let parameters = RackParameters {
            rack_width_mm: 300.0,
            step_count: 3,
            step_rise_mm: 35.0,
            thickness_mm: 4.0,
            ..PARAMETERS
        };
        let error =
            export_parts(parameters, &folder).expect_err("Short configuration needs supports");
        assert!(error.contains("needs support"));
        assert!(!folder.exists());
        export_parts(PARAMETERS, &folder)?;
        let files = std::fs::read_dir(&folder)
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        assert_eq!(files.len(), 17);
        assert!(
            files
                .iter()
                .all(|entry| entry.metadata().is_ok_and(|data| data.len() > 84))
        );
        std::fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        Ok(())
    }
}
