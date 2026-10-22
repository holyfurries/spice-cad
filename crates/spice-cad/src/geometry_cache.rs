use super::{
    EVALUATION_COUNT_MAX, EdgeTargets, Geometry, Model, Modification, Operation, PART_COUNT_MAX,
    Recipe,
};
use crate::kernel::{Body, Kernel};
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailMode {
    Deferred,
    Complete,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DisplayQuality {
    Interactive,
    Final,
}

impl DisplayQuality {
    fn mesh(self, body: &mut Body<'_>) -> Result<crate::kernel::Tessellation, String> {
        match self {
            Self::Interactive => body.game_mesh(),
            Self::Final => body.mesh(),
        }
    }
}

#[derive(Clone)]
pub enum Thumbnail {
    Pending,
    Ready(Option<Arc<eframe::egui::ColorImage>>),
}

impl Thumbnail {
    pub fn pending(&self) -> bool {
        matches!(self, Self::Pending)
    }

    pub fn image(&self) -> Option<Arc<eframe::egui::ColorImage>> {
        match self {
            Self::Pending => None,
            Self::Ready(image) => image.clone(),
        }
    }
}

#[derive(Clone)]
pub struct Presentation {
    orientation: crate::placement::Placement,
    color: [f32; 3],
    pub bounds: [glam::Vec3; 2],
    pub faces: std::collections::BTreeMap<u32, crate::inspection::Face>,
    pub thumbnail: Thumbnail,
}

struct Entry<'kernel> {
    recipe: Rc<Recipe>,
    solid: usize,
    steps: usize,
    body: Rc<Body<'kernel>>,
    geometry: Arc<Geometry>,
    quality: DisplayQuality,
    presentations: [Option<Arc<Presentation>>; 2],
    planes: Option<Arc<crate::dimensions::Planes>>,
}

const OPERATION_CACHE_COUNT_MAX: usize = 16;
const OPERATION_CACHE_BYTES_MAX: u64 = 16 * 1024 * 1024;

struct OperationEntry<'kernel> {
    recipe: Rc<Recipe>,
    solid: usize,
    steps: usize,
    body: Body<'kernel>,
    allocated_bytes: u64,
}

pub(super) struct Operations<'kernel> {
    entries: Vec<OperationEntry<'kernel>>,
    comparisons: Vec<(usize, usize)>,
    allocated_bytes: u64,
    hits: usize,
}

impl<'kernel> Operations<'kernel> {
    fn new() -> Self {
        Self {
            entries: Vec::with_capacity(OPERATION_CACHE_COUNT_MAX),
            comparisons: Vec::with_capacity(EVALUATION_COUNT_MAX),
            allocated_bytes: 0,
            hits: 0,
        }
    }

    pub(super) fn find(
        &mut self,
        recipe: &Recipe,
        solid: usize,
    ) -> Result<Option<(Body<'kernel>, usize)>, String> {
        let found = self.entries.iter().rposition(|entry| {
            cache_matches(
                recipe,
                solid,
                &entry.recipe,
                entry.solid,
                &mut self.comparisons,
            )
        });
        let Some(index) = found else {
            return Ok(None);
        };
        let body = self.entries[index].body.try_clone()?;
        let entry = self.entries.remove(index);
        let steps = entry.steps;
        self.entries.push(entry);
        self.hits += 1;
        Ok(Some((body, steps)))
    }

    pub(super) fn insert(
        &mut self,
        recipe: &Rc<Recipe>,
        solid: usize,
        steps: usize,
        body: &Body<'kernel>,
    ) -> Result<(), String> {
        assert!(steps > 0);
        assert!(steps <= EVALUATION_COUNT_MAX);
        let allocated_bytes = body.memory_allocated_bytes()?;
        if allocated_bytes == 0 || allocated_bytes > OPERATION_CACHE_BYTES_MAX {
            return Ok(());
        }
        while self.entries.len() >= OPERATION_CACHE_COUNT_MAX
            || self.allocated_bytes + allocated_bytes > OPERATION_CACHE_BYTES_MAX
        {
            self.allocated_bytes -= self.entries.remove(0).allocated_bytes;
        }
        let body = body.try_clone()?;
        let allocated_bytes = body.memory_allocated_bytes()?;
        if allocated_bytes == 0 || allocated_bytes > OPERATION_CACHE_BYTES_MAX {
            return Ok(());
        }
        while self.allocated_bytes + allocated_bytes > OPERATION_CACHE_BYTES_MAX {
            self.allocated_bytes -= self.entries.remove(0).allocated_bytes;
        }
        self.entries.push(OperationEntry {
            recipe: Rc::clone(recipe),
            solid,
            steps,
            body,
            allocated_bytes,
        });
        self.allocated_bytes += allocated_bytes;
        assert!(self.entries.len() <= OPERATION_CACHE_COUNT_MAX);
        assert!(self.allocated_bytes <= OPERATION_CACHE_BYTES_MAX);
        Ok(())
    }
}

pub struct Prepared<'kernel> {
    pub bodies: Vec<Rc<Body<'kernel>>>,
    pub geometry: Vec<Arc<Geometry>>,
    pub hits: usize,
    pub build_ms: f64,
    pub tessellation_ms: f64,
    pub refinement_pending: bool,
}

