use glam::{DVec3, dvec3};
use opencascade::primitives::{Face, Shape, Wire};
use std::{
    collections::BTreeMap,
    error::Error,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

type TestResult = Result<(), Box<dyn Error>>;
const TRIANGLE_COUNT_MAX: usize = 100_000;

struct Fixture {
    directory: PathBuf,
}

impl Fixture {
    fn new(name: &str, source: &str) -> Result<Self, Box<dyn Error>> {
        let root = std::env::var_os("SPICE_CAD_ARTIFACTS")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        std::fs::create_dir_all(&root)?;
        let directory = root.join(format!("spice-verification-{}-{name}", std::process::id()));
        std::fs::create_dir(&directory)?;
        let fixture = Self { directory };
        std::fs::write(fixture.source(), source)?;
        Ok(fixture)
    }

    fn source(&self) -> PathBuf {
        self.directory.join("model.luau")
    }
    fn exports(&self) -> PathBuf {
        self.directory.join("exports")
    }

    fn run(&self, flag: &str) -> Result<String, Box<dyn Error>> {
        let binary = std::env::var_os("SPICE_CAD_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/spice-cad")
            });
        let mut command = Command::new(&binary);
        let stdout_path = self.directory.join(format!("{flag}.stdout.log"));
        let stderr_path = self.directory.join(format!("{flag}.stderr.log"));
        command
            .arg(flag)
            .arg(self.source())
            .stdout(std::fs::File::create(&stdout_path)?)
            .stderr(std::fs::File::create(&stderr_path)?);
        if flag == "--export-model" {
            command.arg(self.exports());
        }
        let mut child = command.spawn().map_err(|error| {
            format!(
                "Cannot run {}: {error}. Build spice-cad first or set SPICE_CAD_BIN.",
                binary.display()
            )
        })?;
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            if child.try_wait()?.is_some() {
                break;
            }
            if Instant::now() >= deadline {
                child.kill()?;
                child.wait()?;
                return Err(format!("{flag} timed out for {}", self.source().display()).into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let status = child.wait()?;
        let stdout = fixture_log(&stdout_path)?;
        let stderr = fixture_log(&stderr_path)?;
        if !status.success() {
            return Err(format!(
                "{flag} failed ({status}) for {}: {stdout}\n{stderr}",
                self.source().display()
            )
            .into());
        }
        Ok(stdout)
    }

    fn volume(&self) -> Result<f64, Box<dyn Error>> {
        let output = self.run("--check-model")?;
        let line = output
            .lines()
            .find(|line| line.contains(" mm³ · "))
            .ok_or("Missing model statistics")?;
        let (_, value) = line.rsplit_once(": ").ok_or("Missing model volume")?;
        Ok(value
            .split_whitespace()
            .next()
            .ok_or("Missing volume value")?
            .parse()?)
    }

    fn finish(self) -> TestResult {
        std::fs::remove_dir_all(self.directory)?;
        Ok(())
    }
}

fn fixture_log(path: &Path) -> Result<String, Box<dyn Error>> {
    if std::fs::metadata(path)?.len() > 1024 * 1024 {
        return Err(format!("Model command log exceeds 1 MiB: {}", path.display()).into());
    }
    Ok(std::fs::read_to_string(path)?)
}

#[test]
fn failed_commands_preserve_source_and_logs() -> TestResult {
    let fixture = Fixture::new("failed-command", "this is invalid Luau")?;
    let directory = fixture.directory.clone();
    assert!(fixture.run("--check-model").is_err());
    drop(fixture);
    assert_eq!(
        std::fs::read_to_string(directory.join("model.luau"))?,
        "this is invalid Luau"
    );
    let stdout = directory.join("--check-model.stdout.log");
    let stderr = directory.join("--check-model.stderr.log");
    assert!(stdout.is_file());
    assert!(stderr.is_file());
    assert!(std::fs::metadata(stdout)?.len() + std::fs::metadata(stderr)?.len() > 0);
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn successful_fixtures_are_removed() -> TestResult {
    let fixture = Fixture::new("successful-command", "")?;
    let directory = fixture.directory.clone();
    fixture.finish()?;
    assert!(!directory.try_exists()?);
    Ok(())
}

fn inspect_step(path: &Path) -> Result<(f64, [DVec3; 2]), Box<dyn Error>> {
    let volume_surface_mm3 = crate::geometry::volume_mm3(path)?;
    let shape = Shape::read_step(path)?;
    let mesh = shape.mesh()?;
    assert!(!mesh.indices.is_empty());
    assert!(mesh.indices.len() <= TRIANGLE_COUNT_MAX * 3);
    assert!(mesh.vertices.len() <= TRIANGLE_COUNT_MAX * 3);
    assert_eq!(mesh.indices.len() % 3, 0);
    let mut edges = BTreeMap::<([i64; 3], [i64; 3]), (u32, i32)>::new();
    let mut volume_mm3 = 0.0;
    let mut bounds = [DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)];
    for triangle in mesh.indices.as_chunks::<3>().0 {
        let points = triangle.map(|index| mesh.vertices[index]);
        assert!(points.iter().all(|point| point.is_finite()));
        for point in points {
            bounds = [bounds[0].min(point), bounds[1].max(point)];
        }
        volume_mm3 += points[0].dot(points[1].cross(points[2])) / 6.0;
        let welded = points.map(|point| {
            point
                .to_array()
                .map(|value| (value * 1000.0).round() as i64)
        });
        // OCCT pole triangles can collapse under the same 0.001 mm weld used by the native audit.
        if welded[0] == welded[1] || welded[1] == welded[2] || welded[2] == welded[0] {
            continue;
        }
        for (source, target) in [(0, 1), (1, 2), (2, 0)] {
            let first = welded[source];
            let second = welded[target];
            let (key, direction) = if first < second {
                ((first, second), 1)
            } else {
                ((second, first), -1)
            };
            let entry = edges.entry(key).or_default();
            entry.0 += 1;
            entry.1 += direction;
        }
    }
    assert!(!edges.is_empty());
    assert!(
        edges
            .values()
            .all(|&(count, direction)| count == 2 && direction == 0),
        "Imported STEP mesh is not closed and oriented: {}",
        path.display()
    );
    assert!(volume_mm3 > 0.0);
    assert!(
        (volume_mm3 - volume_surface_mm3).abs() < volume_surface_mm3 * 0.002,
        "STEP mesh volume {volume_mm3} differs from surface volume {volume_surface_mm3}: {}",
        path.display()
    );
    Ok((volume_surface_mm3, bounds))
}

fn section(shape: &Shape, height_mm: f64) -> Result<Shape, Box<dyn Error>> {
    assert!(height_mm.is_finite());
    let wire = Wire::from_ordered_points([
        dvec3(-50.0, -50.0, height_mm),
        dvec3(50.0, -50.0, height_mm),
        dvec3(50.0, 50.0, height_mm),
        dvec3(-50.0, 50.0, height_mm),
    ])?;
    let plane = Shape::from(Face::from_wire(&wire));
    Ok(shape.intersect(&plane).into())
}

fn section_difference_area_mm2(actual: &Shape, expected: &Shape) -> f64 {
    let missing: Shape = expected.subtract(actual).into();
    let extra: Shape = actual.subtract(expected).into();
    let area: f64 = missing
        .faces()
        .chain(extra.faces())
        .map(|face| face.surface_area())
        .sum();
    assert!(area.is_finite());
    assert!(area >= 0.0);
    area
}

fn section_intersections_x_mm(shape: &Shape, y_mm: f64, height_mm: f64) -> Vec<f64> {
    let hits = shape.faces_along_line(dvec3(-50.0, y_mm, height_mm), DVec3::X);
    assert!(hits.len() <= 32);
    let mut positions: Vec<f64> = hits.into_iter().map(|hit| hit.point.x).collect();
    assert!(positions.iter().all(|position| position.is_finite()));
    positions.sort_by(f64::total_cmp);
    positions.dedup_by(|first, second| (*first - *second).abs() < 1e-6);
    positions
}

#[test]
fn exported_sections_match_independent_cavity_and_bushing_fixtures() -> TestResult {
    let cavity: Shape = Shape::box_with_dimensions(10.0, 10.0, 10.0)
        .subtract(&Shape::box_from_corners(
            dvec3(2.0, 2.0, 2.0),
            dvec3(8.0, 8.0, 8.0),
        ))
        .into();
    let flange = Shape::cylinder(DVec3::ZERO, 16.0, DVec3::Z, 4.0);
    let stem = Shape::cylinder(dvec3(0.0, 0.0, 4.0), 10.0, DVec3::Z, 16.0);
    let outer: Shape = flange.union(&stem).into();
    let bushing: Shape = outer
        .subtract(&Shape::cylinder(DVec3::ZERO, 5.0, DVec3::Z, 20.0))
        .into();
    for (name, source, reference, volume_expected_mm3, sections) in [
        (
            "section-cavity",
            "local a=cad.box({width=10,depth=10,height=10});local b=cad.box({width=6,depth=6,height=6,x=2,y=2,z=2});cad.show('hollow',cad.subtract(a,b))",
            cavity,
            784.0,
            vec![(1.0, 100.0), (5.0, 64.0), (9.0, 100.0)],
        ),
        (
            "section-bushing",
            include_str!("../../../examples/revolved-bushing.luau"),
            bushing,
            std::f64::consts::PI * (231.0 * 4.0 + 75.0 * 16.0),
            vec![
                (2.0, std::f64::consts::PI * 231.0),
                (10.0, std::f64::consts::PI * 75.0),
            ],
        ),
    ] {
        let fixture = Fixture::new(name, source)?;
        fixture.run("--export-model")?;
        let path = fixture.exports().join("model.step");
        let (volume, _) = inspect_step(&path)?;
        assert!(
            (volume - volume_expected_mm3).abs() < volume_expected_mm3 * 1e-4,
            "{name}: {volume}"
        );
        let actual = Shape::read_step(path)?;
        for (height_mm, area_expected_mm2) in sections {
            let actual_section = section(&actual, height_mm)?;
            let reference_section = section(&reference, height_mm)?;
            let area: f64 = actual_section.faces().map(|face| face.surface_area()).sum();
            assert!(
                (area - area_expected_mm2).abs() < area_expected_mm2 * 1e-4,
                "{name} at {height_mm}: {area}"
            );
            let difference = section_difference_area_mm2(&actual_section, &reference_section);
            assert!(
                difference < area_expected_mm2 * 1e-4,
                "{name} at {height_mm}: section difference {difference}"
            );
            for y_mm in [1.0, 3.0, 6.0, 12.0] {
                let actual_positions = section_intersections_x_mm(&actual, y_mm, height_mm);
                let expected_positions = section_intersections_x_mm(&reference, y_mm, height_mm);
                assert_eq!(
                    actual_positions.len(),
                    expected_positions.len(),
                    "{name} at z={height_mm}, y={y_mm}"
                );
                for (actual, expected) in actual_positions.into_iter().zip(expected_positions) {
                    assert!(
                        (actual - expected).abs() < 0.001,
                        "{name} at z={height_mm}, y={y_mm}: {actual} != {expected}"
                    );
                }
            }
        }
        fixture.finish()?;
    }
    Ok(())
}

#[test]
fn section_comparison_detects_equal_volume_and_bounds_with_a_misplaced_cavity() -> TestResult {
    let fixture = Fixture::new(
        "misplaced-cavity",
        "local a=cad.box({width=10,depth=10,height=10});local b=cad.box({width=4,depth=4,height=4,x=1,y=2,z=2});cad.show('hollow',cad.subtract(a,b))",
    )?;
    fixture.run("--export-model")?;
    let path = fixture.exports().join("model.step");
    let (volume, bounds) = inspect_step(&path)?;
    assert!((volume - 936.0).abs() < 1e-6);
    assert!(bounds[0].abs().max_element() < 1e-6);
    assert!((bounds[1] - DVec3::splat(10.0)).abs().max_element() < 1e-6);
    let actual = Shape::read_step(path)?;
    let reference: Shape = Shape::box_with_dimensions(10.0, 10.0, 10.0)
        .subtract(&Shape::box_from_corners(
            dvec3(3.0, 2.0, 2.0),
            dvec3(7.0, 6.0, 6.0),
        ))
        .into();
    let difference =
        section_difference_area_mm2(&section(&actual, 4.0)?, &section(&reference, 4.0)?);
    assert!((difference - 16.0).abs() < 1e-6);
    fixture.finish()?;
    Ok(())
}

#[test]
fn rejects_self_intersecting_brep_before_measuring_volume() -> TestResult {
    let fixture = Fixture::new("invalid-brep", "")?;
    let wire = Wire::from_ordered_points([
        dvec3(0.0, 0.0, 0.0),
        dvec3(10.0, 10.0, 0.0),
        dvec3(0.0, 10.0, 0.0),
        dvec3(10.0, 0.0, 0.0),
    ])?;
    let shape = Shape::from(Face::from_wire(&wire));
    let path = fixture.directory.join("invalid.brep");
    shape.write_brep_text(&path)?;
    let raw = opencascade_sys::b_rep_tools::read(path.to_string_lossy().into_owned());
    let error = crate::geometry::validate(&raw).expect_err("Reject the bow-tie wire");
    assert!(error.to_string().contains("Invalid STEP B-rep"), "{error}");
    fixture.finish()?;
    Ok(())
}

#[test]
fn rejects_step_surfaces_without_closed_volume() -> TestResult {
    let fixture = Fixture::new("open-surface", "")?;
    let surface = Shape::from(Face::from_wire(&Wire::rect(10.0, 10.0)));
    let path = fixture.directory.join("surface.step");
    surface.write_step(&path)?;
    let error = crate::geometry::volume_mm3(&path).expect_err("A face is not a closed solid");
    assert!(
        error.to_string().contains("STEP volume integration failed"),
        "{error}"
    );
    fixture.finish()?;
    Ok(())
}

#[test]
fn curved_step_exports_reopen_with_matching_volumes_bounds_and_closed_meshes() -> TestResult {
    let bracket = include_str!("../../../examples/mounting-bracket.luau");
    let changed = bracket.replace("cad.number(100,", "cad.number(120,");
    let bushing = include_str!("../../../examples/revolved-bushing.luau").replace(
        "cad.revolve(section, { angle = sweep_angle })",
        "cad.translate(cad.revolve(section, { angle = sweep_angle }), {x=16,y=16})",
    );
    for (index, source, dimensions) in [
        (0, bracket, [100.0, 50.0, 40.0]),
        (1, changed.as_str(), [120.0, 50.0, 40.0]),
        (
            2,
            "local a=cad.box({width=10,depth=10,height=10});local b=cad.box({width=6,depth=6,height=6,x=2,y=2,z=2});cad.show('hollow',cad.subtract(a,b))",
            [10.0; 3],
        ),
        (
            3,
            include_str!("../../../examples/extruded-bracket.luau"),
            [64.0, 42.0, 6.0],
        ),
        (
            4,
            include_str!("../../../examples/edge-fillets.luau"),
            [60.0, 30.0, 8.0],
        ),
        (
            5,
            include_str!("../../../examples/edge-chamfers.luau"),
            [60.0, 30.0, 8.0],
        ),
        (
            6,
            "local p=cad.polygon({{x=0,y=0},{x=10,y=0},{x=10,y=20},{x=0,y=20}});cad.show('revolve',cad.translate(cad.revolve(p,{}),{x=10,y=10}))",
            [20.0; 3],
        ),
        (7, bushing.as_str(), [32.0, 32.0, 20.0]),
        (
            8,
            "local p=cad.polygon({{x=5,y=0},{x=10,y=0},{x=10,y=20},{x=5,y=20}});cad.show('sector',cad.revolve(p,{angle=90}))",
            [10.0, 10.0, 20.0],
        ),
    ] {
        let fixture = Fixture::new(&format!("curve-{index}"), source)?;
        let expected_volume = fixture.volume()?;
        fixture.run("--export-model")?;
        let (volume, bounds) = inspect_step(&fixture.exports().join("model.step"))?;
        assert!(
            (volume - expected_volume).abs() < expected_volume * 0.002,
            "Case {index}: STEP {volume}, native {expected_volume}"
        );
        assert!(
            bounds[0].abs().max_element() < 0.2,
            "Case {index}: {:?}",
            bounds[0]
        );
        assert!(
            (bounds[1] - DVec3::from_array(dimensions))
                .abs()
                .max_element()
                < 0.2
        );
        fixture.finish()?;
    }
    Ok(())
}

#[test]
fn luau_rack_matches_independently_constructed_reference_geometry() -> TestResult {
    for (index, parameters) in [
        crate::PARAMETERS,
        crate::RackParameters {
            rack_width_mm: 300.5,
            step_count: 3,
            step_rise_mm: 70.0,
            thickness_mm: 4.0,
            ..crate::PARAMETERS
        },
    ]
    .into_iter()
    .enumerate()
    {
        let source = include_str!("../fixtures/rack-blank.luau")
            .replace(
                "cad.number(310,",
                &format!("cad.number({},", parameters.rack_width_mm),
            )
            .replace(
                "cad.number(5,",
                &format!("cad.number({},", parameters.step_count),
            )
            .replace(
                "cad.number(42,",
                &format!("cad.number({},", parameters.step_rise_mm),
            )
            .replace(
                "cad.number(3, { label = \"Wall thickness\"",
                &format!(
                    "cad.number({}, {{ label = \"Wall thickness\"",
                    parameters.thickness_mm
                ),
            );
        let fixture = Fixture::new(&format!("rack-blank-{index}"), &source)?;
        let reference = crate::build_rack(parameters)?.mesh()?;
        let volume: f64 = reference
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|triangle| {
                let [a, b, c] = triangle.map(|index| reference.vertices[index]);
                a.dot(b.cross(c)) / 6.0
            })
            .sum();
        assert!((fixture.volume()? - volume).abs() < volume * 1e-6);
        fixture.finish()?;
    }
    let fixture = Fixture::new(
        "rack-parts",
        include_str!("../../../examples/rack-parts.luau"),
    )?;
    fixture.run("--export-model")?;
    let reference = crate::printing::build_parts(crate::PARAMETERS)?;
    for (index, part) in reference.iter().enumerate() {
        let path = fixture
            .exports()
            .join(format!("part-{:02}.step", index + 1));
        let (volume, bounds) = inspect_step(&path)?;
        let mesh = part.shape.mesh()?;
        let expected: f64 = mesh
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|triangle| {
                let [a, b, c] = triangle.map(|index| mesh.vertices[index]);
                a.dot(b.cross(c)) / 6.0
            })
            .sum();
        assert!((volume - expected).abs() < 0.01, "Rack part {index}");
        assert!(bounds[0].z.abs() < 0.001);
        let (_, dimensions) = part.on_bed()?;
        assert!((bounds[1] - bounds[0] - dimensions).abs().max_element() < 0.001);
    }
    fixture.finish()?;
    Ok(())
}

