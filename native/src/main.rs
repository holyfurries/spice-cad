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
mod analysis;
mod code_editor;
mod diagnostics;
mod dimensions;
mod document;
mod export;
mod files;
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
mod preview;
mod save;
mod search;
mod syntax;
mod text_buffer;
mod viewport;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    let mut arguments: Vec<String> = std::env::args().skip(1).take(7).collect();
    let profile = arguments
        .iter()
        .position(|argument| argument == "--profile")
        .map(|index| {
            arguments.remove(index);
            started
        });
    #[cfg(feature = "rack-tools")]
    if arguments.as_slice() == ["--benchmark"] {
        return benchmark().map_err(Into::into);
    }
    let model_path = match arguments.as_slice() {
        [] => Some(document::example_model(document::Example::Rack)?),
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
                snapshot.mesh.vertices.len() / 3,
                snapshot.analysis_ms,
                snapshot.script_ms,
                snapshot.build_ms,
                snapshot.tessellation_ms,
                snapshot.total_ms
            );
            return Ok(());
        }
        [flag, path, folder] if flag == "--export-model" => {
            let source = model::read_source(std::path::Path::new(path))?;
            export::model_files(&source, std::path::Path::new(folder))?;
            println!("Exported native STEP, STL, and source to {folder}");
            return Ok(());
        }
        _ => {
            return Err(
                (if cfg!(feature = "rack-tools") {
                    "Usage: spice-cad [--profile] [--rack | --benchmark | --model FILE.luau | --check-model FILE.luau | --export-model FILE.luau NEW_DIRECTORY]"
                } else {
                    "Usage: spice-cad [--profile] [--model FILE.luau | --check-model FILE.luau | --export-model FILE.luau NEW_DIRECTORY]"
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
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([1100.0, 650.0]),
        renderer: eframe::Renderer::Wgpu,
        depth_buffer: 32,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            wgpu_setup: setup.into(),
            ..Default::default()
        },
        ..Default::default()
    };
    performance::stage(profile, "before_event_loop");
    eframe::run_native(
        "Spice CAD · native SMLib",
        options,
        Box::new(move |context| Ok(Box::new(Editor::new(context, model_path, profile)?))),
    )?;
    Ok(())
}

enum Command {
    ExportScript {
        source: String,
        folder: std::path::PathBuf,
    },
    Script {
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
struct Snapshot {
    faces: std::collections::BTreeMap<u32, inspection::Face>,
    bounds: [glam::Vec3; 2],
    annotations: dimensions::Annotations,
    editor_revision: Option<u64>,
    edges: Arc<Vec<mesh::EdgeSegment>>,
    saved_at: Option<std::time::SystemTime>,
    analysis_ms: f64,
    script_ms: f64,
    total_ms: f64,
    source: ModelSource,
    name: String,
    mesh: Arc<mesh::Mesh>,
    volume_mm3: f64,
    build_ms: f64,
    tessellation_ms: f64,
    revision: u64,
}
#[derive(PartialEq)]
enum ModelSource {
    Rack(RackParameters),
    Script(String),
}
struct Worker {
    commands: mpsc::SyncSender<Command>,
    results: mpsc::Receiver<Result<Outcome, String>>,
}

impl Worker {
    fn new(context: egui::Context) -> Result<Self, String> {
        let (commands, requests) = mpsc::sync_channel(1);
        let (responses, results) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("cad-kernel".into())
            .spawn(move || {
                while let Ok(command) = requests.recv() {
                    let result = match command {
                        Command::ExportScript { source, folder } => export::model_files(&source, &folder).map(|()| Outcome::Exported(format!("Exported native STEP, STL, and source to {}", folder.display()))),
                        Command::Script { source, editor_revision, revision, saved_at } => {
                            let source = source.to_string();
                            build_script_snapshot(&source, revision).map(|mut snapshot| { snapshot.saved_at = saved_at; snapshot.editor_revision = Some(editor_revision); Outcome::Model(Box::new(snapshot)) })
                        }
                        Command::Build {
                            parameters,
                            revision,
                        } => kernel::Kernel::new()
                            .and_then(|kernel| build_snapshot(&kernel, parameters, revision))
                            .map(|snapshot| Outcome::Model(Box::new(snapshot))),
                        #[cfg(feature = "rack-tools")]
                        Command::Export { parameters } => {
                            export_parts(parameters, std::path::Path::new("output/native"))
                                .map(|()| Outcome::Exported("Exported four rack parts, four fit samples, and assembled STEP to output/native".into()))
                        }
                    };
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
        Ok(Self { commands, results })
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
    let edges = body.edge_segments()?;
    let faces = inspection::faces(&mesh)?;
    let tessellation_ms = start.elapsed().as_secs_f64() * 1000.0;
    let volume_mm3 = mesh.volume_mm3();
    Ok(Snapshot {
        faces,
        bounds: body.bounds(true)?.map(|point| point.as_vec3()),
        annotations: dimensions::Annotations::default(),
        editor_revision: None,
        edges: Arc::new(edges),
        saved_at: None,
        analysis_ms: 0.0,
        script_ms: 0.0,
        total_ms: build_ms + tessellation_ms,
        source: ModelSource::Rack(parameters),
        name: "Spice rack".into(),
        mesh: Arc::new(mesh),
        volume_mm3,
        build_ms,
        tessellation_ms,
        revision,
    })
}

fn build_script_snapshot(source: &str, revision: u64) -> Result<Snapshot, String> {
    assert!(revision > 0);
    let total = Instant::now();
    let model = model::compile(source)?;
    let kernel = kernel::Kernel::new()?;
    let start = Instant::now();
    let mut body = model.build(&kernel)?;
    let volume_mm3 = body.volume()?;
    let build_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let mut mesh = body.mesh()?;
    mesh.smooth_normals();
    let edges = body.edge_segments()?;
    let faces = inspection::faces(&mesh)?;
    let tessellation_ms = start.elapsed().as_secs_f64() * 1000.0;
    let annotations = model.annotations(&mesh)?;
    Ok(Snapshot {
        faces,
        bounds: body.bounds(true)?.map(|point| point.as_vec3()),
        annotations,
        editor_revision: None,
        edges: Arc::new(edges),
        saved_at: None,
        analysis_ms: model.analysis_ms,
        script_ms: model.script_ms,
        total_ms: total.elapsed().as_secs_f64() * 1000.0,
        source: ModelSource::Script(source.into()),
        name: model.name,
        volume_mm3,
        mesh: Arc::new(mesh),
        build_ms,
        tessellation_ms,
        revision,
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
    ) -> Result<Self, String> {
        performance::stage(profile, "device_and_egui_ready");
        viewport::initialize(context)?;
        performance::stage(profile, "viewport_ready");
        interface::configure(&context.egui_ctx);
        let layout = PARAMETERS.layout()?;
        let worker = Worker::new(context.egui_ctx.clone())?;
        let script = model_path.map(document::Document::new);
        let diagnostics = script
            .as_ref()
            .map(|script| diagnostics::Session::new(script.path.clone(), context.egui_ctx.clone()))
            .transpose()?;
        let watcher = script
            .as_ref()
            .map(|script| document::Watcher::new(script.path.clone(), context.egui_ctx.clone()))
            .transpose()?;
        let mut code_state = code_editor::State::default();
        code_state.modal.enable(true);
        let mut palette = palette::State::default();
        let dirty_since = script
            .is_none()
            .then_some(Instant::now() - Duration::from_secs(1));
        if let Some(script) = &script {
            palette.remember(script.path.clone());
        }
        Ok(Self {
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
            show_inspector: false,
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

    fn export(&mut self) -> Result<(), String> {
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
                    source: source.clone(),
                    folder: format!("output/native/model-{stamp}").into(),
                }
            }
        };
        match self.worker.commands.try_send(command) {
            Ok(()) => {
                self.work = WorkerState::Export;
                self.message = "Generating and validating STEP/STL exports…".into();
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
        match self.worker.commands.try_send(Command::Script {
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
        match self.worker.results.try_recv() {
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
                        Ok(Outcome::Model(snapshot)) => {
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
                            self.snapshot = Some(*snapshot);
                            self.selected = None;
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
                            match self.worker.commands.try_send(Command::Build {
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
                        Navigation::Bracket => {
                            example_requested = Some(document::Example::MountingBracket)
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
                        ("Mounting bracket", document::Example::MountingBracket),
                        ("Measured block", document::Example::MeasuredBlock),
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
                                    .min_size(egui::vec2(56.0, interface::CONTROL_HEIGHT)),
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
                        egui::vec2(180.0, 28.0),
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
                    save_requested |= ui
                        .button("Save")
                        .on_hover_text("Ctrl+S · Save and rebuild")
                        .clicked();
                    run_requested |= ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Run").color(interface::ON_ACCENT),
                            )
                            .fill(interface::ACCENT),
                        )
                        .on_hover_text("Ctrl+Enter · Preview editor buffer")
                        .clicked();
                    let mut automatic = self.preview.automatic();
                    if ui
                        .toggle_value(&mut automatic, "Auto Preview")
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
                let can_export = self.can_export();
                export_requested |= ui
                    .add_enabled(
                        can_export,
                        egui::Button::new(if self.script.is_some() {
                            "Export"
                        } else {
                            "Export P1S parts"
                        }),
                    )
                    .on_hover_text(if can_export {
                        "Export validated STEP and STL for slicing"
                    } else {
                        "Run the current code successfully before exporting"
                    })
                    .on_disabled_hover_text("Run the current code successfully before exporting")
                    .clicked();
                if self.script.is_some() {
                    ui.separator();
                    if ui
                        .selectable_label(self.show_inspector && !self.focus_editor, "Parameters")
                        .on_hover_text("Edit named Luau values in the properties pane")
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
                        let (_, upload_ms, saved_ms) = viewport::upload_metrics(frame);
                        ui.label(format!("Upload CPU {:.2} ms", upload_ms));
                        if let Some(saved_ms) = saved_ms {
                            ui.label(format!("Save → upload {:.1} ms", saved_ms));
                        }
                    }
                    ui.label(format!(
                        "UI CPU {:.2} ms · {} uploads",
                        self.ui_ms,
                        viewport::upload_count(frame)
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
        let mut parameter_edit = None;
        if self.show_inspector && !self.focus_editor {
            egui::Panel::right("inspector").default_size(280.0).min_size(240.0).max_size(380.0).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("properties_scroll").show(ui, |ui| {
            if self.script.is_some() {
                interface::section(ui, "MODEL");
                ui.heading(self.snapshot.as_ref().map_or("Luau model", |snapshot| snapshot.name.as_str()));
                ui.small("Solid body · millimetres");
                ui.separator();
                if let Some(script) = &self.script {
                    let parameters = self.code_state.parameters(&script.buffer, ui.ctx());
                    parameter_edit = self.parameter_state.show(ui, script.buffer.revision, parameters, save_requested || run_requested || export_requested || file_dialog_requested.is_some());
                }
                if let Some(snapshot) = &self.snapshot {
                    interface::section(ui, "GEOMETRY");
                    ui.label(format!("Volume {:.3} mm³", snapshot.volume_mm3));
                    ui.label(format!("{} faces · {} triangles", snapshot.mesh.face_count, snapshot.mesh.vertices.len() / 3));
                    interface::section(ui, "SELECTION");
                    if let Some(selection) = &self.selected {
                        let face = selection.latest().id;
                        ui.label(snapshot.annotations.faces.iter().find(|named| named.id == face).map_or("Unnamed face", |named| named.name.as_str()));
                        ui.small(if matches!(selection, inspection::Selection::Pair { .. }) { "Parallel-face separation · reference measurement" } else if selection.planar() { "Planar face spans · reference measurements" } else { "Curved face · X/Y/Z bounds, not surface lengths" });
                        for span in selection.spans().into_iter().flatten() {
                            ui.label(format!("{} ≈{:.3} mm", span.direction, span.distance_mm));
                        }
                        if ui.button("Copy face selector").on_hover_text("Axis-aligned planar faces only. Replace part with your final solid variable and choose a unique name.").clicked() {
                            match dimensions::selector(&snapshot.mesh, face) {
                                Some(selector) => ui.ctx().copy_text(selector),
                                None => self.message = "This face is not axis-aligned and planar; no selector was copied".into(),
                            }
                        }
                    } else { ui.label("Click a model face to see its dimensions"); }
                    ui.small("Shift-click a parallel planar face to measure separation. Click empty space to clear.");
                    interface::section(ui, "DESIGN CHECKS");
                    if self.script.as_ref().is_some_and(|script| snapshot.editor_revision != Some(script.buffer.revision)) {
                        ui.colored_label(interface::WARNING, "Measurements refer to the previous valid preview");
                    }
                    let checked = snapshot.annotations.measurements.iter().filter(|measurement| measurement.checked).count();
                    ui.label(if checked == 0 { "No explicit distance checks".into() } else { format!("{checked} distance checks passed") });
                    ui.small("Code defines shape. Checks validate declared limits; no constraint solver is active.");
                    for measurement in &snapshot.annotations.measurements {
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
                ui.label(format!("{} triangles · {} faces", snapshot.mesh.vertices.len()/3, snapshot.mesh.face_count));
                ui.label(format!("Volume {:.0} mm³", snapshot.volume_mm3));
                if snapshot.source != ModelSource::Rack(self.parameters) { ui.colored_label(interface::WARNING, "Showing the previous valid geometry"); }
            }
            ui.separator(); ui.label(self.selected.as_ref().map_or("Click a face to select".into(), |selection| format!("Selected face {}", selection.latest().id)));
            ui.separator(); ui.small("Preview: rack blank. Print export adds the splitting joints. Joint clearance: 0.2 mm.");
            ui.small("Export uses OpenCascade for STEP and verified print geometry. Print and dry-fit the samples first.");
        });
        });
        }
        if let Some(edit) = parameter_edit {
            if let Some(script) = &mut self.script {
                match self.code_state.replace_parameter(&mut script.buffer, &edit) {
                    Ok(()) => {
                        self.parameter_state = parameters::State::default();
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
                    (ui.available_width() - 240.0).max(240.0)
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        interface::panel_title(ui, "CODE");
                        ui.label("Luau")
                            .on_hover_text(script.path.display().to_string());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
            if let Err(error) = self.export() {
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
                self.viewport_aspect = rect.width().max(1.0) / rect.height().max(1.0);
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
                            let picked = snapshot.mesh.pick(
                                matrix,
                                [
                                    (position.x - rect.left()) / rect.width() * 2.0 - 1.0,
                                    1.0 - (position.y - rect.top()) / rect.height() * 2.0,
                                ],
                            );
                            if let Some(id) = picked {
                                let face = *snapshot.faces.get(&id).expect("Mesh face inspection cache");
                                match inspection::Selection::select(self.selected.as_ref(), face, ui.input(|input| input.modifiers.shift)) {
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
                            edge_width_px: self.edge_width_px,
                            edges: Arc::clone(&snapshot.edges),
                            edges_enabled: self.show_edges,
                            eye: self.camera.eye(),
                            viewport_size: [
                                rect.width() * ui.ctx().pixels_per_point(),
                                rect.height() * ui.ctx().pixels_per_point(),
                            ],
                            saved_at: snapshot.saved_at,
                            mesh: Arc::clone(&snapshot.mesh),
                            revision: snapshot.revision,
                            matrix,
                            selected: self.selected.as_ref().map_or([0; 2], inspection::Selection::ids),
                        },
                    ));
                    if self.show_dimensions || self.selected.is_some() {
                        dimensions::draw(ui, rect, matrix, self.camera.eye(), snapshot.bounds, &snapshot.annotations, self.selected.as_ref());
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
                    rect.left_top() + egui::vec2(12.0, 6.0),
                    egui::vec2((rect.width() - 24.0).max(0.0), 76.0),
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
                        .corner_radius(4.0)
                        .inner_margin(4.0)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.visuals_mut().button_frame = false;
                                if ui
                                    .add(
                                        egui::Button::new("Frame · F")
                                            .min_size(egui::vec2(76.0, interface::CONTROL_HEIGHT)),
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
                                            .min_size(egui::vec2(52.0, interface::CONTROL_HEIGHT)),
                                    )
                                    .clicked()
                                {
                                    self.camera.yaw = -std::f32::consts::FRAC_PI_2;
                                    self.camera.pitch = 0.0;
                                }
                                if ui
                                    .add(
                                        egui::Button::new("Top")
                                            .min_size(egui::vec2(52.0, interface::CONTROL_HEIGHT)),
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
                viewport::upload_count(frame),
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
    }
}

fn snapshot_camera(snapshot: &Snapshot, aspect: f32) -> viewport::Camera {
    assert!(aspect.is_finite());
    assert!(aspect > 0.0);
    let [minimum, maximum] = snapshot.bounds;
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
        assert_eq!(body.edge_segments()?.len(), 12);
        let mut mesh = body.mesh()?;
        mesh.smooth_normals();
        for vertex in &mesh.vertices {
            assert!((glam::Vec3::from_array(vertex.normal).abs().max_element() - 1.0).abs() < 1e-6);
        }
        let cylinder = kernel.cylinder([0.0; 3], 3.0, 8.0)?;
        let edges = cylinder.edge_segments()?;
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
        let changed = bracket.replace("= 100", "= 120");
        let cavity = "local a = cad.box({width=10, depth=10, height=10}); local b = cad.box({width=6, depth=6, height=6,x=2,y=2,z=2}); cad.show('hollow', cad.subtract(a,b))";
        for (index, source, dimensions) in [
            (0, bracket, [100.0, 50.0, 40.0]),
            (1, changed.as_str(), [120.0, 50.0, 40.0]),
            (2, cavity, [10.0; 3]),
        ] {
            let folder =
                std::env::temp_dir().join(format!("spice-cad-curved-{}", std::process::id()));
            let folder = folder.with_extension(index.to_string());
            export::model_files(source, &folder)?;
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
            assert_eq!(count as usize * 3, expected.mesh.vertices.len());
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
                    });
                }
            }
            let decoded = crate::mesh::Mesh {
                vertices: decoded,
                face_count: 1,
            };
            decoded.validate()?;
            assert!((decoded.volume_mm3() - expected.mesh.volume_mm3()).abs() < 0.01);
            assert!(export::model_files(source, &folder).is_err());
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
                for vertex in &snapshot.mesh.vertices {
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
            let error = export::model_files(source, &folder).expect_err("Unsafe export must fail");
            assert!(error.contains(message), "{error}");
            assert!(!folder.exists());
        }
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
        let body = model.build(&kernel)?;
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
    fn worker_bounds_error_messages_without_losing_runtime_locations() -> Result<(), String> {
        let worker = Worker::new(egui::Context::default())?;
        worker
            .commands
            .send(Command::Script {
                source: "--!strict\nerror(string.rep('λ', 100000))".into(),
                editor_revision: 1,
                revision: 1,
                saved_at: None,
            })
            .map_err(|error| error.to_string())?;
        let response = worker
            .results
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
                .commands
                .send(Command::Script {
                    source: source.into(),
                    editor_revision: revision,
                    revision,
                    saved_at: None,
                })
                .map_err(|error| error.to_string())?;
            let result = worker
                .results
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
            assert!(snapshot.mesh.face_count > 6);
            let layout = parameters.layout()?;
            let camera = viewport::Camera::rack(layout.dimensions_mm.to_array());
            let matrix = camera.matrix(1.5);
            assert!(snapshot.mesh.pick(matrix, [0.0, 0.0]).is_some());
            assert_eq!(snapshot.mesh.pick(matrix, [-0.99, 0.99]), None);
        }
        Ok(())
    }

    #[test]
    fn worker_recovers_after_invalid_request() -> Result<(), String> {
        let worker = Worker::new(egui::Context::default())?;
        worker
            .commands
            .send(Command::Build {
                parameters: RackParameters {
                    step_count: 0,
                    ..PARAMETERS
                },
                revision: 1,
            })
            .map_err(|error| error.to_string())?;
        assert!(
            worker
                .results
                .recv_timeout(Duration::from_secs(20))
                .map_err(|error| error.to_string())?
                .is_err()
        );
        worker
            .commands
            .send(Command::Build {
                parameters: PARAMETERS,
                revision: 2,
            })
            .map_err(|error| error.to_string())?;
        let result = worker
            .results
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
