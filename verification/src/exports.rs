use glam::{DVec3, dvec3};
use opencascade::primitives::Shape;
use std::{
    collections::BTreeMap,
    error::Error,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

type TestResult = Result<(), Box<dyn Error>>;
const TRIANGLE_COUNT_MAX: usize = 100_000;

struct Fixture {
    directory: PathBuf,
}

impl Fixture {
    fn new(name: &str, source: &str) -> Result<Self, Box<dyn Error>> {
        let directory =
            std::env::temp_dir().join(format!("spice-verification-{}-{name}", std::process::id()));
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
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/debug/spice-cad")
            });
        let mut command = Command::new(&binary);
        command
            .arg(flag)
            .arg(self.source())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
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
        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(format!(
                "{flag} failed: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        Ok(String::from_utf8(output.stdout)?)
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
}

impl Drop for Fixture {
    fn drop(&mut self) {
        match std::fs::remove_dir_all(&self.directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => eprintln!("Verification: temporary directory cleanup failed: {error}"),
        }
    }
}

fn inspect_step(path: &Path) -> Result<(f64, [DVec3; 2]), Box<dyn Error>> {
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
    Ok((volume_mm3, bounds))
}

#[test]
fn curved_step_exports_reopen_with_matching_volumes_bounds_and_closed_meshes() -> TestResult {
    let bracket = include_str!("../../models/mounting-bracket.luau");
    let changed = bracket.replace("cad.number(100,", "cad.number(120,");
    let bushing = include_str!("../../models/revolved-bushing.luau").replace(
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
        let source = include_str!("../../models/rack.luau")
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
    }
    let fixture = Fixture::new("rack-parts", include_str!("../../models/rack-parts.luau"))?;
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
    Ok(())
}

#[test]
fn multipart_step_exports_preserve_native_volumes_and_part_counts() -> TestResult {
    for (name, source) in [
        ("rocket", include_str!("../../models/rocket-engine.luau")),
        ("loft", include_str!("../../models/curved-loft.luau")),
        ("sweep", include_str!("../../models/smooth-sweep.luau")),
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
    }
    Ok(())
}
