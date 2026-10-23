use serde_json::Value;
use std::path::{Path, PathBuf};

#[test]
#[ignore = "requires a built Spice CAD executable; run with --ignored"]
fn exported_fixtures_pass_strict_target_loader_and_keep_transform_contracts()
-> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let native = std::env::var_os("SPICE_CAD_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/debug/spice-cad"));
    if !native.is_file() {
        return Err("Build Spice CAD first or set SPICE_CAD_BIN".into());
    }
    let output = std::env::var_os("SPICE_IMPORT_ARTIFACTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("output/game-import-acceptance"));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let output = output.join(format!("run-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&output)?;
    for (name, source) in [
        (
            "farmstead",
            include_str!("../../../examples/farmstead.luau"),
        ),
        (
            "facing-marker",
            include_str!("../fixtures/facing-marker.luau"),
        ),
        ("equipment", include_str!("../fixtures/equipment.luau")),
    ] {
        for (variant, options) in [
            ("default", ""),
            (
                "preset",
                "cad.model_options({game={preset='crowns_adrift',scale=10}})",
            ),
            (
                "rotated",
                "cad.model_options({rotation={x=90,z=20},game={forward='x+',up='y-',scale=3,ground=true}})",
            ),
        ] {
            let stem = format!("{name}-{variant}");
            let path = output.join(format!("{stem}.luau"));
            let source = format!(
                "--!strict\n{options}\n{}",
                source.strip_prefix("--!strict\n").unwrap_or(source)
            );
            std::fs::write(&path, source)?;
            let bundle = output.join(&stem);
            run(
                std::process::Command::new(&native)
                    .arg("--export-game")
                    .arg(&path)
                    .arg(&bundle),
                &output.join(format!("{stem}-export.log")),
            )?;
            let report = output.join(format!("{stem}-loader.json"));
            run(
                std::process::Command::new(env!("CARGO_BIN_EXE_spice-game-import"))
                    .arg(&bundle)
                    .arg(&report),
                &output.join(format!("{stem}-loader.log")),
            )?;
            let metadata: Value =
                serde_json::from_slice(&std::fs::read(bundle.join("asset.json"))?)?;
            let report: Value = serde_json::from_slice(&std::fs::read(report)?)?;
            assert_eq!(report["result"], "passed");
            assert_eq!(report["validation_enabled"], true);
            assert_eq!(report["glb_sha256"], metadata["files"]["glb"]["sha256"]);
            if variant != "default" {
                assert!(metadata["bounds"]["minimum"][1].as_f64().unwrap().abs() < 1e-6);
            }
            if name == "farmstead" && variant == "preset" {
                assert_eq!(metadata["display"]["additional_scale"], 1.0);
                assert!((metadata["bounds"]["size"][0].as_f64().unwrap() - 1.15).abs() < 1e-6);
            }
            if name == "farmstead" && variant != "rotated" {
                let instructions = std::fs::read_to_string(bundle.join("GAME_ASSET.md"))?;
                assert!(instructions.contains(if variant == "preset" {
                    "Additional uniform display scale: 1.000000"
                } else {
                    "Additional uniform display scale: 10.000000"
                }));
            }
        }
    }
    println!("Acceptance bundles and reports: {}", output.display());
    Ok(())
}

fn run(command: &mut std::process::Command, log: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        process::Stdio,
        time::{Duration, Instant},
    };
    let output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(log)?;
    let mut child = command
        .stdout(Stdio::from(output.try_clone()?))
        .stderr(Stdio::from(output))
        .spawn()?;
    let started = Instant::now();
    for _ in 0..1200 {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(());
            }
            return Err(format!("Command failed ({status}); see {}", log.display()).into());
        }
        if started.elapsed() > Duration::from_secs(120) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    child.kill()?;
    child.wait()?;
    Err(format!("Command exceeded 120 seconds; see {}", log.display()).into())
}
