use crate::{mesh::Mesh, model, placement::Layout};
use glam::Vec3;
use mlua::Table;

const SAMPLE_COUNT_MAX: usize = 128;
const TRIANGLE_VISITS_MAX: usize = 8_000_000;

#[derive(Clone, Copy)]
pub struct Options {
    pub minimum_wall_mm: f32,
    pub minimum_clearance_mm: f32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            minimum_wall_mm: 0.8,
            minimum_clearance_mm: 0.2,
        }
    }
}

impl Options {
    pub fn from_table(table: &Table) -> mlua::Result<Self> {
        crate::model::options_fields(table, &["minimum_wall", "minimum_clearance"])?;
        let defaults = Self::default();
        let minimum_wall_mm = table
            .get::<Option<f32>>("minimum_wall")?
            .unwrap_or(defaults.minimum_wall_mm);
        let minimum_clearance_mm = table
            .get::<Option<f32>>("minimum_clearance")?
            .unwrap_or(defaults.minimum_clearance_mm);
        if !minimum_wall_mm.is_finite() || !(0.01..=10.0).contains(&minimum_wall_mm) {
            return Err(mlua::Error::runtime(
                "Minimum print wall must be 0.01–10 mm",
            ));
        }
        if !minimum_clearance_mm.is_finite() || !(0.0..=5.0).contains(&minimum_clearance_mm) {
            return Err(mlua::Error::runtime(
                "Minimum assembly clearance must be 0–5 mm",
            ));
        }
        Ok(Self {
            minimum_wall_mm,
            minimum_clearance_mm,
        })
    }
}

struct Part {
    name: String,
    size_mm: glam::DVec3,
    on_bed: bool,
    fits_bed: bool,
    wall: Option<Sample>,
    sample_count: usize,
}

struct Clearance {
    parts: [usize; 2],
    names: [String; 2],
    sample: Sample,
    sample_count: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub points: [Vec3; 2],
    pub distance_mm: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Blocked,
    Warning,
    Info,
}

pub struct Finding {
    pub title: String,
    pub detail: String,
    pub severity: Severity,
    pub parts: [usize; 2],
    pub layout: Layout,
    pub sample: Option<Sample>,
}

pub struct Report {
    assembly: crate::assembly_checks::Report,
    options: Options,
    parts: Vec<Part>,
    clearances: Vec<Clearance>,
    skipped_pairs: usize,
}

impl Report {
    pub fn options(&self) -> Options {
        self.options
    }
    pub fn part_count(&self) -> usize {
        self.parts.len()
    }
    pub fn skipped_pairs(&self) -> usize {
        self.skipped_pairs
    }
    pub fn assembly(&self) -> &crate::assembly_checks::Report {
        &self.assembly
    }

