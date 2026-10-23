use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs,
    io::{BufReader, Cursor, Read},
    path::Path,
    process::{Command, ExitStatus},
    time::Duration,
};

fn main() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<_> = std::env::args().skip(1).take(6).collect();
    let [native, validator, game, baker, output] = arguments.as_slice() else {
        return Err(
            "Usage: infantry-bakes SPICE_BINARY VALIDATOR ISOLATED_GAME_CHECKOUT BAKER NEW_OUTPUT"
                .into(),
        );
    };
    let native = Path::new(native).canonicalize()?;
    let validator = Path::new(validator).canonicalize()?;
    let baker = Path::new(baker).canonicalize()?;
    let game = Path::new(game).canonicalize()?;
    if !game.join(".git").is_file() {
        return Err("Use an isolated git worktree, not the production game checkout".into());
    }
    fs::create_dir(output)?;
    let output = Path::new(output).canonicalize()?;
    let source = include_str!("../../../examples/infantry.luau");
    let marker = "local stride: number = cad.number(0,";
    if source.matches(marker).count() != 1 {
        return Err("Infantry stride declaration changed; update the bake recipe".into());
    }
    for (pose, stride) in [("idle", 0), ("step-left", 1), ("step-right", -1)] {
        let source = source.replacen(
            marker,
            &format!("local stride: number = cad.number({stride},"),
            1,
        );
        let snapshot = output.join(format!("{pose}.luau"));
        fs::write(&snapshot, source)?;
        let bundle = output.join(format!("infantry-{pose}"));
        let status = run(Command::new(&native)
            .arg("--export-game")
            .arg(snapshot)
            .arg(&bundle))?;
        if !status.success() {
            return Err(format!("Export failed for {pose}: {status}").into());
        }
        let status = run(Command::new(&validator)
            .arg(&bundle)
            .arg(bundle.join("target-loader.json")))?;
        if !status.success() {
            return Err(format!("Strict validation failed for {pose}: {status}").into());
        }
        let target = game
            .join("art/source/spice")
            .join(format!("infantry-{pose}"));
        fs::create_dir(&target)?;
        for file in ["model.glb", "asset.json"] {
            fs::copy(bundle.join(file), target.join(file))?;
        }
    }
    let status = run(Command::new(&baker)
        .env_remove("DIORAMA_SPICE")
        .env("DIORAMA_INFANTRY", "1")
        .env("DIORAMA_EXPORT", output.join("bake")))?;
    if !status.success() {
        return Err(format!("Sprite baking failed: {status}").into());
    }
    fs::create_dir(output.join("units"))?;
    let mut audit = Vec::with_capacity(6);
    for facing in ["", "-back"] {
        let mut frames = Vec::with_capacity(3);
        for pose in ["", "-step-left", "-step-right"] {
            let name = format!("levy{facing}{pose}.png");
            let mut bytes = Vec::new();
            fs::File::open(output.join("bake/units").join(&name))?
                .take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 1024 * 1024 {
                return Err("Sprite exceeds 1 MiB".into());
            }
            let mut reader = png::Decoder::new(BufReader::new(Cursor::new(&bytes))).read_info()?;
            if reader.info().width != 128
                || reader.info().height != 128
                || reader.info().color_type != png::ColorType::Rgba
                || reader.info().bit_depth != png::BitDepth::Eight
            {
                return Err(format!("Expected 128x128 RGBA8: {name}").into());
            }
            let mut pixels = vec![0; 128 * 128 * 4];
            reader.next_frame(&mut pixels)?;
            let mut bounds = [128, 128, 0, 0];
            for y in 0..128 {
                for x in 0..128 {
                    if pixels[(y * 128 + x) * 4 + 3] != 0 {
                        if x == 0 || y == 0 || x == 127 || y == 127 {
                            return Err(format!("Clipped sprite: {name}").into());
                        }
                        bounds[0] = bounds[0].min(x);
                        bounds[1] = bounds[1].min(y);
                        bounds[2] = bounds[2].max(x + 1);
                        bounds[3] = bounds[3].max(y + 1);
                    }
                }
            }
            if bounds[2] == 0 {
                return Err(format!("Empty sprite: {name}").into());
            }
            if frames.iter().any(|frame| frame == &pixels) {
                return Err(format!("Duplicate pose: {name}").into());
            }
            audit.push(serde_json::json!({"file":name,"sha256":format!("{:x}",Sha256::digest(&bytes)),"alpha_bounds":bounds,"size":[128,128],"nontransparent_border_pixels":0}));
            frames.push(pixels);
            fs::copy(
                output.join("bake/units").join(&name),
                output.join("units").join(name),
            )?;
        }
    }
    fs::write(
        output.join("pixel-audit.json"),
        serde_json::to_vec_pretty(&audit)?,
    )?;
    Ok(())
}

fn run(command: &mut Command) -> Result<ExitStatus, Box<dyn Error>> {
    let mut child = command.spawn()?;
    for _ in 0..4000 {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    child.kill()?;
    child.wait()?;
    Err("Bake subprocess exceeded 120 seconds".into())
}
