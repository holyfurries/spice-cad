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
mod cancellation;
mod code_editor;
mod diagnostics;
mod dimensions;
mod document;
mod edge_reference;
mod export;
mod face_feature;
mod files;
mod game_asset;
mod hole;
mod inline_output;
mod inspection;
mod interface;
mod kernel;
mod lsp;
mod mesh;
mod modal;
mod model;
mod outliner;
mod palette;
mod parameters;
mod performance;
mod picking;
mod placement;
mod preview;
mod profile;
mod raster;
mod render_views;
mod save;
mod search;
mod shape_operation;
mod splash_frames;
mod studio;
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
    let sweep_kind = match arguments.as_slice() {
        [flag] if flag == "--profile-preview" => Some(performance::SweepKind::Separation),
        [flag] if flag == "--profile-rotation" => Some(performance::SweepKind::Rotation),
        [flag] if flag == "--profile-geometry" => Some(performance::SweepKind::Geometry),
        _ => None,
    };
    let benchmarking = sweep_kind.is_some();
    if benchmarking {
        profile = Some(started);
    }
    let completed = benchmarking.then(|| Arc::new(std::sync::atomic::AtomicBool::new(false)));
    let sweep = completed
        .as_ref()
        .zip(sweep_kind)
        .map(|(completed, kind)| performance::Sweep::new(started, Arc::clone(completed), kind));
    #[cfg(feature = "rack-tools")]
    if arguments.as_slice() == ["--benchmark"] {
        return benchmark().map_err(Into::into);
    }
    let model_path = match arguments.as_slice() {
        [_] if benchmarking => None,
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
        [flag, path, folder] if flag == "--render-views" => {
            let source = model::read_source(std::path::Path::new(path))?;
            render_views::write(&source, std::path::Path::new(folder))?;
            println!("Rendered top, front, side, isometric, and contact-sheet PNGs to {folder}");
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
                    "Usage: spice-cad [--profile-preview | --profile-rotation | --profile-geometry | [--profile] [--rack | --benchmark | --model FILE.luau | --check-model FILE.luau | --export-model FILE.luau NEW_DIRECTORY | --export-game FILE.luau NEW_DIRECTORY | --render-views FILE.luau NEW_DIRECTORY]]"
                } else {
                    "Usage: spice-cad [--profile-preview | --profile-rotation | --profile-geometry | [--profile] [--model FILE.luau | --check-model FILE.luau | --export-model FILE.luau NEW_DIRECTORY | --export-game FILE.luau NEW_DIRECTORY | --render-views FILE.luau NEW_DIRECTORY]]"
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
        thumbnails: model::ThumbnailMode,
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
    Cancelled,
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
    fn visible_bounds(&self, mask: u32) -> Option<[glam::Vec3; 2]> {
        assert!(self.part_bounds.len() <= 32);
        self.part_bounds
            .iter()
            .enumerate()
            .filter(|(index, _)| mask & (1 << index) != 0)
            .map(|(_, bounds)| *bounds)
            .reduce(|first, second| [first[0].min(second[0]), first[1].max(second[1])])
    }

    fn picking(&self) -> picking::View<'_> {
        picking::View {
            parts: &self.display.parts,
            instances: &self.instances,
            visible_parts: u32::MAX,
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
    refinement_pending: bool,
    thumbnails_pending: bool,
    thumbnail_renders: u64,
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
    fn complete(&self) -> bool {
        !self.refinement_pending && !self.thumbnails_pending
    }

    fn visible_parts(&self, outliner: &outliner::State) -> u32 {
        match &self.source {
            ModelSource::Rack(_) => u32::MAX,
            ModelSource::Script(_) => outliner.visible_parts(&self.parts),
        }
    }

    fn part_source(&self, index: usize, revision: u64) -> Option<usize> {
        if self.editor_revision != Some(revision) {
            return None;
        }
        Some(self.parts.get(index)?.line)
    }

    fn restore_selection(
        &self,
        previous: &Self,
        selected: &inspection::Selection,
        name: Option<&str>,
    ) -> Option<inspection::Selection> {
        if let Some(name) = name {
            if !matches!(selected, inspection::Selection::Pair { .. }) {
                return None;
            }
            let previous = previous
                .scene
                .annotations
                .measurements
                .iter()
                .find(|measurement| measurement.name == name)?;
            if !previous.matches_selection(selected) {
                return None;
            }
            return self
                .scene
                .annotations
                .measurements
                .iter()
                .find(|measurement| measurement.name == name)?
                .selection(&self.scene.faces);
        }
        if let inspection::Selection::Part { index, .. } = selected {
            let name = &previous.parts.get(*index as usize)?.name;
            let index = self.parts.iter().position(|part| part.name == *name)?;
            return self.scene.selection(&inspection::Selection::Part {
                index: index as u32,
                faces: [0; 2],
                bounds: [glam::Vec3::ZERO; 2],
            });
        }
        if Arc::ptr_eq(&previous.scene.display, &self.scene.display) {
            return self.scene.selection(selected);
        }
        None
    }

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
    commands: Option<mpsc::SyncSender<(Command, cancellation::Token)>>,
    generation: cancellation::Generation,
    results: Option<mpsc::Receiver<Result<Outcome, String>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    fn try_send(&self, command: Command) -> Result<(), mpsc::TrySendError<Command>> {
        self.commands
            .as_ref()
            .expect("Live geometry worker")
            .try_send((command, self.generation.token()))
            .map_err(|error| match error {
                mpsc::TrySendError::Full((command, _)) => mpsc::TrySendError::Full(command),
                mpsc::TrySendError::Disconnected((command, _)) => {
                    mpsc::TrySendError::Disconnected(command)
                }
            })
    }

    fn invalidate_preview(&self) {
        self.generation.advance();
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
                while let Ok((command, token)) = requests.recv() {
                    let result = cache.as_mut().map_err(|error| (*error).clone()).and_then(|cache| match command {
                        Command::ExportScript { source, folder, target } => export::model_files_cached(&source, &folder, target, cache).map(|()| Outcome::Exported(format!("Exported {} to {}", target.description(), folder.display()))),
                        Command::Script { source, editor_revision, revision, saved_at, mut input, thumbnails } => {
                            if let Some(input) = &mut input { input.worker_started_ms = input.elapsed_ms(Instant::now()); }
                            let source = source.to_string();
                            cache.with_preview(token, |cache| build_script_snapshot_with_thumbnails(source, revision, cache, thumbnails)).map(|snapshot| {
                                let Some(mut snapshot) = snapshot else { return Outcome::Cancelled; };
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
            generation: cancellation::Generation::default(),
            results: Some(results),
            thread: Some(thread),
        })
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.invalidate_preview();
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
    let kernel::Tessellation {
        mut mesh,
        volume_mm3,
    } = body.mesh()?;
    mesh.smooth_normals();
    let edges = body.edges()?;
    let faces = inspection::faces(&mesh)?;
    let tessellation_ms = start.elapsed().as_secs_f64() * 1000.0;
    let bounds = body.bounds(true)?.map(|point| point.as_vec3());
    let geometry = Arc::new(model::Geometry {
        mesh,
        edges,
        bounds,
        volume_mm3,
    });
    let display = Arc::new(viewport::Geometry::new(&[geometry])?);
    Ok(Snapshot {
        refinement_pending: false,
        thumbnails_pending: false,
        thumbnail_renders: 0,
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
            part_bounds: vec![bounds],
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
    build_script_snapshot_with_thumbnails(source, revision, cache, model::ThumbnailMode::Complete)
}

fn build_script_snapshot_with_thumbnails(
    source: String,
    revision: u64,
    cache: &mut model::Cache<'_>,
    thumbnails: model::ThumbnailMode,
) -> Result<Snapshot, String> {
    assert!(revision > 0);
    let total = Instant::now();
    let model = cache.compile(&source)?;
    cache.checkpoint()?;
    let quality = if thumbnails == model::ThumbnailMode::Deferred {
        model::DisplayQuality::Interactive
    } else {
        model::DisplayQuality::Final
    };
    let prepared = cache.prepare_quality(&model, quality)?;
    let geometry = prepared.geometry;
    let display = cache.display(&geometry)?;
    let build_ms = prepared.build_ms;
    let start = Instant::now();
    let checks = cache.annotations(&model)?;
    let volume_mm3 = geometry.iter().map(|geometry| geometry.volume_mm3).sum();
    let presentations = cache.presentations(&model, placement::Layout::Assembly, thumbnails)?;
    let mut thumbnails_pending = presentations.iter().any(|part| part.thumbnail.pending());
    let scene = build_scene(
        &model,
        &geometry,
        &presentations,
        Arc::clone(&display),
        placement::Layout::Assembly,
        &checks,
    )?;
    let alternate = if model.parts.iter().any(|part| part.print.is_some()) {
        let presentations = cache.presentations(&model, placement::Layout::Print, thumbnails)?;
        thumbnails_pending |= presentations.iter().any(|part| part.thumbnail.pending());
        Some(build_scene(
            &model,
            &geometry,
            &presentations,
            display,
            placement::Layout::Print,
            &checks,
        )?)
    } else {
        None
    };
    let tessellation_ms = prepared.tessellation_ms + start.elapsed().as_secs_f64() * 1000.0;
    cache.checkpoint()?;
    Ok(Snapshot {
        refinement_pending: prepared.refinement_pending,
        thumbnails_pending,
        thumbnail_renders: cache.thumbnail_renders,
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
                image: presentation.thumbnail.image(),
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
    outliner: outliner::State,
    sweep: Option<performance::Sweep>,
    preview_input: Option<performance::Input>,
    refinement: preview::Refinement,
    save_dialog: Option<save::Dialog>,
    preview: preview::Schedule,
    palette: palette::State,
    parameter_state: parameters::State,
    parameter_editor: Option<parameters::DimensionEditor>,
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
    show_about: bool,
    splash: Option<studio::Splash>,
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
    selected_measurement: Option<String>,
    message: String,
    ui_ms: f64,
}
enum ExitState {
    Open,
    ConfirmDiscard,
    Closing,
}

impl Editor {
    fn frame_selection(&mut self) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let mask = snapshot.visible_parts(&self.outliner);
        let picking = picking::View {
            visible_parts: mask,
            ..snapshot.scene.picking()
        };
        let bounds = self
            .selected
            .as_ref()
            .and_then(|selection| picking.selection_part(selection))
            .filter(|index| mask & (1 << index) != 0)
            .map(|index| snapshot.scene.part_bounds[index])
            .or_else(|| snapshot.scene.visible_bounds(mask));
        if let Some(bounds) = bounds {
            self.camera.frame(bounds, self.viewport_aspect);
            self.message = if self.selected.is_some() {
                "Framed selected part"
            } else {
                "Framed visible parts"
            }
            .into();
        } else {
            self.message = "No visible parts to frame · use Show all in Parts".into();
        }
    }

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
        let script = if let Some(sweep) = &sweep {
            Some(document::Document::loaded(
                std::env::temp_dir().join(format!("spice-cad-profile-{}.luau", std::process::id())),
                sweep.source(),
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
            outliner: outliner::State::default(),
            sweep,
            preview_input: None,
            refinement: preview::Refinement::default(),
            save_dialog: None,
            preview: preview::Schedule::default(),
            palette,
            parameter_state: parameters::State::default(),
            parameter_editor: None,
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
            show_about: false,
            splash: Some(studio::Splash::default()),
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
            selected_measurement: None,
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
        self.parameter_editor = None;
        if let Some(location) = self.pending_location.take() {
            self.code_state
                .jump_to(location.range(&document.buffer.rope));
        }
        self.script = Some(document);
        self.worker.invalidate_preview();
        self.snapshot = None;
        self.outliner = outliner::State::default();
        self.preview_input = None;
        self.refinement = preview::Refinement::default();
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
                if changed {
                    self.worker.invalidate_preview();
                }
                if self
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.editor_revision == Some(revision))
                {
                    let reused = if self.snapshot.as_ref().is_some_and(Snapshot::complete) {
                        self.preview.reuse(revision)
                    } else {
                        false
                    };
                    if changed || reused {
                        self.message = "Preview ready · geometry reused".into();
                    }
                }
            }
        }
    }

    fn can_run(&self) -> bool {
        if !matches!(self.work, WorkerState::Idle) {
            return false;
        }
        let Some(script) = &self.script else {
            return false;
        };
        if !script.source_ready() {
            return false;
        }
        self.parameter_state.has_pending(script.buffer.revision)
            || !self.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.editor_revision == Some(script.buffer.revision) && snapshot.complete()
            })
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
            return Err("Run the current source successfully before exporting".into());
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

    fn open_measurement(&mut self, name: &str) -> Result<(), String> {
        let snapshot = self.snapshot.as_ref().ok_or("No model preview")?;
        let script = self.script.as_ref().ok_or("No source document")?;
        if snapshot.editor_revision != Some(script.buffer.revision) {
            return Err("Run the current source before selecting a measurement".into());
        }
        let measurement = snapshot
            .scene
            .annotations
            .measurements
            .iter()
            .find(|measurement| measurement.name == name)
            .ok_or("Measurement is not in the current layout")?;
        if !measurement.visible(&picking::View {
            visible_parts: snapshot.visible_parts(&self.outliner),
            ..snapshot.scene.picking()
        }) {
            return Err("Show both measured parts before selecting this measurement".into());
        }
        let selection = measurement
            .selection(&snapshot.scene.faces)
            .ok_or("Measurement faces are unavailable")?;
        self.parameter_state.reveal(
            measurement
                .parameter
                .as_ref()
                .map(|parameter| parameter.name.as_str()),
        );
        let range = if let Some(parameter) = &measurement.parameter {
            self.message = format!(
                "{} ≈{:.3} mm · linked parameter {}",
                measurement.name, measurement.distance_mm, parameter.name
            );
            parameter.range.clone()
        } else {
            self.message = format!(
                "{} ≈{:.3} mm · {}",
                measurement.name,
                measurement.distance_mm,
                if measurement.checked {
                    "distance check passed"
                } else {
                    "reference measurement"
                }
            );
            let line = measurement.line.min(script.buffer.rope.len_lines() - 1);
            let start = script.buffer.rope.line_to_char(line);
            start..start + script.buffer.rope.line(line).len_chars()
        };
        assert!(range.start <= range.end);
        assert!(range.end <= script.buffer.rope.len_chars());
        self.parameter_editor = measurement.parameter.as_ref().map(|parameter| {
            parameters::DimensionEditor::new(
                script.buffer.revision,
                parameter.clone(),
                &measurement.name,
                measurement.distance_mm,
            )
        });
        self.language_popup = None;
        if self.parameter_editor.is_some() {
            self.code_state.reveal_source(range);
        } else {
            self.code_state.jump_to(range);
        }
        self.selected = Some(selection);
        self.selected_measurement = Some(name.into());
        self.show_code = true;
        self.show_inspector = true;
        self.focus_editor = false;
        Ok(())
    }

    fn change_layout(&mut self, layout: placement::Layout) -> bool {
        let Some(snapshot) = &mut self.snapshot else {
            return false;
        };
        if !snapshot.set_layout(layout) {
            return false;
        }
        if let Some(revision) = snapshot.editor_revision {
            self.code_state
                .outputs
                .set(revision, Arc::clone(&snapshot.scene.outputs));
        }
        self.selected = None;
        self.selected_measurement = None;
        self.focus_editor = false;
        self.camera = snapshot_camera(snapshot, self.viewport_aspect);
        self.message = if layout == placement::Layout::Print {
            "Print layout · export checks each part separately"
        } else {
            "Assembly layout"
        }
        .into();
        true
    }

    fn start_preview(&mut self, pointer_down: bool) {
        if !matches!(self.work, WorkerState::Idle) {
            return;
        }
        let Some(revision) = self.preview.ready(Instant::now()) else {
            return;
        };
        let thumbnails = if self.refinement.interactive(pointer_down, Instant::now()) {
            model::ThumbnailMode::Deferred
        } else {
            model::ThumbnailMode::Complete
        };
        self.start_script(revision, thumbnails);
    }

    fn start_script(&mut self, revision: u64, thumbnails: model::ThumbnailMode) {
        assert!(matches!(self.work, WorkerState::Idle));
        let Some(script) = &self.script else {
            return;
        };
        assert_eq!(revision, script.buffer.revision);
        self.revision += 1;
        match self.worker.try_send(Command::Script {
            thumbnails,
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
                self.preview.dispatched(revision);
                if thumbnails == model::ThumbnailMode::Complete {
                    self.refinement.dispatched();
                }
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
                if let Some(sweep) = &mut self.sweep {
                    if matches!(&result, Ok(Outcome::Model(snapshot)) if snapshot.input.is_some()) {
                        sweep.result(current);
                    }
                }
                let run_revision = match active {
                    Some(ModelRevision::Script(revision)) => Some(revision),
                    _ => None,
                };
                if !current {
                    context.request_repaint();
                } else {
                    match result {
                        Ok(Outcome::Model(mut snapshot)) => {
                            self.outliner.sync(&snapshot.parts);
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
                            self.selected =
                                self.snapshot.as_ref().zip(self.selected.as_ref()).and_then(
                                    |(previous, selected)| {
                                        snapshot.restore_selection(
                                            previous,
                                            selected,
                                            self.selected_measurement.as_deref(),
                                        )
                                    },
                                );
                            if self.selected.is_none() {
                                self.selected_measurement = None;
                            }
                            self.snapshot = Some(*snapshot);
                        }
                        Ok(Outcome::Exported(message)) => self.message = message,
                        Ok(Outcome::Cancelled) => {}
                        Err(error) => {
                            if self.sweep.is_some() {
                                eprintln!("Profile: preview_sweep build_error={error}");
                            }
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
                eprintln!(
                    "Profile: preview_sweep_thumbnails renders={}",
                    self.snapshot
                        .as_ref()
                        .map_or(0, |snapshot| snapshot.thumbnail_renders)
                );
                self.sweep = None;
                self.exit = ExitState::Closing;
                context.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                context.request_repaint_after(sweep.delay(now));
            }
        }
    }

    fn raw_input_hook(&mut self, context: &egui::Context, input: &mut egui::RawInput) {
        self.code_state.route_vim_clipboard(context, input);
        if self.sweep.is_some() {
            input.events.clear();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let started = Instant::now();
        self.poll(ui.ctx());
        if self.parameter_editor.as_ref().is_some_and(|editor| {
            !self
                .script
                .as_ref()
                .is_some_and(|script| editor.current(script.buffer.revision))
                || ui.input(|input| input.viewport().close_requested())
        }) {
            self.parameter_editor = None;
            self.message = "Source changed or window closing · parameter edit cancelled".into();
        }
        let navigation_allowed = self.pending_document.is_none()
            && self.parameter_editor.is_none()
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
                if mode == palette::Mode::Files && self.code_state.vim_owns_shortcuts(ui.ctx()) {
                    if self.code_state.modal.mode != modal::Mode::Normal {
                        continue;
                    }
                }
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
                run: self.can_run(),
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
                if self.code_state.modal.enabled
                    && self.code_state.modal.mode == modal::Mode::Insert
                {
                    if input.consume_key(egui::Modifiers::CTRL, egui::Key::N) {
                        completion_navigate = true;
                        self.completion_index = self.completion_index.saturating_add(1);
                    }
                    if input.consume_key(egui::Modifiers::CTRL, egui::Key::P) {
                        completion_navigate = true;
                        self.completion_index = self.completion_index.saturating_sub(1);
                    }
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
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::O,
                ))
            });
        let mut close_after_save = false;
        let mut file_dialog_requested = None;
        let mut measurement_requested = None;
        let mut part_action = None;
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
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
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
                            frame_layout = true;
                        }
                        Navigation::Front => {
                            self.camera.yaw = -std::f32::consts::FRAC_PI_2;
                            self.camera.pitch = 0.0;
                        }
                        Navigation::Top => {
                            self.camera.yaw = -std::f32::consts::FRAC_PI_2;
                            self.camera.pitch = 1.55;
                        }
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
                let vim = self.code_state.vim_owns_shortcuts(ui.ctx());
                if ui.input_mut(|input| {
                    if input.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, key) {
                        return true;
                    }
                    if !vim {
                        return input.consume_key(egui::Modifiers::COMMAND, key);
                    }
                    false
                }) {
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
            egui::MenuBar::new().style(interface::menu_style).config(interface::menu_config()).ui(ui, |ui| {
                studio::product_mark(ui, 26.0);
                ui.label(egui::RichText::new("Spice CAD").size(15.0).strong());
                ui.separator();
                ui.menu_button("File", |ui| {
                    if ui.add(egui::Button::new("New model").shortcut_text("Ctrl+Shift+N")).clicked() {
                        file_dialog_requested = Some(save::Mode::NewModel);
                        ui.close();
                    }
                    if ui.add(egui::Button::new("Open…").shortcut_text("Ctrl+Shift+O")).clicked() {
                        open_requested = true;
                        ui.close();
                    }
                    if ui.add(egui::Button::new("Quick Open").shortcut_text("Ctrl+P")).clicked() {
                        palette_requested = Some(palette::Mode::Files);
                        ui.close();
                    }
                    ui.add_enabled_ui(self.script.is_some(), |ui| {
                        if ui.add(egui::Button::new("Save").shortcut_text("Ctrl+S")).clicked() {
                            save_requested = true;
                            ui.close();
                        }
                        if ui.add(egui::Button::new("Save as…").shortcut_text("Ctrl+Shift+S")).clicked() {
                            file_dialog_requested = Some(save::Mode::SaveAs);
                            ui.close();
                        }
                    });
                    ui.separator();
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
                        .on_disabled_hover_text("Run the current source successfully before exporting");
                    } else {
                        export_requested |= ui
                            .add_enabled(can_export, egui::Button::new("Export P1S parts"))
                            .clicked();
                    }
                    ui.separator();
                    ui.menu_button("Examples", |ui| {
                        for (label, example) in [
                            ("Spice rack", document::Example::Rack),
                            ("Spice rack · four parts", document::Example::RackParts),
                            ("Mounting bracket", document::Example::MountingBracket),
                            ("Measured block", document::Example::MeasuredBlock),
                            ("Extruded bracket", document::Example::ExtrudedBracket),
                            ("Rounded enclosure", document::Example::RoundedEnclosure),
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
                });
                ui.menu_button("Edit", |ui| {
                    let mut vim = self.code_state.modal.enabled;
                    if ui.checkbox(&mut vim, "Vim-style keys").changed() {
                        self.code_state.modal.enable(vim);
                    }
                    for (label, action) in [
                        (
                            "Find · / or Ctrl+Shift+F in Vim",
                            modal::Action::Find {
                                backward: false,
                                replace: false,
                            },
                        ),
                        (
                            "Replace · Ctrl+Shift+H in Vim",
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
                            ui.close();
                        }
                    }
                });
                ui.menu_button("View", |ui| {
                    let mut properties_visible = self.show_inspector && !self.focus_editor;
                    if ui.checkbox(&mut properties_visible, "Properties").changed() {
                        self.show_inspector = properties_visible;
                        if properties_visible {
                            self.focus_editor = false;
                        }
                    }
                    ui.add_enabled_ui(self.script.is_some(), |ui| {
                        let mut visible = !self.code_state.outputs.hidden;
                        if ui.checkbox(&mut visible, "Inline results").on_hover_text("Part previews and measurements alongside the source").changed() {
                            self.code_state.outputs.hidden = !visible;
                        }
                    });
                    ui.checkbox(&mut self.show_diagnostics, "Diagnostics");
                    ui.checkbox(&mut self.show_dimensions, "Measurements & face names");
                    ui.separator();
                    ui.menu_button("Appearance", |ui| {
                        interface::display_settings(ui, &mut self.show_edges, &mut self.edge_width_px);
                    });
                    ui.checkbox(&mut self.show_performance, "Performance");
                    ui.separator();
                    if ui.add(egui::Button::new("Command palette").shortcut_text("Ctrl+Shift+P")).clicked() {
                        palette_requested = Some(palette::Mode::Commands);
                        ui.close();
                    }
                });
                ui.menu_button("Model", |ui| {
                    ui.add_enabled_ui(self.script.is_some(), |ui| {
                        if ui.add_enabled(self.can_run(), egui::Button::new("Run").shortcut_text("Ctrl+Enter")).clicked() {
                            run_requested = true;
                            ui.close();
                        }
                        let mut automatic = self.preview.automatic();
                        if ui.checkbox(&mut automatic, "Live preview")
                            .on_hover_text("Update the model after a 250 ms typing pause; parameter controls update while dragging")
                            .changed() {
                            if let Some(script) = &self.script {
                                self.preview.set_automatic(automatic, script.buffer.revision, Instant::now());
                            }
                        }
                    });
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("About Spice CAD").clicked() {
                        self.show_about = true;
                        ui.close();
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                if let Some(script) = &self.script {
                    ui.allocate_ui_with_layout(
                        egui::vec2(280.0, interface::CONTROL_HEIGHT),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.add(egui::Label::new(script.path.file_name().unwrap_or_default().to_string_lossy()).truncate())
                                .on_hover_text(script.path.display().to_string());
                            if script.edited() {
                                let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 12.0), egui::Sense::hover());
                                ui.painter().circle_filled(rect.center(), 2.5, interface::ACCENT);
                            }
                        },
                    );
                    let can_run = self.can_run();
                    run_requested |= ui.add_enabled(
                        can_run,
                        egui::Button::new(if matches!(self.work, WorkerState::Busy(_)) { "Running…" } else { "Run" })
                            .min_size(egui::vec2(80.0, interface::CONTROL_HEIGHT)),
                    ).on_hover_text("Ctrl+Enter · Run the current source")
                        .on_disabled_hover_text(if !script.source_ready() {
                            "Loading source…"
                        } else if matches!(self.work, WorkerState::Idle) {
                            if self.preview.automatic() {
                                "Model is up to date. Live preview runs automatically after edits."
                            } else {
                                "Model is up to date. Run becomes available after source edits."
                            }
                        } else {
                            "Waiting for the current operation"
                        }).clicked();
                }
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
            ui.add_space(3.0);
        });
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::NONE
                    .fill(interface::PANEL)
                    .inner_margin(egui::Margin::symmetric(12, 3)),
            )
            .show(ui, |ui| {
                ui.spacing_mut().interact_size.y = 24.0;
                ui.spacing_mut().button_padding = egui::vec2(6.0, 2.0);
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);
                ui.horizontal(|ui| {
                    if let Some(diagnostics) = &self.diagnostics {
                        let (status, color) = diagnostics.status();
                        if ui
                            .selectable_label(
                                self.show_diagnostics,
                                egui::RichText::new(status).size(12.0).color(color),
                            )
                            .on_hover_text("Show or hide diagnostics")
                            .clicked()
                        {
                            self.show_diagnostics = !self.show_diagnostics;
                        }
                    }
                    if ui
                        .selectable_label(
                            self.show_details,
                            egui::RichText::new("Details").size(12.0),
                        )
                        .clicked()
                    {
                        self.show_details = !self.show_details;
                    }
                    ui.separator();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(script) = &self.script {
                            let cursor =
                                self.code_state.cursor().min(script.buffer.rope.len_chars());
                            let line = script.buffer.rope.char_to_line(cursor);
                            ui.label(
                                egui::RichText::new(format!(
                                    "Ln {}, Col {} · {} lines",
                                    line + 1,
                                    cursor - script.buffer.rope.line_to_char(line) + 1,
                                    script.buffer.rope.len_lines()
                                ))
                                .monospace()
                                .size(12.0),
                            );
                            ui.label(
                                egui::RichText::new(self.code_state.modal.label())
                                    .monospace()
                                    .size(12.0)
                                    .color(interface::ACCENT),
                            );
                            ui.separator();
                        }
                        let message = self
                            .code_state
                            .error
                            .or(self.code_state.modal.error)
                            .unwrap_or(&self.message);
                        ui.allocate_ui_with_layout(
                            ui.available_size(),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(message)
                                            .size(12.0)
                                            .color(interface::MUTED),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(message);
                            },
                        );
                    });
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
        if self.show_about {
            egui::Window::new("About Spice CAD")
                .open(&mut self.show_about)
                .resizable(false)
                .default_width(430.0)
                .show(ui.ctx(), |ui| {
                    studio::identity(ui, "Code-driven modeling · Native SMLib");
                    ui.horizontal(|ui| {
                        ui.label(concat!("Version ", env!("CARGO_PKG_VERSION")));
                        ui.hyperlink_to("Holy Furries Studio", "https://holyfurries.com");
                    });
                });
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
        if let Some(layout) = layout_requested.take() {
            frame_layout = self.change_layout(layout);
        }
        let mut parameter_edit = None;
        if let Some(editor) = &mut self.parameter_editor {
            let ready = matches!(self.work, WorkerState::Idle)
                && self
                    .script
                    .as_ref()
                    .is_some_and(|script| self.parameter_state.ready(script.buffer.revision));
            let close = match editor.show(ui.ctx(), ready) {
                Ok((edit, close)) => {
                    parameter_edit = edit;
                    close
                }
                Err(error) => {
                    self.message = error.into();
                    true
                }
            };
            if close {
                self.parameter_editor = None;
                self.code_state.focus();
            }
        }
        if self.show_inspector && !self.focus_editor {
            egui::Panel::right("inspector").default_size(320.0).min_size(280.0).max_size(440.0).frame(egui::Frame::side_top_panel(ui.style()).inner_margin(12.0)).show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
            ui.spacing_mut().interact_size.y = 28.0;
            ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
            egui::ScrollArea::vertical().id_salt("properties_scroll").show(ui, |ui| {
            if self.script.is_some() {
                ui.label(egui::RichText::new("Properties").size(16.0).strong());
                if let Some(snapshot) = &self.snapshot {
                    ui.small(egui::RichText::new(format!("{} {} · millimetres", snapshot.parts.len(), if snapshot.parts.len() == 1 { "part" } else { "parts" })).color(interface::MUTED));
                }
                ui.separator();
                if let Some(snapshot) = &self.snapshot {
                    if let Some(selection) = &self.selected {
                        interface::section(ui, "Selected geometry");
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
                        egui::Grid::new("selection_dimensions").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                            for span in selection.spans().into_iter().flatten() {
                                ui.label(span.direction.to_string());
                                ui.monospace(format!("{:.3} mm", span.distance_mm));
                                ui.end_row();
                            }
                        });
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
                    egui::CollapsingHeader::new(format!("Parts ({})", snapshot.parts.len())).id_salt("properties_parts").default_open(true).show(ui, |ui| {
                        let selected = self.selected.as_ref().and_then(|selection| snapshot.scene.picking().selection_part(selection));
                        let source_ready = self.script.as_ref().is_some_and(|script| snapshot.editor_revision == Some(script.buffer.revision));
                        let before = snapshot.visible_parts(&self.outliner);
                        part_action = self.outliner.show(ui, &snapshot.parts, selected, source_ready);
                        let after = snapshot.visible_parts(&self.outliner);
                        if before != after {
                            self.message = format!("{} / {} parts visible", after.count_ones(), snapshot.parts.len());
                            ui.ctx().request_repaint();
                        }
                    });
                }
                if let Some(script) = &self.script {
                    let parameters = self.code_state.parameters(&script.buffer, ui.ctx());
                    parameter_edit = self.parameter_state.show(ui, script.buffer.revision, parameters, save_requested || run_requested || export_requested || file_dialog_requested.is_some(), !matches!(self.work, WorkerState::Busy(_) | WorkerState::Export)).or(parameter_edit.take());
                }
                if let Some(snapshot) = &self.snapshot {
                    interface::section(ui, if snapshot.scene.layout == placement::Layout::Assembly { "Measurements" } else { "Assembly measurements" });
                    if self.script.as_ref().is_some_and(|script| snapshot.editor_revision != Some(script.buffer.revision)) {
                        ui.colored_label(interface::WARNING, "Showing the previous valid model");
                    }
                    let checked = snapshot.checks.measurements.iter().filter(|measurement| measurement.checked).count();
                    if checked > 0 {
                        ui.small(egui::RichText::new(format!("{checked} checks passed")).color(interface::MUTED));
                    }
                    if snapshot.checks.measurements.is_empty() {
                        ui.small("Select a face or edge to inspect dimensions.");
                    }
                    if snapshot.scene.layout == placement::Layout::Print {
                        ui.small("Switch to Assembly to edit these dimensions.");
                    }
                    for measurement in snapshot.checks.measurements.iter().filter(|measurement| measurement.visible(&picking::View { visible_parts: snapshot.visible_parts(&self.outliner), ..snapshot.scene.picking() })) {
                        let current = snapshot.scene.layout == placement::Layout::Assembly && self.script.as_ref().is_some_and(|script| snapshot.editor_revision == Some(script.buffer.revision));
                        let selected = self.selected_measurement.as_ref().map_or_else(|| self.selected.as_ref().is_some_and(|selection| measurement.matches_selection(selection)), |name| *name == measurement.name);
                        if ui.add_enabled(current, egui::Button::new("").left_text(&measurement.name).shortcut_text(format!("{:.2} mm", measurement.distance_mm)).min_size(egui::vec2(ui.available_width(), 28.0)).selected(selected).frame_when_inactive(false))
                            .on_hover_text(measurement.parameter.as_ref().map_or_else(|| "Reference dimension · reveal source".into(), |parameter| format!("Edit linked parameter: {}", parameter.name))).clicked() {
                            measurement_requested = Some(measurement.name.clone());
                        }
                    }
                    ui.add_space(8.0);
                    egui::CollapsingHeader::new("Geometry details").id_salt("properties_geometry").show(ui, |ui| {
                        ui.label(format!("Volume: {:.3} mm³", snapshot.volume_mm3));
                        ui.label(format!("{} faces · {} triangles", snapshot.scene.display.face_count(), snapshot.scene.display.vertex_count() / 3));
                    });
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
            if self.snapshot.is_some() && self.parameter_state.ready(script.buffer.revision) {
                sweep.sample(Instant::now());
            }
            if matches!(self.work, WorkerState::Idle)
                && self.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.editor_revision == Some(script.buffer.revision)
                })
            {
                if let Some((index, value, at)) = sweep.pending() {
                    match self.parameter_state.edit_literal(
                        script.buffer.revision,
                        sweep.property(),
                        value.to_string(),
                    ) {
                        Ok(Some(mut edit)) => {
                            edit.input_at = Some(at);
                            parameter_edit = Some(edit);
                            sweep_index = Some(index);
                        }
                        Ok(None) => {}
                        Err(error) => {
                            eprintln!("Profile: preview_sweep parameter_error={error}");
                            self.message = error.into();
                        }
                    }
                }
            }
        }
        if let Some(edit) = parameter_edit {
            if let Some(script) = &mut self.script {
                match self.code_state.replace_parameter(&mut script.buffer, &edit) {
                    Ok(()) => {
                        self.worker.invalidate_preview();
                        self.parameter_state.applied(&edit, script.buffer.revision);
                        if let (Some(sweep), Some(index)) = (&mut self.sweep, sweep_index) {
                            sweep.applied(index, script.buffer.revision);
                        }
                        self.preview_input = edit
                            .input_at
                            .filter(|_| self.profile.is_some())
                            .map(|at| performance::Input::new(script.buffer.revision, at));
                        let now = Instant::now();
                        self.refinement.edited(now);
                        self.preview.parameter(script.buffer.revision, now);
                        self.message = if self.preview.automatic() {
                            "Parameter changed · Live preview pending"
                        } else {
                            "Parameter changed in source · Run to refresh preview"
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
                    if code_editor::show(ui, &mut script.buffer, &mut self.code_state) {
                        self.message = if self.preview.automatic() {
                            "Source changed · Live preview pending"
                        } else {
                            "Source changed · Run to preview without saving"
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
            if run_requested
                && !self.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.editor_revision == Some(script.buffer.revision)
                })
            {
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
        if let Some(snapshot) = &self.snapshot {
            let mask = snapshot.visible_parts(&self.outliner);
            let picking = picking::View {
                visible_parts: mask,
                ..snapshot.scene.picking()
            };
            if self
                .selected
                .as_ref()
                .is_some_and(|selection| !picking.selection_visible(selection))
            {
                self.selected = None;
                self.selected_measurement = None;
            }
            if let Some(action) = part_action {
                match action {
                    outliner::Action::Select(index) | outliner::Action::Frame(index) => {
                        let faces = &snapshot.scene.display.parts[index].faces;
                        let bounds = snapshot.scene.part_bounds[index];
                        self.selected = Some(inspection::Selection::Part {
                            index: index as u32,
                            faces: [*faces.start(), *faces.end()],
                            bounds,
                        });
                        self.selected_measurement = None;
                        self.message = format!("Selected part · {}", snapshot.parts[index].name);
                        if matches!(action, outliner::Action::Frame(_)) {
                            self.camera.frame(bounds, self.viewport_aspect);
                            self.focus_editor = false;
                            self.message = format!("Framed part · {}", snapshot.parts[index].name);
                        }
                    }
                    outliner::Action::Source(index) => {
                        if let Some(script) = &self.script {
                            if let Some(line) = snapshot
                                .part_source(index, script.buffer.revision)
                                .filter(|line| *line < script.buffer.rope.len_lines())
                            {
                                let start = script.buffer.rope.line_to_char(line);
                                if self.code_state.modal.enabled {
                                    self.code_state.modal.mode = modal::Mode::Normal;
                                }
                                self.code_state.jump_to(start..start);
                                self.show_code = true;
                                self.message = format!(
                                    "Source · {} · line {}",
                                    snapshot.parts[index].name,
                                    line + 1
                                );
                            }
                        }
                    }
                }
                ui.ctx().request_repaint();
            }
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(interface::VIEWPORT))
            .show(ui, |ui| {
                if self.focus_editor {
                    return;
                }
                egui::Frame::NONE
                    .fill(interface::PANEL)
                    .inner_margin(egui::Margin::symmetric(8, 3))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        interface::menu_style(ui.style_mut());
                        ui.spacing_mut().interact_size.y = 28.0;
                        ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
                        ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
                        ui.horizontal_wrapped(|ui| {
                            if let Some(snapshot) = &self.snapshot {
                                if snapshot.alternate.is_some() {
                                    ui.menu_button(
                                        if snapshot.scene.layout == placement::Layout::Assembly {
                                            "Assembly"
                                        } else {
                                            "Print layout"
                                        },
                                        |ui| {
                                            for (layout, label) in [
                                                (placement::Layout::Assembly, "Assembly"),
                                                (placement::Layout::Print, "Print layout"),
                                            ] {
                                                if ui
                                                    .selectable_label(
                                                        snapshot.scene.layout == layout,
                                                        label,
                                                    )
                                                    .clicked()
                                                {
                                                    layout_requested = Some(layout);
                                                    ui.close();
                                                }
                                            }
                                        },
                                    );
                                }
                            }
                            if ui
                                .button("Frame")
                                .on_hover_text("F · Frame the selected part, or all visible parts")
                                .clicked()
                            {
                                frame_layout = true;
                            }
                            ui.menu_button("View", |ui| {
                                for (label, yaw, pitch) in [
                                    ("Isometric", -std::f32::consts::FRAC_PI_4, (1.0_f32 / 3.0_f32.sqrt()).asin()),
                                    ("Front", -std::f32::consts::FRAC_PI_2, 0.0),
                                    ("Right", 0.0, 0.0),
                                    ("Top", -std::f32::consts::FRAC_PI_2, 1.55),
                                ] {
                                    if ui.button(label).clicked() {
                                        self.camera.yaw = yaw;
                                        self.camera.pitch = pitch;
                                        ui.close();
                                    }
                                }
                            });
                            egui::containers::menu::MenuButton::new("Display")
                                .config(interface::menu_config())
                                .ui(ui, |ui| {
                                    ui.checkbox(
                                        &mut self.show_dimensions,
                                        "Measurements & face names",
                                    );
                                    ui.separator();
                                    interface::display_settings(
                                        ui,
                                        &mut self.show_edges,
                                        &mut self.edge_width_px,
                                    );
                                });
                        });
                    });
                let (rect, response) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
                if rect.width() <= 0.0 || rect.height() <= 0.0 {
                    return;
                }
                self.viewport_aspect = rect.width().max(1.0) / rect.height().max(1.0);
                if self.snapshot.is_none() {
                    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                        ui.centered_and_justified(|ui| {
                            ui.label("Open a model to begin · See Diagnostics if the model could not be built");
                        });
                    });
                    return;
                }
                if frame_layout {
                    self.frame_selection();
                }
                if rect.width() >= 100.0 {
                    if let Some(request) = self.code_state.outputs.take_focus() {
                        if let (Some(snapshot), Some(script)) = (&self.snapshot, &self.script) {
                            if let Some((selection, camera)) = focus_part(
                                snapshot,
                                &request,
                                script.buffer.revision,
                                self.viewport_aspect,
                                &self.camera,
                            ) {
                                self.outliner.reveal(&request.name);
                                self.selected = Some(selection);
                                self.selected_measurement = None;
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
                    self.frame_selection();
                }
                if let Some(snapshot) = &self.snapshot {
                    let visible_parts = snapshot.visible_parts(&self.outliner);
                    let picking = picking::View { visible_parts, ..snapshot.scene.picking() };
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
                                picking.pick_edge(
                                    matrix,
                                    [rect.width(), rect.height()],
                                    [position.x - rect.left(), position.y - rect.top()],
                                )
                            } else {
                                None
                            };
                            if let Some(edge) = edge {
                                self.selected = Some(inspection::Selection::Edge(edge));
                                self.selected_measurement = None;
                                self.message = "Edge dimensions · reference measurements".into();
                            } else if let Some(hit) = picking.pick(
                                matrix,
                                [
                                    (position.x - rect.left()) / rect.width() * 2.0 - 1.0,
                                    1.0 - (position.y - rect.top()) / rect.height() * 2.0,
                                ],
                            ) {
                                let face = *snapshot
                                    .scene
                                    .faces
                                    .get(&hit.face_id)
                                    .expect("Mesh face inspection cache");
                                match inspection::Selection::select(
                                    self.selected.as_ref(),
                                    face,
                                    extend,
                                ) {
                                    Ok(selection) => {
                                        self.message = if matches!(
                                            selection,
                                            inspection::Selection::Pair { .. }
                                        ) {
                                            "Parallel-face separation · reference measurement"
                                                .into()
                                        } else {
                                            "Face dimensions · reference measurements".into()
                                        };
                                        self.selected = Some(selection);
                                        self.selected_measurement = None;
                                    }
                                    Err(error) => self.message = error,
                                }
                            } else {
                                self.selected = None;
                                self.selected_measurement = None;
                                self.message = "Selection cleared".into();
                            }
                        }
                    }
                    ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                        rect,
                        viewport::Draw {
                            visible_parts,
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
                            selected: self
                                .selected
                                .as_ref()
                                .map_or([0; 2], inspection::Selection::ids),
                            selected_part: match &self.selected {
                                Some(inspection::Selection::Part { faces, .. }) => *faces,
                                _ => [0; 2],
                            },
                            selected_edge: match &self.selected {
                                Some(inspection::Selection::Edge(edge)) => Some(edge.segments),
                                _ => None,
                            },
                        },
                    ));
                    if let Some(bounds) = snapshot.scene.visible_bounds(visible_parts)
                        .filter(|_| self.show_dimensions || self.selected.is_some()) {
                        if let Some(name) = dimensions::draw(
                            ui,
                            rect,
                            matrix,
                            bounds,
                            &snapshot.scene.annotations,
                            self.selected.as_ref(),
                            |id| picking.face_visible(id),
                        ) {
                            measurement_requested = Some(name);
                        }
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
            });
        if let Some(layout) = layout_requested {
            if self.change_layout(layout) {
                ui.ctx().request_repaint();
            }
        }
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
        if let Some(name) = measurement_requested {
            if let Err(error) = self.open_measurement(&name) {
                self.message = error;
            }
            ui.ctx().request_repaint();
        }
        self.observe_preview();
        self.start_preview(ui.input(|input| input.pointer.any_down()));
        if matches!(self.work, WorkerState::Idle) && !ui.input(|input| input.pointer.any_down()) {
            if let (Some(snapshot), Some(script), Some(delay)) = (
                &self.snapshot,
                &self.script,
                self.refinement.delay(Instant::now()),
            ) {
                if !snapshot.complete() && snapshot.editor_revision == Some(script.buffer.revision)
                {
                    if delay.is_zero() {
                        self.preview_input = None;
                        self.start_script(script.buffer.revision, model::ThumbnailMode::Complete);
                    } else {
                        ui.ctx().request_repaint_after(delay);
                    }
                }
            }
        }
        let startup_pending = self.snapshot.is_none()
            && (matches!(self.work, WorkerState::Busy(_)) || self.preview.pending());
        if startup_pending {
            if let Some(splash) = &mut self.splash {
                match splash.show(ui.ctx()) {
                    Ok(true) => {}
                    Ok(false) => self.splash = None,
                    Err(error) => {
                        self.message = format!("Could not load shared splash: {error}");
                        self.splash = None;
                    }
                }
            }
        } else {
            self.splash = None;
        }
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
            let ready = self.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.complete() && viewport::geometry_prepared(frame, &snapshot.scene.display)
            }) && matches!(self.work, WorkerState::Idle);
            if sweep.finish(
                metrics
                    .input_latency
                    .filter(|_| ready)
                    .map(|latency| latency.revision),
                now,
            ) {
                eprintln!(
                    "Profile: preview_sweep_quality final={}",
                    self.snapshot
                        .as_ref()
                        .is_some_and(|snapshot| !snapshot.refinement_pending)
                );
                eprintln!(
                    "Profile: preview_sweep_gpu uploads={} geometry_allocations={} geometry_bytes={} placement_uploads={}",
                    metrics.uploads,
                    metrics.buffer_allocations,
                    metrics.geometry_bytes,
                    metrics.placement_writes
                );
                eprintln!(
                    "Profile: preview_sweep_thumbnails renders={}",
                    self.snapshot
                        .as_ref()
                        .map_or(0, |snapshot| snapshot.thumbnail_renders)
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
    let mut camera = *previous;
    camera.frame(bounds, aspect);
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
        let tessellation = shape.mesh()?;
        let mesh = tessellation.mesh;
        mesh_ms.push(start.elapsed().as_secs_f64() * 1000.0);
        volume = mesh.volume_mm3();
        let exact = tessellation.volume_mm3;
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

    #[cfg(feature = "rack-tools")]
    #[test]
    fn outliner_keeps_the_rack_tools_model_visible_and_frameable() -> Result<(), String> {
        let kernel = kernel::Kernel::new()?;
        let snapshot = build_snapshot(&kernel, PARAMETERS, 1)?;
        let mask = snapshot.visible_parts(&outliner::State::default());
        assert_eq!(mask, u32::MAX);
        assert_eq!(
            snapshot.scene.visible_bounds(mask),
            Some(snapshot.scene.bounds)
        );
        assert_eq!(
            snapshot.scene.part_bounds.len(),
            snapshot.scene.display.parts.len()
        );
        Ok(())
    }

    #[test]
    fn outliner_part_selection_tracks_names_even_when_geometry_is_shared() -> Result<(), String> {
        let source = "cad.show('first',cad.box({width=10,depth=10,height=10}));cad.show('second',cad.box({width=10,depth=10,height=10}))";
        let before = build_script_snapshot(source, 1)?;
        let selection = inspection::Selection::Part {
            index: 0,
            faces: [1, 6],
            bounds: before.scene.part_bounds[0],
        };
        let mut after = build_script_snapshot(source, 2)?;
        after.scene.display = Arc::clone(&before.scene.display);
        after.parts.swap(0, 1);
        let restored = after
            .restore_selection(&before, &selection, None)
            .expect("Named part retained");
        assert_eq!(after.scene.picking().selection_part(&restored), Some(1));
        after.parts[1].name = "renamed".into();
        assert!(after.restore_selection(&before, &selection, None).is_none());
        Ok(())
    }

    #[test]
    fn outliner_source_locations_require_the_current_revision() -> Result<(), String> {
        let source = "local part=cad.box({width=10,depth=10,height=10})\n\ncad.show('part',part)";
        let mut snapshot = build_script_snapshot(source, 1)?;
        snapshot.editor_revision = Some(42);
        assert_eq!(snapshot.part_source(0, 42), Some(2));
        assert_eq!(snapshot.part_source(0, 43), None);
        assert_eq!(snapshot.part_source(1, 42), None);
        Ok(())
    }

    #[test]
    fn named_measurements_survive_shape_changes_and_clear_when_removed() -> Result<(), String> {
        let source = include_str!("../../models/measured-block.luau");
        let first = build_script_snapshot(source, 1)?;
        let selected = first.checks.measurements[0]
            .selection(&first.scene.faces)
            .ok_or("Named measurement selection")?;
        let second = build_script_snapshot(&source.replace("cad.number(20,", "cad.number(24,"), 2)?;
        assert!(!Arc::ptr_eq(&first.scene.display, &second.scene.display));
        let restored = second
            .restore_selection(&first, &selected, Some("height"))
            .ok_or("Restored measurement")?;
        assert!(second.checks.measurements[0].matches_selection(&restored));
        let distance = restored.spans()[0].ok_or("Restored span")?.distance_mm;
        assert!((distance - 24.0).abs() < 0.001);
        let removed = build_script_snapshot(
            &source
                .lines()
                .filter(|line| !line.starts_with("cad.measure"))
                .collect::<Vec<_>>()
                .join("\n"),
            3,
        )?;
        assert!(
            removed
                .restore_selection(&first, &selected, Some("height"))
                .is_none()
        );
        let duplicate = source.replace(
            "cad.show",
            "cad.measure(\"other height\", {from=bottom,to=top,parameter=\"width\"})\ncad.show",
        );
        let previous = build_script_snapshot(&duplicate, 4)?;
        let selected = previous.checks.measurements[1]
            .selection(&previous.scene.faces)
            .ok_or("Second measurement sharing faces")?;
        let renamed = duplicate
            .lines()
            .filter(|line| !line.starts_with("cad.measure(\"height\""))
            .collect::<Vec<_>>()
            .join("\n");
        let current = build_script_snapshot(&renamed, 5)?;
        assert!(
            current
                .restore_selection(&previous, &selected, Some("other height"))
                .is_some()
        );
        assert!(
            current
                .restore_selection(&previous, &selected, Some("height"))
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn rack_measurements_follow_properties_and_displayed_joint_separation() -> Result<(), String> {
        let source = include_str!("../../models/rack-parts.luau");
        let kernel = kernel::Kernel::new()?;
        let mut cache = model::Cache::new(&kernel);
        let mut previous: Option<Snapshot> = None;
        for (index, source, expected) in [
            (1, source.to_owned(), [8.0, 53.0, 3.0, 12.2]),
            (
                2,
                source
                    .replace("cad.number(8,", "cad.number(12,")
                    .replace("cad.number(3,", "cad.number(4,")
                    .replace("cad.number(50,", "cad.number(60,")
                    .replace("cad.number(0.2,", "cad.number(0.4,"),
                [12.0, 63.0, 4.0, 12.4],
            ),
            (
                3,
                source.replace("cad.number(12,", "cad.number(0,"),
                [8.0, 53.0, 3.0, 0.2],
            ),
            (
                4,
                source.replace("cad.number(5,", "cad.number(3,"),
                [8.0, 53.0, 3.0, 12.2],
            ),
            (
                5,
                source.replace("cad.number(5,", "cad.number(8,"),
                [8.0, 53.0, 3.0, 12.2],
            ),
        ] {
            let snapshot = build_script_snapshot_cached(source, index, &mut cache)?;
            if let Some(previous) = &previous {
                let selected = previous.checks.measurements[0]
                    .selection(&previous.scene.faces)
                    .ok_or("Previous lip selection")?;
                let restored = snapshot
                    .restore_selection(previous, &selected, Some("Lip height"))
                    .ok_or("Rack lip selection after edit")?;
                assert!(snapshot.checks.measurements[0].matches_selection(&restored));
            }
            assert_eq!(snapshot.checks.measurements.len(), 4);
            for ((measurement, name), (parameter, expected)) in snapshot
                .checks
                .measurements
                .iter()
                .zip([
                    "Lip height",
                    "Usable tread depth",
                    "Wall thickness",
                    "Joint tip gap",
                ])
                .zip(
                    ["lip", "jar_diameter", "wall", "joint_clearance"]
                        .into_iter()
                        .zip(expected),
                )
            {
                assert_eq!(measurement.name, name);
                assert_eq!(
                    measurement
                        .parameter
                        .as_ref()
                        .map(|parameter| parameter.name.as_str()),
                    Some(parameter)
                );
                assert!(
                    (measurement.distance_mm - expected).abs() < 0.001,
                    "{name}: {}",
                    measurement.distance_mm
                );
            }
            previous = Some(snapshot);
        }
        Ok(())
    }

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
            let presentations = cache.presentations(
                &model,
                placement::Layout::Assembly,
                model::ThumbnailMode::Complete,
            )?;
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
        let error = build_script_snapshot_with_thumbnails(
            source.replacen("number(10,", "number(20,", 1),
            2,
            &mut cache,
            model::ThumbnailMode::Deferred,
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
        let deferred = build_script_snapshot_with_thumbnails(
            rotated.clone(),
            3,
            &mut cache,
            model::ThumbnailMode::Deferred,
        )?;
        assert!(deferred.thumbnails_pending);
        assert_eq!(deferred.thumbnail_renders, second.thumbnail_renders);
        assert_eq!(deferred.build_ms, 0.0);
        assert!(matches!(
            deferred.scene.outputs[0].content,
            inline_output::Content::Part { image: None, .. }
        ));
        let third = build_script_snapshot_cached(rotated.clone(), 4, &mut cache)?;
        assert!(!third.thumbnails_pending);
        assert_eq!(third.thumbnail_renders, second.thumbnail_renders + 1);
        assert_eq!(deferred.scene.bounds, third.scene.bounds);
        assert!(Arc::ptr_eq(&deferred.scene.display, &third.scene.display));
        for (id, face) in &deferred.scene.faces {
            assert_eq!(face.center, third.scene.faces[id].center);
            for (a, b) in face
                .spans
                .iter()
                .flatten()
                .zip(third.scene.faces[id].spans.iter().flatten())
            {
                assert_eq!(a.points, b.points);
                assert_eq!(a.distance_mm, b.distance_mm);
            }
        }
        let reused = build_script_snapshot_with_thumbnails(
            rotated.clone(),
            5,
            &mut cache,
            model::ThumbnailMode::Deferred,
        )?;
        assert!(!reused.thumbnails_pending);
        assert_eq!(reused.thumbnail_renders, third.thumbnail_renders);
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
    fn rotation_sweep_keeps_exact_bounds_and_shared_geometry() -> Result<(), String> {
        let sweep = performance::Sweep::new(
            Instant::now(),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            performance::SweepKind::Rotation,
        );
        let source = sweep.source();
        let kernel = kernel::Kernel::new()?;
        let mut cache = model::Cache::new(&kernel);
        let initial = build_script_snapshot_cached(source.clone(), 1, &mut cache)?;
        for angle in 1..=100 {
            let source = source.replace("cad.number(0,", &format!("cad.number({angle},"));
            let current = build_script_snapshot_with_thumbnails(
                source,
                angle + 1,
                &mut cache,
                model::ThumbnailMode::Deferred,
            )
            .map_err(|error| format!("Rotation {angle}: {error}"))?;
            assert!(current.thumbnails_pending);
            assert!(Arc::ptr_eq(&initial.scene.display, &current.scene.display));
            assert_eq!(initial.thumbnail_renders, current.thumbnail_renders);
            assert_eq!(current.cached_parts, 4);
        }
        Ok(())
    }

    #[test]
    fn slider_snapshot_converges_to_final_quality_and_clears_unstable_face_selection()
    -> Result<(), String> {
        let source = include_str!("../../models/mounting-bracket.luau").to_owned();
        let kernel = kernel::Kernel::new()?;
        let mut cache = model::Cache::new(&kernel);
        let interactive = build_script_snapshot_with_thumbnails(
            source.clone(),
            1,
            &mut cache,
            model::ThumbnailMode::Deferred,
        )?;
        let refined = build_script_snapshot_cached(source.clone(), 2, &mut cache)?;
        let direct = build_script_snapshot_cached(source, 3, &mut model::Cache::new(&kernel))?;
        let selected = inspection::Selection::Face(
            *interactive
                .scene
                .faces
                .values()
                .next()
                .expect("Bracket faces"),
        );
        let restored = refined.restore_selection(&interactive, &selected, None);
        let vertices = |snapshot: &Snapshot| {
            let mesh = snapshot.scene.world_mesh();
            bytemuck::cast_slice::<mesh::Vertex, u8>(&mesh.vertices).to_vec()
        };
        drop(cache);
        drop(kernel);
        assert!(interactive.refinement_pending);
        assert!(interactive.thumbnails_pending);
        assert!(!interactive.complete());
        assert!(!refined.refinement_pending);
        assert!(!refined.thumbnails_pending);
        assert!(refined.complete());
        assert_eq!(refined.build_ms, 0.0);
        assert_eq!(refined.cached_parts, 1);
        assert!(restored.is_none());
        assert_eq!(refined.volume_mm3, direct.volume_mm3);
        assert_eq!(vertices(&refined), vertices(&direct));
        Ok(())
    }

    #[test]
    fn geometry_sweep_rebuilds_holes_and_reuses_the_final_revision() -> Result<(), String> {
        let sweep = performance::Sweep::new(
            Instant::now(),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            performance::SweepKind::Geometry,
        );
        let source = sweep.source();
        let kernel = kernel::Kernel::new()?;
        let mut cache = model::Cache::new(&kernel);
        let initial = build_script_snapshot_with_thumbnails(
            source.clone(),
            1,
            &mut cache,
            model::ThumbnailMode::Deferred,
        )?;
        let changed_source = source.replace("cad.number(3,", "cad.number(5,");
        let changed = build_script_snapshot_with_thumbnails(
            changed_source.clone(),
            2,
            &mut cache,
            model::ThumbnailMode::Deferred,
        )?;
        let repeated = build_script_snapshot_with_thumbnails(
            changed_source,
            3,
            &mut cache,
            model::ThumbnailMode::Deferred,
        )?;
        drop(cache);
        drop(kernel);
        assert_eq!(changed.cached_parts, 0);
        assert!(initial.volume_mm3 > changed.volume_mm3);
        assert!(!Arc::ptr_eq(&initial.scene.display, &changed.scene.display));
        assert_eq!(repeated.cached_parts, 1);
        assert!(Arc::ptr_eq(&changed.scene.display, &repeated.scene.display));
        assert_eq!(changed.scene.bounds, initial.scene.bounds);
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
        let mut mesh = body.mesh()?.mesh;
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
                .split("for index, u")
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
    fn worker_discards_superseded_requests_and_builds_latest_without_error() -> Result<(), String> {
        let worker = Worker::new(egui::Context::default())?;
        let obsolete = worker.generation.token();
        worker.invalidate_preview();
        let command = |source: &str, revision| Command::Script {
            source: source.into(),
            thumbnails: model::ThumbnailMode::Deferred,
            input: None,
            editor_revision: revision,
            revision,
            saved_at: None,
        };
        worker
            .commands
            .as_ref()
            .expect("Live worker")
            .try_send((
                command("error('This obsolete error must never be reported')", 1),
                obsolete,
            ))
            .map_err(|error| error.to_string())?;
        let obsolete = worker
            .recv_timeout(Duration::from_secs(20))
            .map_err(|error| error.to_string())??;
        let cancelled = matches!(obsolete, Outcome::Cancelled);
        worker
            .try_send(command(
                "cad.show('latest',cad.box({width=2,depth=3,height=4}))",
                2,
            ))
            .map_err(|error| error.to_string())?;
        let latest = worker
            .recv_timeout(Duration::from_secs(20))
            .map_err(|error| error.to_string())??;
        let latest_revision = match latest {
            Outcome::Model(snapshot) => Some(snapshot.revision),
            _ => None,
        };
        drop(worker);
        assert!(cancelled);
        assert_eq!(latest_revision, Some(2));
        Ok(())
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
                thumbnails: model::ThumbnailMode::Complete,
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
                thumbnails: model::ThumbnailMode::Complete,
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
            generation: cancellation::Generation::default(),
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
                    thumbnails: model::ThumbnailMode::Complete,
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
                    thumbnails: model::ThumbnailMode::Complete,
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
                    Outcome::Exported(_) | Outcome::Cancelled => {
                        return Err("Expected script model".into());
                    }
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
            Outcome::Exported(_) | Outcome::Cancelled => {
                return Err("Expected rebuilt geometry".into());
            }
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