    pub fn findings(&self) -> Vec<Finding> {
        let mut findings = Vec::with_capacity(
            self.parts.len() * 3
                + self.clearances.len()
                + self.assembly.overlaps.len()
                + self.assembly.failures.len(),
        );
        for (index, part) in self.parts.iter().enumerate() {
            let size = format!(
                "{:.2} × {:.2} × {:.2} mm",
                part.size_mm.x, part.size_mm.y, part.size_mm.z
            );
            if !part.fits_bed {
                findings.push(Finding { title: format!("{} · exceeds P1S envelope", part.name), detail: format!("{size}. Each dimension must fit within 256 mm. Change dimensions or print orientation."), severity: Severity::Blocked, parts: [index; 2], layout: Layout::Print, sample: None });
            }
            if !part.on_bed {
                findings.push(Finding { title: format!("{} · off the bed", part.name), detail: "The lowest point must be at Z=0 in print placement. Adjust the source or print placement.".into(), severity: Severity::Blocked, parts: [index; 2], layout: Layout::Print, sample: None });
            }
            let (title, detail, severity) = match part.wall {
                Some(sample) => (
                    format!("{} · sampled wall {:.3} mm", part.name, sample.distance_mm),
                    format!(
                        "{size} · {} wall rays. Warning threshold {:.2} mm. Select to inspect this sample in print placement.",
                        part.sample_count, self.options.minimum_wall_mm
                    ),
                    if sample.distance_mm < self.options.minimum_wall_mm {
                        Severity::Warning
                    } else {
                        Severity::Info
                    },
                ),
                None => (
                    format!("{} · wall not established", part.name),
                    format!(
                        "{size}. No usable thickness sample within the work budget; inspect this part in the slicer."
                    ),
                    Severity::Warning,
                ),
            };
            findings.push(Finding {
                title,
                detail,
                severity,
                parts: [index; 2],
                layout: Layout::Print,
                sample: part.wall,
            });
        }
        for clearance in &self.clearances {
            findings.push(Finding { title: format!("{} / {} · sampled gap {:.3} mm", clearance.names[0], clearance.names[1], clearance.sample.distance_mm), detail: format!("{} samples · warning threshold {:.2} mm. Shown in assembly placement; intentional mating contact may be acceptable.", clearance.sample_count, self.options.minimum_clearance_mm), severity: Severity::Warning, parts: clearance.parts, layout: Layout::Assembly, sample: Some(clearance.sample) });
        }
        for overlap in &self.assembly.overlaps {
            let names = overlap.parts.map(|index| self.parts[index].name.as_str());
            findings.push(Finding { title: format!("{} / {} · overlap {:.6} mm³", names[0], names[1], overlap.volume_mm3), detail: "Exact B-rep intersection in assembly placement. Select to highlight both parts. Intentional interference fits may be acceptable; separate part exports remain available.".into(), severity: Severity::Warning, parts: overlap.parts, layout: Layout::Assembly, sample: None });
        }
        for failure in &self.assembly.failures {
            let names = failure.parts.map(|index| self.parts[index].name.as_str());
            findings.push(Finding {
                title: format!("{} / {} · collision check incomplete", names[0], names[1]),
                detail: failure.message.clone(),
                severity: Severity::Warning,
                parts: failure.parts,
                layout: Layout::Assembly,
                sample: None,
            });
        }
        findings.sort_by_key(|finding| match finding.severity {
            Severity::Blocked => 0,
            Severity::Warning => 1,
            Severity::Info => 2,
        });
        findings
    }
    pub fn blocked(&self) -> bool {
        self.parts.iter().any(|part| !part.on_bed || !part.fits_bed)
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "units": "mm",
            "printer": "Bambu Lab P1S",
            "bed_size": [256, 256, 256],
            "minimum_wall": self.options.minimum_wall_mm,
            "minimum_clearance": self.options.minimum_clearance_mm,
            "blocked": self.blocked(),
            "wall_sample_space": "print placement",
            "gap_sample_space": "assembly placement",
            "assembly_collisions": {
                "space": "assembly placement",
                "method": "Native B-rep intersection and volume integration after bounding-box filtering; numerical kernel tolerances apply. Zero-volume contact is not an overlap.",
                "volume_tolerance_mm3": crate::assembly_checks::VOLUME_TOLERANCE_MM3,
                "pairs": self.assembly.pairs,
                "boolean_tests": self.assembly.boolean_tests,
                "cache_hits": self.assembly.cache_hits,
                "complete": self.assembly.failures.is_empty(),
                "overlaps": self.assembly.overlaps.iter().map(|overlap| serde_json::json!({"parts": overlap.parts.map(|index| &self.parts[index].name), "volume_mm3": overlap.volume_mm3})).collect::<Vec<_>>(),
                "failures": self.assembly.failures.iter().map(|failure| serde_json::json!({"parts": failure.parts.map(|index| &self.parts[index].name), "error": failure.message})).collect::<Vec<_>>(),
            },
            "method": "Native envelope bounds and separate B-rep collision checks. Walls and gaps use bounded triangle-centroid samples. Wall rays follow inward triangle normals; sampled assembly surface distances are upper bounds, not exact minimum clearance. Missing warnings do not establish printability.",
            "parts": self.parts.iter().map(|part| serde_json::json!({
                "name": part.name,
                "print_size": part.size_mm.to_array(),
                "on_bed": part.on_bed,
                "fits_bed": part.fits_bed,
                "sampled_wall": part.wall.map(|sample| sample.distance_mm),
                "wall_sample_points": part.wall.map(|sample| sample.points.map(|point| point.to_array())),
                "wall_samples": part.sample_count,
                "thin_wall_warning": part.wall.is_some_and(|sample| sample.distance_mm < self.options.minimum_wall_mm),
            })).collect::<Vec<_>>(),
            "close_assembly_surfaces": self.clearances.iter().map(|clearance| serde_json::json!({
                "parts": clearance.names,
                "sampled_distance": clearance.sample.distance_mm,
                "sample_points": clearance.sample.points.map(|point| point.to_array()),
                "samples": clearance.sample_count,
            })).collect::<Vec<_>>(),
            "pairs_skipped_by_budget": self.skipped_pairs,
        })
    }

    pub fn markdown(&self) -> String {
        let mut text = format!(
            "# Print checks\n\nP1S envelope: 256 × 256 × 256 mm. Minimum sampled wall: {:.2} mm; assembly clearance warning threshold: {:.2} mm.\n\n",
            self.options.minimum_wall_mm, self.options.minimum_clearance_mm
        );
        for part in &self.parts {
            text.push_str(&format!(
                "- {}: {:.2} × {:.2} × {:.2} mm; {}. ",
                part.name,
                part.size_mm.x,
                part.size_mm.y,
                part.size_mm.z,
                if part.on_bed && part.fits_bed {
                    "envelope and bed contact checked"
                } else {
                    "BLOCKED: check bed placement and size"
                }
            ));
            match part.wall {
                Some(sample) => text.push_str(&format!(
                    "Sampled wall {:.3} mm ({} rays){}.\n",
                    sample.distance_mm,
                    part.sample_count,
                    if sample.distance_mm < self.options.minimum_wall_mm {
                        "; THIN WALL WARNING"
                    } else {
                        ""
                    }
                )),
                None => text.push_str("Wall thickness was not established.\n"),
            }
        }
        for clearance in &self.clearances {
            text.push_str(&format!("- CLOSE SURFACES: {} / {}: sampled assembly gap {:.3} mm. Intended contact may be acceptable.\n", clearance.names[0], clearance.names[1], clearance.sample.distance_mm));
        }
        text.push_str(&format!("\nAssembly collision checks: {} pairs; {} B-rep intersections; {} cached results. Reporting overlap above {} mm³; zero-volume contact is allowed.\n", self.assembly.pairs, self.assembly.boolean_tests, self.assembly.cache_hits, crate::assembly_checks::VOLUME_TOLERANCE_MM3));
        for overlap in &self.assembly.overlaps {
            text.push_str(&format!(
                "- OVERLAP: {} / {}: {:.6} mm³ in assembly placement.\n",
                self.parts[overlap.parts[0]].name,
                self.parts[overlap.parts[1]].name,
                overlap.volume_mm3
            ));
        }
        for failure in &self.assembly.failures {
            text.push_str(&format!(
                "- INCOMPLETE: {} / {}: {}\n",
                self.parts[failure.parts[0]].name,
                self.parts[failure.parts[1]].name,
                failure.message
            ));
        }
        if self.skipped_pairs > 0 {
            text.push_str(&format!(
                "\n{} assembly pairs were not sampled because the work budget was exhausted.\n",
                self.skipped_pairs
            ));
        }
        text.push_str("\nWall and gap values are bounded mesh samples, not exact minimum thickness or clearance. Collision checks use native B-rep intersections with numerical kernel tolerances; failures are reported as incomplete. Features can fall between mesh samples; curved surfaces depend on tessellation. Check the slicer and print a fit gauge with your actual filament and settings. Part checks use print placements; gaps and collisions use assembly placements. Each part is exported separately; this does not validate a combined plate arrangement.\n");
        text
    }
}

