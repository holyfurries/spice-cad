use crate::{
    kernel::Body,
    model::Model,
    placement::{Layout, Placement},
};
use std::rc::{Rc, Weak};

pub const VOLUME_TOLERANCE_MM3: f64 = 1e-6;
const PAIR_COUNT_MAX: usize = 32 * 31 / 2;

#[derive(Clone)]
pub struct Overlap {
    pub parts: [usize; 2],
    pub volume_mm3: f64,
}

#[derive(Clone)]
pub struct Failure {
    pub parts: [usize; 2],
    pub message: String,
}

#[derive(Clone, Default)]
pub struct Report {
    pub pairs: usize,
    pub boolean_tests: usize,
    pub cache_hits: usize,
    pub overlaps: Vec<Overlap>,
    pub failures: Vec<Failure>,
}

struct Entry<'kernel> {
    bodies: [Weak<Body<'kernel>>; 2],
    placements: [Placement; 2],
    volume_mm3: f64,
}

#[derive(Default)]
pub struct Cache<'kernel> {
    entries: Vec<Entry<'kernel>>,
}

impl<'kernel> Cache<'kernel> {
    pub fn check(&mut self, model: &Model, bodies: &[Rc<Body<'kernel>>]) -> Result<Report, String> {
        validate(model, bodies.len())?;
        let mut report = Report::default();
        let mut entries = Vec::with_capacity(bodies.len() * (bodies.len() - 1) / 2);
        let mut placed: Vec<_> = (0..bodies.len()).map(|_| None).collect();
        let references = bodies.iter().map(Rc::as_ref).collect::<Vec<_>>();
        for first in 0..bodies.len() {
            for second in first + 1..bodies.len() {
                let parts = [first, second];
                let placements = parts.map(|index| model.parts[index].placement(Layout::Assembly));
                let matches = |entry: &&Entry<'kernel>| {
                    entry.placements == placements
                        && entry.bodies.iter().zip(parts).all(|(body, index)| {
                            std::ptr::eq(body.as_ptr(), Rc::as_ptr(&bodies[index]))
                        })
                };
                let cached = self
                    .entries
                    .get(report.pairs)
                    .filter(matches)
                    .or_else(|| self.entries.iter().find(matches));
                let result = if let Some(entry) = cached {
                    report.cache_hits += 1;
                    Ok(entry.volume_mm3)
                } else {
                    pair(
                        model,
                        &references,
                        &mut placed,
                        parts,
                        &mut report.boolean_tests,
                    )
                };
                if let Ok(volume_mm3) = result.as_ref() {
                    entries.push(Entry {
                        bodies: parts.map(|index| Rc::downgrade(&bodies[index])),
                        placements,
                        volume_mm3: *volume_mm3,
                    });
                }
                record(&mut report, parts, result);
            }
        }
        assert!(entries.len() <= PAIR_COUNT_MAX);
        self.entries = entries;
        Ok(report)
    }
}