pub struct Cache<'kernel> {
    pub assembly_checks: crate::assembly_checks::Cache<'kernel>,
    kernel: &'kernel Kernel,
    preview: Option<crate::cancellation::Token>,
    pub thumbnail_renders: u64,
    entries: Vec<Entry<'kernel>>,
    operations: Operations<'kernel>,
    comparisons: Vec<(usize, usize)>,
    analysis: crate::analysis::Cache,
    display: Option<Arc<crate::viewport::Geometry>>,
}

impl<'kernel> Cache<'kernel> {
    pub fn new(kernel: &'kernel Kernel) -> Self {
        Self {
            assembly_checks: Default::default(),
            kernel,
            preview: None,
            thumbnail_renders: 0,
            entries: Vec::new(),
            operations: Operations::new(),
            comparisons: Vec::with_capacity(EVALUATION_COUNT_MAX),
            analysis: Default::default(),
            display: None,
        }
    }

    #[cfg(test)]
    pub fn compile(&mut self, source: &str) -> Result<Model, String> {
        self.checkpoint()?;
        super::compile_cached(source, &mut self.analysis)
    }

    pub fn compile_project(
        &mut self,
        source: &str,
        project: Option<&crate::project::Project>,
    ) -> Result<Model, String> {
        self.checkpoint()?;
        super::compile_project_cached(source, project, &mut self.analysis)
    }

    pub fn with_preview<T>(
        &mut self,
        token: crate::cancellation::Token,
        build: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<Option<T>, String> {
        assert!(self.preview.is_none());
        self.preview = Some(token);
        let result = build(self);
        let token = self.preview.take().expect("Active preview token");
        if token.cancelled() {
            return Ok(None);
        }
        result.map(Some)
    }

    pub fn checkpoint(&self) -> Result<(), String> {
        if let Some(token) = &self.preview {
            token.checkpoint()?;
        }
        Ok(())
    }

    pub fn display(
        &mut self,
        geometry: &[Arc<Geometry>],
    ) -> Result<Arc<crate::viewport::Geometry>, String> {
        self.checkpoint()?;
        if let Some(display) = &self.display {
            if display.parts.len() == geometry.len()
                && display
                    .parts
                    .iter()
                    .zip(geometry)
                    .all(|(a, b)| Arc::ptr_eq(&a.geometry, b))
            {
                return Ok(Arc::clone(display));
            }
        }
        let display = Arc::new(crate::viewport::Geometry::new(geometry)?);
        self.display = Some(Arc::clone(&display));
        Ok(display)
    }

    pub fn annotations(&mut self, model: &Model) -> Result<crate::dimensions::Annotations, String> {
        self.checkpoint()?;
        assert_eq!(self.entries.len(), model.parts.len());
        if !model.has_annotations() {
            return Ok(Default::default());
        }
        let mut first = 1;
        let outputs: Vec<_> = self
            .entries
            .iter_mut()
            .zip(&model.parts)
            .map(|(entry, part)| {
                let planes = entry.planes.get_or_insert_with(|| {
                    Arc::new(crate::dimensions::planes(&entry.geometry.mesh))
                });
                let faces = first..=first + entry.geometry.mesh.face_count - 1;
                first += entry.geometry.mesh.face_count;
                (part.solid, faces, part.assembly.matrix(), &**planes)
            })
            .collect();
        model.definitions.resolve(&outputs)
    }

    pub fn presentations(
        &mut self,
        model: &Model,
        layout: crate::placement::Layout,
        thumbnails: ThumbnailMode,
    ) -> Result<Vec<Arc<Presentation>>, String> {
        assert_eq!(self.entries.len(), model.parts.len());
        assert!(model.parts.len() <= PART_COUNT_MAX);
        let slot = match layout {
            crate::placement::Layout::Assembly => 0,
            crate::placement::Layout::Print => 1,
        };
        let mut remaining = crate::thumbnail::PIXEL_VISITS_MAX;
        let mut result = Vec::with_capacity(model.parts.len());
        for (entry, part) in self.entries.iter_mut().zip(&model.parts) {
            if let Some(token) = &self.preview {
                token.checkpoint()?;
            }
            let orientation = part.placement(layout).orientation();
            let cached = &mut entry.presentations[slot];
            if cached.as_ref().is_none_or(|previous| {
                previous.orientation != orientation || previous.color != part.material.base_color
            }) {
                let bounds = if orientation == crate::placement::Placement::default() {
                    entry.geometry.bounds
                } else {
                    let mut body = entry.body.try_clone()?;
                    orientation.apply(&mut body, false)?;
                    body.bounds(true)?.map(|point| point.as_vec3())
                };
                let mesh = entry
                    .geometry
                    .mesh
                    .placed(orientation.matrix(), part.material.base_color);
                let faces = crate::inspection::faces(&mesh)?;
                let thumbnail = if thumbnails == ThumbnailMode::Complete {
                    self.thumbnail_renders += 1;
                    Thumbnail::Ready(
                        crate::thumbnail::render(&mesh, bounds, &mut remaining).map(Arc::new),
                    )
                } else {
                    Thumbnail::Pending
                };
                *cached = Some(Arc::new(Presentation {
                    orientation,
                    color: part.material.base_color,
                    bounds,
                    faces,
                    thumbnail,
                }));
            }
            let presentation = cached.as_mut().expect("Prepared presentation");
            if thumbnails == ThumbnailMode::Complete && presentation.thumbnail.pending() {
                let mesh = entry
                    .geometry
                    .mesh
                    .placed(orientation.matrix(), part.material.base_color);
                self.thumbnail_renders += 1;
                let image = crate::thumbnail::render(&mesh, presentation.bounds, &mut remaining)
                    .map(Arc::new);
                Arc::make_mut(presentation).thumbnail = Thumbnail::Ready(image);
            }
            result.push(Arc::clone(presentation));
        }
        Ok(result)
    }

    pub fn prepare(&mut self, model: &Model) -> Result<Prepared<'kernel>, String> {
        self.prepare_quality(model, DisplayQuality::Final)
    }