pub fn model_report(model: &model::Model) -> Result<Report, String> {
    let kernel = crate::kernel::Kernel::new()?;
    let mut bodies = model.build(&kernel)?;
    let meshes = model.meshes(&mut bodies, false)?;
    let assembly = crate::assembly_checks::check(model, &bodies)?;
    let mut print_bounds = Vec::with_capacity(bodies.len());
    for (body, part) in bodies.iter_mut().zip(&model.parts) {
        part.placement(Layout::Print).apply(body, false)?;
        print_bounds.push(body.bounds(true)?);
    }
    report(
        model,
        &meshes.iter().collect::<Vec<_>>(),
        &print_bounds,
        assembly,
    )
}

pub fn cached_report(
    source: &str,
    project: Option<&crate::project::Project>,
    cache: &mut model::Cache<'_>,
) -> Result<Report, String> {
    let model = cache.compile_project(source, project)?;
    let prepared = cache.prepare(&model)?;
    let assembly = cache.assembly_checks.check(&model, &prepared.bodies)?;
    let print_bounds = prepared
        .bodies
        .iter()
        .zip(&model.parts)
        .map(|(body, part)| {
            let mut placed = body.try_clone()?;
            part.placement(Layout::Print).apply(&mut placed, false)?;
            placed.bounds(true)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let meshes = prepared
        .geometry
        .iter()
        .map(|geometry| &geometry.mesh)
        .collect::<Vec<_>>();
    report(&model, &meshes, &print_bounds, assembly)
}

pub fn report(
    model: &model::Model,
    meshes: &[&Mesh],
    print_bounds: &[[glam::DVec3; 2]],
    assembly_checks: crate::assembly_checks::Report,
) -> Result<Report, String> {
    if meshes.len() != model.parts.len()
        || meshes.len() != print_bounds.len()
        || meshes.is_empty()
        || meshes.len() > 32
    {
        return Err("Print checks require 1–32 matching parts and meshes".into());
    }
    assert_eq!(meshes.len(), model.parts.len());
    assert!(!meshes.is_empty());
    let mut parts = Vec::with_capacity(meshes.len());
    let mut assembly = Vec::with_capacity(meshes.len());
    let mut bounds = Vec::with_capacity(meshes.len());
    for ((part, mesh), print_bounds) in model.parts.iter().zip(meshes).zip(print_bounds) {
        if mesh.vertices.is_empty()
            || mesh.vertices.len() > 300000
            || !mesh.vertices.len().is_multiple_of(3)
        {
            return Err("Print checks require complete bounded meshes".into());
        }
        let size_mm = print_bounds[1] - print_bounds[0];
        let (wall, sample_count) = sampled_wall(mesh, TRIANGLE_VISITS_MAX / meshes.len());
        let print_matrix = part.placement(Layout::Print).matrix();
        parts.push(Part {
            name: part.name.clone(),
            size_mm,
            on_bed: print_bounds[0].z.abs() <= 0.001,
            fits_bed: size_mm.max_element() <= 256.0001,
            wall: wall.map(|sample| Sample {
                points: sample
                    .points
                    .map(|point| print_matrix.transform_point3(point)),
                ..sample
            }),
            sample_count,
        });
        if model.print_options.minimum_clearance_mm > 0.0 {
            let placed = mesh.placed(part.placement(Layout::Assembly).matrix(), part.color);
            bounds.push(mesh_bounds(&placed, glam::Mat4::IDENTITY));
            assembly.push(placed);
        }
    }
    let mut clearances = Vec::new();
    let mut budget = TRIANGLE_VISITS_MAX;
    let mut skipped_pairs = 0;
    for first in 0..assembly.len() {
        for second in first + 1..assembly.len() {
            let lower = (bounds[first][0] - bounds[second][1])
                .max(bounds[second][0] - bounds[first][1])
                .max(Vec3::ZERO)
                .length();
            if lower >= model.print_options.minimum_clearance_mm {
                continue;
            }
            let triangles = (assembly[first].vertices.len() + assembly[second].vertices.len()) / 3;
            let samples = SAMPLE_COUNT_MAX
                .min(budget / triangles)
                .min(assembly[first].vertices.len() / 3)
                .min(assembly[second].vertices.len() / 3);
            if samples == 0 {
                skipped_pairs += 1;
                continue;
            }
            budget -= samples * triangles;
            let forward = sampled_distance(&assembly[first], &assembly[second], samples);
            let reverse = sampled_distance(&assembly[second], &assembly[first], samples);
            let sample = if forward.distance_mm <= reverse.distance_mm {
                forward
            } else {
                reverse
            };
            if sample.distance_mm < model.print_options.minimum_clearance_mm {
                clearances.push(Clearance {
                    parts: [first, second],
                    names: [
                        model.parts[first].name.clone(),
                        model.parts[second].name.clone(),
                    ],
                    sample,
                    sample_count: samples * 2,
                });
            }
        }
    }
    Ok(Report {
        assembly: assembly_checks,
        options: model.print_options,
        parts,
        clearances,
        skipped_pairs,
    })
}

fn mesh_bounds(mesh: &Mesh, matrix: glam::Mat4) -> [Vec3; 2] {
    assert!(!mesh.vertices.is_empty());
    assert!(mesh.vertices.len() <= 300000);
    let mut bounds = [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)];
    for vertex in &mesh.vertices {
        let point = matrix.transform_point3(Vec3::from_array(vertex.position));
        bounds = [bounds[0].min(point), bounds[1].max(point)];
    }
    bounds
}

fn sampled_wall(mesh: &Mesh, budget: usize) -> (Option<Sample>, usize) {
    let triangles = mesh.vertices.as_chunks::<3>().0;
    assert!(!triangles.is_empty());
    assert!(budget <= TRIANGLE_VISITS_MAX);
    let samples = SAMPLE_COUNT_MAX
        .min(budget / triangles.len())
        .min(triangles.len());
    let mut minimum = f64::INFINITY;
    let mut closest = None;
    let mut checked = 0;
    for sample in 0..samples {
        let index = sample * triangles.len() / samples;
        let points = triangles[index].map(|vertex| Vec3::from_array(vertex.position).as_dvec3());
        let normal = (points[1] - points[0])
            .cross(points[2] - points[0])
            .normalize_or_zero();
        if normal.length_squared() < 0.5 {
            continue;
        }
        let center = (points[0] + points[1] + points[2]) / 3.0;
        if let Some(distance_mm) = wall_ray(triangles, index, center, -normal) {
            if distance_mm < minimum {
                minimum = distance_mm;
                closest = Some(Sample {
                    points: [center.as_vec3(), (center - normal * distance_mm).as_vec3()],
                    distance_mm: distance_mm as f32,
                });
            }
            checked += 1;
        }
    }
    (closest, checked)
}

fn wall_ray(
    triangles: &[[crate::mesh::Vertex; 3]],
    source: usize,
    origin: glam::DVec3,
    direction: glam::DVec3,
) -> Option<f64> {
    assert!(source < triangles.len());
    assert!(origin.is_finite());
    assert!(direction.is_finite());
    let mut closest = f64::INFINITY;
    for (index, triangle) in triangles.iter().enumerate() {
        if index == source {
            continue;
        }
        let [first, second, third] =
            triangle.map(|vertex| Vec3::from_array(vertex.position).as_dvec3());
        let edge_first = second - first;
        let edge_second = third - first;
        if edge_first.cross(edge_second).dot(direction) <= 1e-12 {
            continue;
        }
        let cross = direction.cross(edge_second);
        let determinant = edge_first.dot(cross);
        let offset = origin - first;
        let u = offset.dot(cross) / determinant;
        if !(-1e-9..=1.0 + 1e-9).contains(&u) {
            continue;
        }
        let other = offset.cross(edge_first);
        let v = direction.dot(other) / determinant;
        if v < -1e-9 || u + v > 1.0 + 1e-9 {
            continue;
        }
        let distance = edge_second.dot(other) / determinant;
        if distance > 1e-7 {
            closest = closest.min(distance);
        }
    }
    closest.is_finite().then_some(closest)
}

fn sampled_distance(first: &Mesh, second: &Mesh, samples: usize) -> Sample {
    let triangles = first.vertices.as_chunks::<3>().0;
    assert!(!triangles.is_empty());
    assert!((1..=SAMPLE_COUNT_MAX).contains(&samples));
    let mut distance_squared = f32::INFINITY;
    let mut points_closest = [Vec3::ZERO; 2];
    for sample in 0..samples {
        let points = triangles[sample * triangles.len() / samples]
            .map(|vertex| Vec3::from_array(vertex.position));
        let center = (points[0] + points[1] + points[2]) / 3.0;
        for triangle in second.vertices.as_chunks::<3>().0 {
            let target = point_triangle_closest(
                center,
                triangle.map(|vertex| Vec3::from_array(vertex.position)),
            );
            let squared = center.distance_squared(target);
            if squared < distance_squared {
                distance_squared = squared;
                points_closest = [center, target];
            }
        }
    }
    Sample {
        points: points_closest,
        distance_mm: distance_squared.sqrt(),
    }
}

fn point_triangle_closest(point: Vec3, triangle: [Vec3; 3]) -> Vec3 {
    assert!(point.is_finite());
    assert!(triangle.iter().all(|point| point.is_finite()));
    let [first, second, third] = triangle;
    let normal = (second - first).cross(third - first);
    let normal_squared = normal.length_squared();
    if normal_squared > 1e-16 {
        let projected = point - normal * (point - first).dot(normal) / normal_squared;
        if (second - first).cross(projected - first).dot(normal) >= 0.0
            && (third - second).cross(projected - second).dot(normal) >= 0.0
            && (first - third).cross(projected - third).dot(normal) >= 0.0
        {
            return projected;
        }
    }
    let mut minimum = f32::INFINITY;
    let mut closest = first;
    for index in 0..3 {
        let first = triangle[index];
        let direction = triangle[(index + 1) % 3] - first;
        let along = ((point - first).dot(direction) / direction.length_squared().max(1e-20))
            .clamp(0.0, 1.0);
        let target = first + direction * along;
        let distance = point.distance_squared(target);
        if distance < minimum {
            minimum = distance;
            closest = target;
        }
    }
    closest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_collisions_remain_advisory_when_gap_sampling_is_disabled() -> Result<(), String> {
        let source = "cad.print_options({minimum_clearance=0});local a=cad.box({width=10,depth=10,height=10});cad.show('first',a,{print={}});cad.show('second',cad.translate(a,{x=0}),{assembly={position={x=5}},print={position={x=40}}})";
        let report = check(source)?;
        assert!(!report.blocked());
        assert!(report.clearances.is_empty());
        let json = report.json();
        assert_eq!(json["assembly_collisions"]["complete"], true);
        assert!(
            (json["assembly_collisions"]["overlaps"][0]["volume_mm3"]
                .as_f64()
                .ok_or("Overlap volume")?
                - 500.0)
                .abs()
                < 1e-6
        );
        let overlap = report
            .findings()
            .into_iter()
            .find(|finding| finding.title.contains("overlap"))
            .ok_or("Overlap finding")?;
        assert_eq!(overlap.parts, [0, 1]);
        assert!(overlap.layout == Layout::Assembly);
        assert!(overlap.sample.is_none());
        assert!(report.markdown().contains("OVERLAP"));
        let folder =
            std::env::temp_dir().join(format!("spice-collision-export-{}", std::process::id()));
        crate::export::model_files(source, &folder, crate::export::Target::Print)?;
        let exported =
            std::fs::read(folder.join("PRINT_CHECKS.json")).map_err(|error| error.to_string());
        std::fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        let json: serde_json::Value =
            serde_json::from_slice(&exported?).map_err(|error| error.to_string())?;
        assert_eq!(
            json["assembly_collisions"]["overlaps"]
                .as_array()
                .ok_or("Overlaps")?
                .len(),
            1
        );
        assert!(
            (json["assembly_collisions"]["overlaps"][0]["volume_mm3"]
                .as_f64()
                .ok_or("Exported overlap")?
                - 500.0)
                .abs()
                < 1e-6
        );
        Ok(())
    }

    fn check(source: &str) -> Result<Report, String> {
        let model = model::compile(source)?;
        model_report(&model)
    }

    #[test]
    fn detects_thin_internal_walls_in_a_large_envelope() -> Result<(), String> {
        let source = "cad.print_options({minimum_wall=1}); local outer=cad.box({width=30,depth=30,height=20}); local cut=cad.box({width=29,depth=29,height=20,x=0.5,y=0.5,z=0.5});cad.show('cup',cad.subtract(outer,cut))";
        let report = check(source)?;
        assert!(!report.blocked());
        assert!((report.parts[0].wall.ok_or("Missing thickness")?.distance_mm - 0.5).abs() < 0.001);
        assert_eq!(report.json()["parts"][0]["thin_wall_warning"], true);
        let wall = report.parts[0].wall.expect("Wall sample");
        assert!((wall.points[0].distance(wall.points[1]) - wall.distance_mm).abs() < 0.001);
        let findings = report.findings();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].severity == Severity::Warning);
        assert!(findings[0].layout == Layout::Print);
        Ok(())
    }

    #[test]
    fn uses_assembly_clearance_and_separate_print_placements() -> Result<(), String> {
        let source = "cad.print_options({minimum_clearance=0.2});local a=cad.box({width=10,depth=10,height=10});cad.show('first',a,{print={position={x=0}}});cad.show('second',cad.translate(a,{x=0}),{assembly={position={x=10.1}},print={position={x=30}}})";
        let report = check(source)?;
        assert_eq!(report.clearances.len(), 1);
        assert!((report.clearances[0].sample.distance_mm - 0.1).abs() < 0.001);
        assert!(!report.blocked());
        assert!(report.markdown().contains("CLOSE SURFACES"));
        let sample = report.clearances[0].sample;
        assert!((sample.points[0].distance(sample.points[1]) - 0.1).abs() < 0.001);
        assert!(
            sample
                .points
                .iter()
                .all(|point| (9.99..10.11).contains(&point.x))
        );
        let second_wall = report.parts[1].wall.expect("Second part wall sample");
        assert!(
            second_wall
                .points
                .iter()
                .all(|point| (29.99..40.01).contains(&point.x))
        );
        let findings = report.findings();
        let gap = findings
            .iter()
            .find(|finding| finding.layout == Layout::Assembly)
            .ok_or("Missing gap finding")?;
        assert_eq!(gap.parts, [0, 1]);
        Ok(())
    }

    #[test]
    fn cached_checks_preserve_geometry_and_transform_wall_markers_into_print_placement()
    -> Result<(), String> {
        let kernel = crate::kernel::Kernel::new()?;
        let mut cache = model::Cache::new(&kernel);
        let source = "cad.show('plate',cad.box({width=10,depth=8,height=0.5}),{print={position={x=20,y=30},rotation={z=90}}})";
        let model = cache.compile_project(source, None)?;
        let prepared = cache.prepare(&model)?;
        let geometry = std::sync::Arc::clone(&prepared.geometry[0]);
        drop(prepared);
        let report = cached_report(source, None, &mut cache)?;
        let sample = report.parts[0].wall.ok_or("Thickness sample")?;
        assert!((sample.distance_mm - 0.5).abs() < 0.001);
        assert!((sample.points[0].distance(sample.points[1]) - 0.5).abs() < 0.001);
        for point in sample.points {
            assert!((11.99..20.01).contains(&point.x));
            assert!((29.99..40.01).contains(&point.y));
        }
        let prepared = cache.prepare(&model)?;
        assert_eq!(prepared.hits, 1);
        assert!(std::sync::Arc::ptr_eq(&geometry, &prepared.geometry[0]));
        Ok(())
    }

    #[test]
    fn rejects_invalid_settings_and_reports_printer_limits() -> Result<(), String> {
        for options in [
            "minimum_wall=0",
            "minimum_wall=0/0",
            "minimum_clearance=-1",
            "unknown=2",
        ] {
            assert!(check(&format!("cad.print_options({{{options}}});cad.show('x',cad.box({{width=2,depth=2,height=2}}))")).is_err());
        }
        let report = check("cad.show('large',cad.box({width=257,depth=2,height=2,z=1}))")?;
        assert!(report.blocked());
        assert!(!report.parts[0].on_bed);
        assert!(!report.parts[0].fits_bed);
        Ok(())
    }

    #[test]
    fn exports_fit_gauge_with_source_and_advisory_reports() -> Result<(), String> {
        let source = include_str!("../../models/fit-gauge.luau");
        let report = check(source)?;
        assert_eq!(report.parts.len(), 10);
        assert!(!report.blocked());
        let folder = std::env::temp_dir().join(format!("spice-print-gauge-{}", std::process::id()));
        crate::export::model_files(source, &folder, crate::export::Target::Print)?;
        let result = (|| -> Result<(), String> {
            let json: serde_json::Value = serde_json::from_slice(
                &std::fs::read(folder.join("PRINT_CHECKS.json"))
                    .map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            assert_eq!(json["parts"].as_array().ok_or("Missing parts")?.len(), 10);
            assert!(folder.join("part-10.stl").is_file());
            assert!(folder.join("part-01.step").is_file());
            assert!(
                std::fs::read_to_string(folder.join("PRINT_CHECKS.md"))
                    .map_err(|error| error.to_string())?
                    .contains("bounded mesh samples")
            );
            assert_eq!(
                std::fs::read_to_string(folder.join("model.luau"))
                    .map_err(|error| error.to_string())?,
                source
            );
            Ok(())
        })();
        std::fs::remove_dir_all(folder).map_err(|error| error.to_string())?;
        result
    }

    #[test]
    fn wall_rays_do_not_self_hit_rotated_solids_at_large_coordinates() -> Result<(), String> {
        let report = check(
            "local b=cad.box({width=10,depth=11,height=12});b=cad.rotate(b,{axis='x',angle=31});b=cad.rotate(b,{axis='y',angle=37});b=cad.translate(b,{x=1800,y=1800,z=1800});cad.show('thick',b)",
        )?;
        assert!(
            report.parts[0]
                .wall
                .ok_or("Missing wall samples")?
                .distance_mm
                > 9.9
        );
        assert_eq!(report.json()["parts"][0]["thin_wall_warning"], false);
        Ok(())
    }
}