#[test]
fn step_exports_use_print_placements_instead_of_assembly_placements() -> TestResult {
    let source = "local b=cad.box({width=10,depth=10,height=2});cad.show('a',b,{assembly={position={z=40}},print={}});cad.show('b',cad.translate(b,{x=0}),{assembly={position={x=30,z=40}},print={position={x=40}}})";
    let fixture = Fixture::new("placements", source)?;
    fixture.run("--export-model")?;
    for index in 1..=2 {
        let (volume, bounds) =
            inspect_step(&fixture.exports().join(format!("part-{index:02}.step")))?;
        assert!((volume - 200.0).abs() < 0.001);
        assert!(bounds[0].z.abs() < 0.001);
        assert!(
            (bounds[1] - bounds[0] - dvec3(10.0, 10.0, 2.0))
                .abs()
                .max_element()
                < 0.001
        );
        assert!((bounds[0].x - (index - 1) as f64 * 40.0).abs() < 0.001);
    }
    fixture.finish()?;
    Ok(())
}

#[test]
fn multipart_step_exports_preserve_native_volumes_and_part_counts() -> TestResult {
    for (name, source) in [
        (
            "rocket",
            include_str!("../../../examples/rocket-engine.luau"),
        ),
        ("loft", include_str!("../../../examples/curved-loft.luau")),
        ("sweep", include_str!("../../../examples/smooth-sweep.luau")),
    ] {
        let fixture = Fixture::new(name, source)?;
        let expected = fixture.volume()?;
        fixture.run("--export-model")?;
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(fixture.exports().join("parts.json"))?)?;
        let parts = manifest.as_array().ok_or("Missing parts manifest")?;
        assert!(!parts.is_empty());
        assert!(parts.len() <= 32);
        if name == "rocket" {
            assert_eq!(parts.len(), 3);
        }
        let mut total = 0.0;
        for part in parts {
            let path = fixture
                .exports()
                .join(part["step"].as_str().ok_or("Missing STEP file")?);
            let (volume, bounds) = inspect_step(&path)?;
            assert!(
                bounds[0].z.abs() < 0.001,
                "{name} {}: {:?}",
                part["name"],
                bounds[0]
            );
            total += volume;
        }
        assert!(
            (total - expected).abs() < expected * 0.002,
            "{name}: STEP {total}, native {expected}"
        );
        fixture.finish()?;
    }
    Ok(())
}