    pub fn prepare_quality(
        &mut self,
        model: &Model,
        quality: DisplayQuality,
    ) -> Result<Prepared<'kernel>, String> {
        assert!(!model.parts.is_empty());
        assert!(model.parts.len() <= PART_COUNT_MAX);
        let mut next: Vec<Entry<'kernel>> = Vec::with_capacity(model.parts.len());
        let mut prepared = Prepared {
            bodies: Vec::with_capacity(model.parts.len()),
            geometry: Vec::with_capacity(model.parts.len()),
            hits: 0,
            build_ms: 0.0,
            tessellation_ms: 0.0,
            refinement_pending: false,
        };
        let mut steps = 0;
        let mut vertices = 0;
        let mut segments = 0;
        for part in &model.parts {
            self.checkpoint()?;
            let found = self.entries.iter().chain(next.iter()).find(|entry| {
                cache_matches(
                    &model.recipe,
                    part.solid,
                    &entry.recipe,
                    entry.solid,
                    &mut self.comparisons,
                )
            });
            let entry = if let Some(entry) = found {
                prepared.hits += 1;
                steps += entry.steps;
                if steps > EVALUATION_COUNT_MAX {
                    return Err(
                        "Model exceeds 512 evaluation steps; reduce repeated subtrees".into(),
                    );
                }
                let mut reused = Entry {
                    recipe: Rc::clone(&model.recipe),
                    solid: part.solid,
                    steps: entry.steps,
                    body: Rc::clone(&entry.body),
                    geometry: Arc::clone(&entry.geometry),
                    quality: entry.quality,
                    presentations: entry.presentations.clone(),
                    planes: entry.planes.clone(),
                };
                if reused.quality < quality {
                    self.checkpoint()?;
                    let started = std::time::Instant::now();
                    let mut body = reused.body.try_clone()?;
                    let crate::kernel::Tessellation {
                        mut mesh,
                        volume_mm3,
                    } = quality.mesh(&mut body)?;
                    self.checkpoint()?;
                    mesh.smooth_normals();
                    reused.geometry = Arc::new(Geometry {
                        mesh,
                        edges: body.edges()?,
                        volume_mm3,
                        bounds: reused.geometry.bounds,
                    });
                    reused.quality = quality;
                    reused.presentations = Default::default();
                    reused.planes = None;
                    prepared.tessellation_ms += started.elapsed().as_secs_f64() * 1000.0;
                }
                reused
            } else {
                let before = steps;
                let started = std::time::Instant::now();
                let mut body = model
                    .build_solid_cached(
                        self.kernel,
                        part.solid,
                        &mut steps,
                        Some(&mut self.operations),
                        self.preview.as_ref(),
                    )
                    .map_err(|error| format!("Part '{}': {error}", part.name))?;
                prepared.build_ms += started.elapsed().as_secs_f64() * 1000.0;
                self.checkpoint()?;
                let started = std::time::Instant::now();
                let crate::kernel::Tessellation {
                    mut mesh,
                    volume_mm3,
                } = quality.mesh(&mut body)?;
                self.checkpoint()?;
                mesh.smooth_normals();
                let geometry = Arc::new(Geometry {
                    mesh,
                    edges: body.edges()?,
                    volume_mm3,
                    bounds: body.bounds(true)?.map(|point| point.as_vec3()),
                });
                prepared.tessellation_ms += started.elapsed().as_secs_f64() * 1000.0;
                Entry {
                    recipe: Rc::clone(&model.recipe),
                    solid: part.solid,
                    steps: steps - before,
                    body: Rc::new(body),
                    geometry,
                    quality,
                    presentations: Default::default(),
                    planes: None,
                }
            };
            vertices += entry.geometry.mesh.vertices.len();
            if vertices > 300000 {
                return Err("Assembly exceeds its 100000 triangle budget".into());
            }
            segments += entry.geometry.edges.segments.len();
            if segments > 131072 {
                return Err("Assembly exceeds its display edge budget".into());
            }
            prepared.bodies.push(Rc::clone(&entry.body));
            prepared.refinement_pending |= entry.quality == DisplayQuality::Interactive;
            prepared.geometry.push(Arc::clone(&entry.geometry));
            next.push(entry);
        }
        self.entries = next;
        assert_eq!(prepared.bodies.len(), model.parts.len());
        assert!(prepared.hits <= model.parts.len());
        Ok(prepared)
    }
}