pub fn check(model: &Model, bodies: &[Body<'_>]) -> Result<Report, String> {
    validate(model, bodies.len())?;
    let bodies = bodies.iter().collect::<Vec<_>>();
    let mut placed: Vec<_> = (0..bodies.len()).map(|_| None).collect();
    let mut report = Report::default();
    for first in 0..bodies.len() {
        for second in first + 1..bodies.len() {
            let parts = [first, second];
            let result = pair(
                model,
                &bodies,
                &mut placed,
                parts,
                &mut report.boolean_tests,
            );
            record(&mut report, parts, result);
        }
    }
    assert!(report.pairs <= PAIR_COUNT_MAX);
    Ok(report)
}

fn validate(model: &Model, count: usize) -> Result<(), String> {
    if count != model.parts.len() || !(1..=32).contains(&count) {
        return Err("Assembly checks require 1–32 matching solids".into());
    }
    Ok(())
}

fn pair<'kernel>(
    model: &Model,
    bodies: &[&Body<'kernel>],
    placed: &mut [Option<(Body<'kernel>, [glam::DVec3; 2])>],
    parts: [usize; 2],
    boolean_tests: &mut usize,
) -> Result<f64, String> {
    for index in parts {
        if placed[index].is_none() {
            let mut body = bodies[index].try_clone()?;
            model.parts[index]
                .placement(Layout::Assembly)
                .apply(&mut body, false)?;
            let bounds = body.bounds(true)?;
            placed[index] = Some((body, bounds));
        }
    }
    let (first, a) = placed[parts[0]].as_ref().expect("Placed first part");
    let (second, b) = placed[parts[1]].as_ref().expect("Placed second part");
    if a[1].cmple(b[0]).any() || b[1].cmple(a[0]).any() {
        return Ok(0.0);
    }
    *boolean_tests += 1;
    first.intersection_volume(second)
}

fn record(report: &mut Report, parts: [usize; 2], result: Result<f64, String>) {
    report.pairs += 1;
    assert!(parts[0] < parts[1]);
    assert!(report.pairs <= PAIR_COUNT_MAX);
    match result {
        Ok(volume_mm3) if volume_mm3 > VOLUME_TOLERANCE_MM3 => {
            report.overlaps.push(Overlap { parts, volume_mm3 })
        }
        Ok(_) => {}
        Err(message) => report.failures.push(Failure { parts, message }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{kernel::Kernel, model};

    #[test]
    fn maximum_assembly_reuses_all_pairs_without_intersections() -> Result<(), String> {
        let kernel = Kernel::new()?;
        let mut geometry = model::Cache::new(&kernel);
        let model = geometry.compile_project("for index=1,32 do cad.show(tostring(index),cad.box({width=10,depth=10,height=10}),{assembly={position={x=index*20}}}) end", None)?;
        let prepared = geometry.prepare(&model)?;
        let started = std::time::Instant::now();
        let cold = geometry.assembly_checks.check(&model, &prepared.bodies)?;
        let cold_ms = started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(cold.pairs, PAIR_COUNT_MAX);
        assert_eq!(cold.boolean_tests, 0);
        assert!(cold.failures.is_empty());
        let started = std::time::Instant::now();
        for _ in 0..100 {
            let warm = geometry.assembly_checks.check(&model, &prepared.bodies)?;
            assert_eq!(warm.cache_hits, PAIR_COUNT_MAX);
            assert_eq!(warm.boolean_tests, 0);
            assert!(warm.overlaps.is_empty());
        }
        eprintln!(
            "Assembly checks: 32 parts, {PAIR_COUNT_MAX} pairs, cold_ms={cold_ms:.3}, warm_ms_mean={:.3}",
            started.elapsed().as_secs_f64() * 10.0
        );
        Ok(())
    }

    #[test]
    fn intersections_distinguish_overlap_containment_and_zero_volume_contact() -> Result<(), String>
    {
        let kernel = Kernel::new()?;
        let first = kernel.box_shape([0.0; 3], [10.0; 3])?;
        for (minimum, maximum, expected) in [
            ([5.0, 0.0, 0.0], [15.0, 10.0, 10.0], 500.0),
            ([1.0; 3], [3.0; 3], 8.0),
            ([0.0; 3], [10.0; 3], 1000.0),
            ([10.0, 0.0, 0.0], [20.0, 10.0, 10.0], 0.0),
            ([11.0, 0.0, 0.0], [21.0, 10.0, 10.0], 0.0),
        ] {
            let other = kernel.box_shape(minimum, maximum)?;
            let volume = first.intersection_volume(&other)?;
            assert!((volume - expected).abs() < 1e-6, "{volume} != {expected}");
            assert!((first.volume()? - 1000.0).abs() < 1e-6);
            assert!(
                (other.volume()?
                    - (maximum[0] - minimum[0])
                        * (maximum[1] - minimum[1])
                        * (maximum[2] - minimum[2]))
                    .abs()
                    < 1e-6
            );
        }
        Ok(())
    }

    #[test]
    fn curved_disjoint_solids_with_overlapping_bounds_are_not_collisions() -> Result<(), String> {
        let model = model::compile(
            "cad.show('a',cad.cylinder({radius=2,height=5}));cad.show('b',cad.cylinder({radius=2,height=5,x=3,y=3}))",
        )?;
        let kernel = Kernel::new()?;
        let bodies = model.build(&kernel)?;
        let report = check(&model, &bodies)?;
        assert_eq!(report.boolean_tests, 1);
        assert!(report.overlaps.is_empty());
        assert!(
            report.failures.is_empty(),
            "{:?}",
            report.failures.first().map(|f| &f.message)
        );
        Ok(())
    }

    #[test]
    fn placement_and_geometry_changes_invalidate_only_affected_pairs() -> Result<(), String> {
        let kernel = Kernel::new()?;
        let mut geometry = model::Cache::new(&kernel);
        let source = "local a=cad.box({width=10,depth=10,height=10});cad.show('a',a,{print={}});cad.show('b',cad.translate(a,{x=0}),{assembly={position={x=5}},print={position={x=40}}});cad.show('c',cad.box({width=2,depth=2,height=2}),{assembly={position={x=30}},print={position={x=70}}})";
        let model = geometry.compile_project(source, None)?;
        let prepared = geometry.prepare(&model)?;
        let first = geometry.assembly_checks.check(&model, &prepared.bodies)?;
        assert_eq!(first.overlaps.len(), 1);
        assert!((first.overlaps[0].volume_mm3 - 500.0).abs() < 1e-6);
        assert_eq!(first.boolean_tests, 1);
        let warm = geometry.assembly_checks.check(&model, &prepared.bodies)?;
        assert_eq!(warm.cache_hits, 3);
        assert_eq!(warm.boolean_tests, 0);
        assert!((prepared.bodies[1].bounds(true)?[0].x).abs() < 1e-6);
        let print_edit = geometry.compile_project(&source.replace("x=40", "x=50"), None)?;
        let prepared = geometry.prepare(&print_edit)?;
        assert_eq!(
            geometry
                .assembly_checks
                .check(&print_edit, &prepared.bodies)?
                .cache_hits,
            3
        );
        let moved = geometry.compile_project(&source.replace("x=5}", "x=20}"), None)?;
        let prepared = geometry.prepare(&moved)?;
        let result = geometry.assembly_checks.check(&moved, &prepared.bodies)?;
        assert_eq!(result.cache_hits, 1);
        assert!(result.overlaps.is_empty());
        assert_eq!(result.boolean_tests, 0);
        let changed = geometry
            .compile_project(&source.replace("width=2,depth=2", "width=3,depth=2"), None)?;
        let prepared = geometry.prepare(&changed)?;
        let result = geometry.assembly_checks.check(&changed, &prepared.bodies)?;
        assert_eq!(result.cache_hits, 0);
        assert_eq!(result.overlaps.len(), 1);
        Ok(())
    }

    #[test]
    fn rotations_use_assembly_placements_and_failures_remain_explicit() -> Result<(), String> {
        let model = model::compile(
            "local a=cad.box({width=10,depth=2,height=2});cad.show('a',a,{print={}});cad.show('b',cad.translate(a,{x=0}),{assembly={rotation={z=90},position={x=2}},print={position={x=50}}})",
        )?;
        let kernel = Kernel::new()?;
        let bodies = model.build(&kernel)?;
        let report = check(&model, &bodies)?;
        assert_eq!(report.overlaps.len(), 1);
        assert!((report.overlaps[0].volume_mm3 - 8.0).abs() < 1e-6);
        let mut report = Report::default();
        record(&mut report, [0, 1], Err("Kernel failed".into()));
        assert_eq!(report.failures.len(), 1);
        assert!(report.overlaps.is_empty());
        Ok(())
    }
}
