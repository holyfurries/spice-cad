use super::{
    EVALUATION_COUNT_MAX, EdgeTargets, Geometry, Model, Modification, Operation, PART_COUNT_MAX,
    Recipe,
};
use crate::kernel::{Body, Kernel};
use std::rc::Rc;

struct Entry<'kernel> {
    recipe: Rc<Recipe>,
    solid: usize,
    steps: usize,
    body: Rc<Body<'kernel>>,
    geometry: Rc<Geometry>,
}

pub struct Prepared<'kernel> {
    pub bodies: Vec<Body<'kernel>>,
    pub geometry: Vec<Rc<Geometry>>,
    pub hits: usize,
    pub build_ms: f64,
    pub tessellation_ms: f64,
}

pub struct Cache<'kernel> {
    kernel: &'kernel Kernel,
    entries: Vec<Entry<'kernel>>,
    comparisons: Vec<(usize, usize)>,
}

impl<'kernel> Cache<'kernel> {
    pub fn new(kernel: &'kernel Kernel) -> Self {
        Self {
            kernel,
            entries: Vec::new(),
            comparisons: Vec::with_capacity(EVALUATION_COUNT_MAX),
        }
    }

    pub fn prepare(&mut self, model: &Model) -> Result<Prepared<'kernel>, String> {
        assert!(!model.parts.is_empty());
        assert!(model.parts.len() <= PART_COUNT_MAX);
        let mut next = Vec::with_capacity(model.parts.len());
        let mut prepared = Prepared {
            bodies: Vec::with_capacity(model.parts.len()),
            geometry: Vec::with_capacity(model.parts.len()),
            hits: 0,
            build_ms: 0.0,
            tessellation_ms: 0.0,
        };
        let mut steps = 0;
        let mut vertices = 0;
        let mut segments = 0;
        for part in &model.parts {
            let found = self.entries.iter().find(|entry| {
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
                Entry {
                    recipe: Rc::clone(&model.recipe),
                    solid: part.solid,
                    steps: entry.steps,
                    body: Rc::clone(&entry.body),
                    geometry: Rc::clone(&entry.geometry),
                }
            } else {
                let before = steps;
                let started = std::time::Instant::now();
                let mut body = model
                    .build_solid(self.kernel, part.solid, &mut steps)
                    .map_err(|error| format!("Part '{}': {error}", part.name))?;
                prepared.build_ms += started.elapsed().as_secs_f64() * 1000.0;
                let started = std::time::Instant::now();
                let mut mesh = body.mesh()?;
                mesh.smooth_normals();
                let geometry = Rc::new(Geometry {
                    mesh,
                    edges: body.edges()?,
                    volume_mm3: body.volume()?,
                });
                prepared.tessellation_ms += started.elapsed().as_secs_f64() * 1000.0;
                Entry {
                    recipe: Rc::clone(&model.recipe),
                    solid: part.solid,
                    steps: steps - before,
                    body: Rc::new(body),
                    geometry,
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
            let started = std::time::Instant::now();
            prepared.bodies.push(entry.body.try_clone()?);
            prepared.build_ms += started.elapsed().as_secs_f64() * 1000.0;
            prepared.geometry.push(Rc::clone(&entry.geometry));
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
            ) if b == d && first.profiles[a].points == second.profiles[c].points => {}
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
                "'renamed',cut,{color={r=1,g=0,b=0},assembly={position={x=50}},print={}}"
            )
        );
        let mut second = cache.prepare(&super::super::compile(&edited)?)?;
        assert_eq!(second.hits, 2);
        assert!(Rc::ptr_eq(&first.geometry[0], &second.geometry[0]));
        second.bodies[0].translate([100.0, 0.0, 0.0])?;
        second.bodies[0].step_data("working-copy")?;
        let next = cache.prepare(&super::super::compile(source)?)?;
        assert_eq!(next.hits, 2);
        assert!(next.bodies[0].bounds(true)?[0].abs().max_element() < 0.001);
        assert!((next.bodies[0].volume()? - first.geometry[0].volume_mm3).abs() < 0.001);
        let changed = cache.prepare(&super::super::compile(
            &source.replace("radius=2", "radius=3"),
        )?)?;
        assert_eq!(changed.hits, 1);
        assert!(!Rc::ptr_eq(&next.geometry[0], &changed.geometry[0]));
        assert!(Rc::ptr_eq(&next.geometry[1], &changed.geometry[1]));
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