fn cache_matches(
    first: &Recipe,
    first_solid: usize,
    second: &Recipe,
    second_solid: usize,
    pending: &mut Vec<(usize, usize)>,
) -> bool {
    pending.clear();
    pending.push((first_solid, second_solid));
    let mut steps = 0;
    while let Some((a, b)) = pending.pop() {
        steps += 1;
        if steps > EVALUATION_COUNT_MAX {
            return false;
        }
        match (first.operations[a], second.operations[b]) {
            (Operation::Shape(a), Operation::Shape(b)) if first.shapes[a] == second.shapes[b] => {}
            (
                Operation::Box {
                    minimum: a,
                    maximum: b,
                },
                Operation::Box {
                    minimum: c,
                    maximum: d,
                },
            ) if a == c && b == d => {}
            (
                Operation::Cylinder {
                    origin: a,
                    radius: b,
                    height: c,
                },
                Operation::Cylinder {
                    origin: d,
                    radius: e,
                    height: f,
                },
            ) if a == d && b == e && c == f => {}
            (
                Operation::Profile {
                    profile: a,
                    sweep: b,
                },
                Operation::Profile {
                    profile: c,
                    sweep: d,
                },
            ) if b == d && first.profiles[a] == second.profiles[c] => {}
            (
                Operation::Boolean {
                    first: a,
                    second: b,
                    subtract: c,
                },
                Operation::Boolean {
                    first: d,
                    second: e,
                    subtract: f,
                },
            ) if c == f => {
                pending.extend([(a, d), (b, e)]);
            }
            (
                Operation::Modify {
                    source: a,
                    modification: b,
                },
                Operation::Modify {
                    source: c,
                    modification: d,
                },
            ) => {
                match (b, d) {
                    (Modification::Shell(a), Modification::Shell(b)) => {
                        let a = &first.shells[a];
                        let b = &second.shells[b];
                        if a.thickness != b.thickness {
                            return false;
                        }
                        match (&a.opening, &b.opening) {
                            (None, None) => {}
                            (Some(a), Some(b)) if a.geometry_matches(b) => {}
                            _ => return false,
                        }
                    }
                    (Modification::FaceFeature(a), Modification::FaceFeature(b)) => {
                        let a = &first.face_features[a];
                        let b = &second.face_features[b];
                        if !a.face.geometry_matches(&b.face)
                            || a.offset != b.offset
                            || a.shape != b.shape
                            || a.depth != b.depth
                            || a.kind != b.kind
                        {
                            return false;
                        }
                    }
                    (Modification::Hole(a), Modification::Hole(b)) => {
                        let a = &first.holes[a];
                        let b = &second.holes[b];
                        if !a.face.geometry_matches(&b.face)
                            || a.offset != b.offset
                            || a.radius != b.radius
                            || a.depth != b.depth
                            || a.recess != b.recess
                        {
                            return false;
                        }
                    }
                    (Modification::Translate(a), Modification::Translate(b)) if a == b => {}
                    (
                        Modification::Rotate { axis: a, angle: b },
                        Modification::Rotate { axis: c, angle: d },
                    ) if a == c && b == d => {}
                    (Modification::EdgeTreatment(a), Modification::EdgeTreatment(b)) => {
                        let a = &first.edge_treatments[a];
                        let b = &second.edge_treatments[b];
                        if a.treatment != b.treatment {
                            return false;
                        }
                        match (&a.edges, &b.edges) {
                            (EdgeTargets::All, EdgeTargets::All) => {}
                            (EdgeTargets::Selected(a), EdgeTargets::Selected(b))
                                if a.len() == b.len() =>
                            {
                                if !a
                                    .iter()
                                    .zip(b)
                                    .all(|(a, b)| first.edges[*a].points == second.edges[*b].points)
                                {
                                    return false;
                                }
                            }
                            _ => return false,
                        }
                    }
                    _ => return false,
                }
                pending.push((a, c));
            }
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_refinement_preserves_interactive_geometry_and_recovers() -> Result<(), String> {
        let kernel = Kernel::new()?;
        let mut cache = Cache::new(&kernel);
        let model = super::super::compile("cad.show('part',cad.cylinder({radius=5,height=10}))")?;
        let interactive = cache.prepare_quality(&model, DisplayQuality::Interactive)?;
        let geometry = Arc::clone(&interactive.geometry[0]);
        let body = Rc::clone(&interactive.bodies[0]);
        drop(interactive);
        let generation = crate::cancellation::Generation::default();
        let cancelled = cache
            .with_preview(generation.token().after_checkpoints(2), |cache| {
                cache.prepare(&model)
            })?
            .is_none();
        let geometry_preserved = Arc::ptr_eq(&geometry, &cache.entries[0].geometry);
        let quality_preserved = cache.entries[0].quality == DisplayQuality::Interactive;
        let scope_cleared = cache.preview.is_none();
        let refined = cache.prepare(&model)?;
        let body_reused = Rc::ptr_eq(&body, &refined.bodies[0]);
        let refined_without_rebuild = refined.build_ms == 0.0 && !refined.refinement_pending;
        drop(refined);
        drop(body);
        drop(cache);
        drop(kernel);
        assert!(cancelled);
        assert!(geometry_preserved);
        assert!(quality_preserved);
        assert!(scope_cleared);
        assert!(body_reused);
        assert!(refined_without_rebuild);
        Ok(())
    }

    #[test]
    fn cancellation_keeps_completed_predecessors_and_clears_preview_scope() -> Result<(), String> {
        let kernel = Kernel::new()?;
        let mut cache = Cache::new(&kernel);
        let source = "local p=cad.box({width=20,depth=20,height=10});local q=cad.translate(p,{x=2});cad.show('part',cad.translate(q,{y=2}))";
        let model = super::super::compile(source)?;
        let generation = crate::cancellation::Generation::default();
        let token = generation.token().after_checkpoints(4);
        let cancelled = cache
            .with_preview(token, |cache| cache.prepare(&model))?
            .is_none();
        let retained = cache.operations.entries.len();
        let hits_before = cache.operations.hits;
        let recovered = cache.with_preview(generation.token(), |cache| cache.prepare(&model))?;
        let recovery_hits = cache.operations.hits - hits_before;
        let recovered = recovered.is_some();
        generation.advance();
        let export_model = cache.compile(source)?;
        let exported_bodies = cache.prepare(&export_model)?.bodies.len();
        let scope_cleared = cache.preview.is_none();
        drop(cache);
        drop(kernel);
        assert!(cancelled);
        assert_eq!(retained, 1);
        assert!(recovered);
        assert_eq!(recovery_hits, 1);
        assert!(scope_cleared);
        assert_eq!(exported_bodies, 1);
        Ok(())
    }

    #[test]
    fn cancellation_before_meshing_preserves_all_completed_predecessors() -> Result<(), String> {
        let kernel = Kernel::new()?;
        let mut cache = Cache::new(&kernel);
        let model = super::super::compile(
            "local p=cad.box({width=20,depth=20,height=10});local q=cad.translate(p,{x=2});cad.show('part',cad.translate(q,{y=2}))",
        )?;
        let generation = crate::cancellation::Generation::default();
        let cancelled = cache
            .with_preview(generation.token().after_checkpoints(6), |cache| {
                cache.prepare(&model)
            })?
            .is_none();
        let retained = cache.operations.entries.len();
        let completed_parts = cache.entries.len();
        drop(cache);
        drop(kernel);
        assert!(cancelled);
        assert_eq!(retained, 2);
        assert_eq!(completed_parts, 0);
        Ok(())
    }

    #[test]
    fn later_hole_edits_reuse_the_filleted_body_without_mutating_it() -> Result<(), String> {
        let kernel = Kernel::new()?;
        let mut cache = Cache::new(&kernel);
        let source = "local p=cad.fillet(cad.box({width=20,depth=20,height=10}),{radius=1});local f=cad.face(p,{name='top',normal='z+'});cad.show('part',cad.hole(p,{face=f,radius=2,depth=4}))";
        let first = cache.prepare(&super::super::compile(source)?)?;
        let hits_before = cache.operations.hits;
        let edited = source.replace("radius=2", "radius=3");
        let second_model = super::super::compile(&edited)?;
        let second = cache.prepare(&second_model)?;
        let hits = cache.operations.hits - hits_before;
        let mut direct = second_model.build(&kernel)?;
        let expected_volume = direct[0].mesh()?.volume_mm3;
        let restored = cache.prepare(&super::super::compile(source)?)?;
        let first_volume = first.geometry[0].volume_mm3;
        let second_volume = second.geometry[0].volume_mm3;
        let restored_volume = restored.geometry[0].volume_mm3;
        let final_hits = second.hits;
        let hits_before = cache.operations.hits;
        let changed = cache.prepare(&super::super::compile(
            &source.replace("width=20", "width=30"),
        )?)?;
        let upstream_hits = cache.operations.hits - hits_before;
        let changed_width = changed.geometry[0].bounds[1].x;
        drop(changed);
        drop(direct);
        drop(restored);
        drop(second);
        drop(first);
        drop(cache);
        drop(kernel);
        assert_eq!(hits, 1);
        assert_eq!(final_hits, 0);
        assert!((second_volume - expected_volume).abs() < 1e-6);
        assert!(second_volume < first_volume);
        assert!((restored_volume - first_volume).abs() < 1e-6);
        assert_eq!(upstream_hits, 0);
        assert!((changed_width - 30.0).abs() < 0.001);
        Ok(())
    }

    #[test]
    fn intermediate_cache_preserves_logical_steps_and_retention_limits() -> Result<(), String> {
        let kernel = Kernel::new()?;
        let mut operations = Operations::new();
        let source = "local p=cad.box({width=20,depth=20,height=10});local q=cad.translate(p,{x=2});cad.show('part',cad.translate(q,{y=2}))";
        let model = super::super::compile(source)?;
        let solid = model.parts[0].solid;
        let mut first_steps = 0;
        model.build_solid_cached(
            &kernel,
            solid,
            &mut first_steps,
            Some(&mut operations),
            None,
        )?;
        let mut cached_steps = 0;
        model.build_solid_cached(
            &kernel,
            solid,
            &mut cached_steps,
            Some(&mut operations),
            None,
        )?;
        let mut near_limit = EVALUATION_COUNT_MAX - first_steps + 1;
        let exhausted = model
            .build_solid_cached(&kernel, solid, &mut near_limit, Some(&mut operations), None)
            .is_err();
        for width in 1..=40 {
            let model = super::super::compile(&format!(
                "local p=cad.box({{width={width},depth=2,height=3}});cad.show('part',cad.translate(p,{{x=1}}))"
            ))?;
            model.build_solid_cached(
                &kernel,
                model.parts[0].solid,
                &mut 0,
                Some(&mut operations),
                None,
            )?;
        }
        let count = operations.entries.len();
        let allocated_bytes = operations.allocated_bytes;
        let measured_bytes: u64 = operations
            .entries
            .iter()
            .map(|entry| entry.allocated_bytes)
            .sum();
        drop(operations);
        drop(kernel);
        assert_eq!(first_steps, 5);
        assert_eq!(cached_steps, first_steps);
        assert!(exhausted);
        assert_eq!(count, OPERATION_CACHE_COUNT_MAX);
        assert!(allocated_bytes <= OPERATION_CACHE_BYTES_MAX);
        assert!(allocated_bytes > 0);
        assert_eq!(allocated_bytes, measured_bytes);
        Ok(())
    }

    #[test]
    fn interactive_geometry_refines_without_rebuilding_and_never_downgrades() -> Result<(), String>
    {
        let kernel = Kernel::new()?;
        let model = super::super::compile(include_str!("../../../examples/mounting-bracket.luau"))?;
        let mut cache = Cache::new(&kernel);
        let interactive = cache.prepare_quality(&model, DisplayQuality::Interactive)?;
        let repeated = cache.prepare_quality(&model, DisplayQuality::Interactive)?;
        let refined = cache.prepare(&model)?;
        let final_repeated = cache.prepare(&model)?;
        let dragging = cache.prepare_quality(&model, DisplayQuality::Interactive)?;
        let observations = [
            (
                "canonical body reused",
                Rc::ptr_eq(&interactive.bodies[0], &refined.bodies[0]),
            ),
            ("interactive pending", interactive.refinement_pending),
            ("repeated interactive pending", repeated.refinement_pending),
            ("refined final", !refined.refinement_pending),
            ("no downgrade", !dragging.refinement_pending),
            ("no CAD rebuild", refined.build_ms == 0.0),
            ("cached body hit", refined.hits == 1),
            (
                "interactive cache hit",
                Arc::ptr_eq(&interactive.geometry[0], &repeated.geometry[0]),
            ),
            (
                "new final mesh",
                !Arc::ptr_eq(&interactive.geometry[0], &refined.geometry[0]),
            ),
            (
                "final cache hit",
                Arc::ptr_eq(&refined.geometry[0], &final_repeated.geometry[0]),
            ),
            (
                "drag reuses final mesh",
                Arc::ptr_eq(&refined.geometry[0], &dragging.geometry[0]),
            ),
            (
                "fewer interactive triangles",
                interactive.geometry[0].mesh.vertices.len()
                    < refined.geometry[0].mesh.vertices.len(),
            ),
            (
                "same face count",
                interactive.geometry[0].mesh.face_count == refined.geometry[0].mesh.face_count,
            ),
            (
                "same exact bounds",
                interactive.geometry[0].bounds == refined.geometry[0].bounds,
            ),
            (
                "same exact volume",
                interactive.geometry[0].volume_mm3 == refined.geometry[0].volume_mm3,
            ),
        ];
        drop(cache);
        drop(dragging);
        drop(final_repeated);
        drop(refined);
        drop(repeated);
        drop(interactive);
        drop(kernel);
        for (name, valid) in observations {
            assert!(valid, "{name}");
        }
        Ok(())
    }

    #[test]
    #[ignore = "Manual interactive tessellation comparison; run without concurrent builds"]
    fn profile_interactive_tessellation() -> Result<(), String> {
        for (name, source) in [
            (
                "bracket",
                include_str!("../../../examples/mounting-bracket.luau"),
            ),
            ("rack", include_str!("../../../examples/rack-parts.luau")),
            (
                "rocket",
                include_str!("../../../examples/rocket-engine.luau"),
            ),
        ] {
            let kernel = Kernel::new()?;
            let model = super::super::compile(source)?;
            for sample in 0..5 {
                let mut bodies = model.build(&kernel)?;
                for body in &mut bodies {
                    body.volume()?;
                    for coarse in [sample % 2 == 0, sample % 2 != 0] {
                        let started = std::time::Instant::now();
                        let mut mesh = if coarse {
                            body.game_mesh()?
                        } else {
                            body.mesh()?
                        }
                        .mesh;
                        mesh.smooth_normals();
                        eprintln!(
                            "{name} sample {sample} coarse {coarse}: {:.3} ms, {} triangles, {} faces",
                            started.elapsed().as_secs_f64() * 1000.0,
                            mesh.vertices.len() / 3,
                            mesh.face_count
                        );
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    #[ignore = "Manual display-geometry stage timings and Rust allocation counts"]
    fn profile_display_geometry_stages() -> Result<(), String> {
        fn stage<T>(name: &str, run: impl FnOnce() -> T) -> T {
            let started = std::time::Instant::now();
            let (result, allocations) = crate::allocations::measure(run);
            eprintln!(
                "{name}: {:.3} ms, {allocations:?}",
                started.elapsed().as_secs_f64() * 1000.0
            );
            result
        }
        for (name, source) in [
            (
                "bracket",
                include_str!("../../../examples/mounting-bracket.luau"),
            ),
            ("rack", include_str!("../../../examples/rack-parts.luau")),
            (
                "rocket",
                include_str!("../../../examples/rocket-engine.luau"),
            ),
        ] {
            let kernel = Kernel::new()?;
            let model = super::super::compile(source)?;
            for sample in 0..3 {
                let mut bodies = model.build(&kernel)?;
                eprintln!("{name} sample {sample}, {} bodies", bodies.len());
                for body in &mut bodies {
                    let crate::kernel::Tessellation {
                        mut mesh,
                        volume_mm3,
                    } = stage("mesh and exact volume", || body.mesh())?;
                    stage("mesh validation audit", || mesh.validate())?;
                    stage("normals", || mesh.smooth_normals());
                    let edges = stage("edges", || body.edges())?;
                    let bounds = stage("bounds", || body.bounds(true))?;
                    assert!(volume_mm3 > 0.0);
                    assert!(bounds[0].cmple(bounds[1]).all());
                    eprintln!(
                        "{} triangles, {} edge segments, {volume_mm3:.3} mm³",
                        mesh.vertices.len() / 3,
                        edges.segments.len()
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn tessellation_allocations_stay_within_the_curved_bracket_budget() -> Result<(), String> {
        let kernel = Kernel::new()?;
        let model = super::super::compile(include_str!("../../../examples/mounting-bracket.luau"))?;
        let mut bodies = model.build(&kernel)?;
        assert_eq!(bodies.len(), 1);
        let (tessellation, allocations) = crate::allocations::measure(|| bodies[0].mesh());
        let tessellation = tessellation?;
        drop(bodies);
        drop(kernel);
        let vertices = tessellation.mesh.vertices.len();
        assert_eq!(vertices / 3, 2564);
        assert!(allocations.calls < (vertices / 6) as u64, "{allocations:?}");
        let vertex_size_bytes = std::mem::size_of::<crate::mesh::Vertex>();
        assert!(
            allocations.bytes <= (vertices * vertex_size_bytes * 3) as u64,
            "{allocations:?}"
        );
        assert!((tessellation.volume_mm3 - 49560.026).abs() < 0.001);
        Ok(())
    }

    #[test]
    fn closed_mesh_validation_keeps_compact_allocation_and_memory_budgets() -> Result<(), String> {
        let kernel = Kernel::new()?;
        let model = super::super::compile(include_str!("../../../examples/mounting-bracket.luau"))?;
        let mut bodies = model.build(&kernel)?;
        let mesh = bodies[0].mesh()?.mesh;
        drop(bodies);
        drop(kernel);
        let (result, allocations) = crate::allocations::measure(|| mesh.validate());
        result?;
        assert!(allocations.calls <= 2, "{allocations:?}");
        assert!(
            allocations.bytes < mesh.vertices.len() as u64 * 20,
            "{allocations:?}"
        );
        Ok(())
    }

    #[test]
    fn cached_annotation_allocations_do_not_scale_with_mesh_density() -> Result<(), String> {
        fn run(radius: u32) -> Result<(usize, crate::allocations::Counts), String> {
            let kernel = Kernel::new()?;
            let mut cache = Cache::new(&kernel);
            let source = format!(
                "local part=cad.cylinder({{radius={radius},height=20}});local a=cad.face(part,{{name='bottom',normal='z-'}});local b=cad.face(part,{{name='top',normal='z+'}});cad.measure('height',{{from=a,to=b}});cad.show('part',part)"
            );
            let model = cache.compile(&source)?;
            let prepared = cache.prepare(&model)?;
            cache.annotations(&model)?;
            let previous = Arc::clone(
                cache.entries[0]
                    .planes
                    .as_ref()
                    .ok_or("Missing plane cache")?,
            );
            cache.prepare(&model)?;
            assert!(Arc::ptr_eq(
                &previous,
                cache.entries[0]
                    .planes
                    .as_ref()
                    .ok_or("Missing reused planes")?
            ));
            let (annotations, counts) = crate::allocations::measure(|| cache.annotations(&model));
            assert!((annotations?.measurements[0].distance_mm - 20.0).abs() < 0.001);
            Ok((prepared.geometry[0].mesh.vertices.len(), counts))
        }
        let (coarse, first) = run(1)?;
        let (fine, second) = run(1000)?;
        assert!(fine > coarse * 4);
        assert_eq!(first, second);
        eprintln!(
            "Cached annotations: {coarse}/{fine} vertices, {} allocations and {} requested bytes in either case",
            first.calls, first.bytes
        );
        Ok(())
    }

    #[test]
    fn cache_reuses_dependencies_ignores_metadata_and_keeps_canonical_bodies() -> Result<(), String>
    {
        let kernel = Kernel::new()?;
        let mut cache = Cache::new(&kernel);
        let source = "local a=cad.box({width=10,depth=10,height=2});local hole=cad.cylinder({radius=2,height=2,x=5,y=5});local cut=cad.subtract(a,hole);cad.show('plate',cut,{print={}});cad.show('other',cad.box({width=3,depth=4,height=5}),{print={position={x=20}}})";
        let first = cache.prepare(&super::super::compile(source)?)?;
        assert_eq!(first.hits, 0);
        let edited = format!(
            "-- comment\nlocal unused=cad.box({{width=1,depth=1,height=1}});{}",
            source.replace(
                "'plate',cut,{print={}}",
                "'renamed',cut,{material = cad.material('Surface', {base_color = {r=1,g=0,b=0}}),assembly={position={x=50}},print={}}"
            )
        );
        let second = cache.prepare(&super::super::compile(&edited)?)?;
        assert_eq!(second.hits, 2);
        assert!(Arc::ptr_eq(&first.geometry[0], &second.geometry[0]));
        let mut working = second.bodies[0].try_clone()?;
        working.translate([100.0, 0.0, 0.0])?;
        working.step_data("working-copy")?;
        let next = cache.prepare(&super::super::compile(source)?)?;
        assert_eq!(next.hits, 2);
        assert!(next.bodies[0].bounds(true)?[0].abs().max_element() < 0.001);
        assert!((next.bodies[0].volume()? - first.geometry[0].volume_mm3).abs() < 0.001);
        let changed = cache.prepare(&super::super::compile(
            &source.replace("radius=2", "radius=3"),
        )?)?;
        assert_eq!(changed.hits, 1);
        assert!(!Arc::ptr_eq(&next.geometry[0], &changed.geometry[0]));
        assert!(Arc::ptr_eq(&next.geometry[1], &changed.geometry[1]));
        let invalid = super::super::compile(
            "local a=cad.box({width=1,depth=1,height=1});cad.show('empty',cad.subtract(a,a))",
        )?;
        assert!(cache.prepare(&invalid).is_err());
        assert_eq!(cache.entries.len(), 2);
        let reduced =
            super::super::compile("cad.show('other',cad.box({width=3,depth=4,height=5}))")?;
        assert_eq!(cache.prepare(&reduced)?.hits, 1);
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.prepare(&super::super::compile(source)?)?.hits, 1);
        Ok(())
    }

    #[test]
    fn cache_keys_include_profiles_transforms_booleans_and_edge_targets() -> Result<(), String> {
        let mut pending = Vec::new();
        for (source, from, to, equal) in [
            (
                "cad.show('part',cad.box({width=10,depth=2,height=3}))",
                "width=10",
                "width=11",
                false,
            ),
            (
                "cad.show('part',cad.cylinder({radius=2,height=3}))",
                "radius=2",
                "radius=3",
                false,
            ),
            (
                "cad.show('part',cad.extrude(cad.circle({radius=2}),{height=3}))",
                "radius=2",
                "radius=3",
                false,
            ),
            (
                "local p=cad.profile({loops={cad.circle({radius=10}),cad.circle({radius=2})}});cad.show('part',cad.extrude(p,{height=3}))",
                "radius=2",
                "radius=3",
                false,
            ),
            (
                "local p=cad.path({start={x=0,y=-5},segments={{to={x=20,y=-5}},{to={x=20,y=5},center={x=20,y=0}},{to={x=0,y=5}},{to={x=0,y=-5},center={x=0,y=0}}}});cad.show('part',cad.extrude(p,{height=3}))",
                "20",
                "25",
                false,
            ),
            (
                "cad.show('part',cad.translate(cad.box({width=10,depth=2,height=3}),{x=1}))",
                "x=1",
                "x=2",
                false,
            ),
            (
                "cad.show('part',cad.rotate(cad.box({width=10,depth=2,height=3}),{axis='z',angle=30}))",
                "angle=30",
                "angle=40",
                false,
            ),
            (
                "cad.show('part',cad.subtract(cad.box({width=10,depth=2,height=3}),cad.box({width=2,depth=2,height=3})))",
                "cad.subtract",
                "cad.union",
                false,
            ),
            (
                "local p=cad.polygon({{x=0,y=0},{x=10,y=0},{x=0,y=10}});cad.show('part',cad.extrude(p,{height=3}))",
                "x=10",
                "x=11",
                false,
            ),
            (
                "local p=cad.polygon({{x=0,y=0},{x=10,y=0},{x=0,y=10}});cad.show('part',cad.extrude(p,{height=3}))",
                "height=3",
                "height=4",
                false,
            ),
            (
                "local p=cad.polygon({{x=1,y=0},{x=10,y=0},{x=1,y=10}});cad.show('part',cad.revolve(p,{angle=180}))",
                "angle=180",
                "angle=270",
                false,
            ),
            (
                "cad.show('part',cad.fillet(cad.box({width=10,depth=10,height=10}),{radius=1}))",
                "radius=1",
                "radius=2",
                false,
            ),
            (
                "local p=cad.box({width=10,depth=10,height=10});local e=cad.edge(p,{name='edge',from={x=0,y=0,z=0},to={x=10,y=0,z=0}});cad.show('part',cad.fillet(p,{radius=1,edges={e}}))",
                "'edge'",
                "'renamed'",
                true,
            ),
            (
                "local p=cad.box({width=10,depth=10,height=10});local e=cad.edge(p,{name='edge',from={x=0,y=0,z=0},to={x=10,y=0,z=0}});cad.show('part',cad.fillet(p,{radius=1,edges={e}}))",
                "to={x=10,y=0,z=0}",
                "to={x=0,y=10,z=0}",
                false,
            ),
            (
                "local p=cad.box({width=10,depth=10,height=10});local e=cad.edge(p,{name='edge',from={x=0,y=0,z=0},to={x=10,y=0,z=0}});cad.show('part',cad.fillet(p,{radius=1,edges={e}}))",
                ",edges={e}",
                "",
                false,
            ),
        ] {
            let first = super::super::compile(source)?;
            let second = super::super::compile(&source.replace(from, to))?;
            assert_eq!(
                cache_matches(
                    &first.recipe,
                    first.parts[0].solid,
                    &second.recipe,
                    second.parts[0].solid,
                    &mut pending
                ),
                equal,
                "{from} -> {to}"
            );
        }
        Ok(())
    }
}
