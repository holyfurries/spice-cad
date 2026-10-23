use std::{env, error::Error, ffi::OsString, path::Path, process::Command};

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("Packaged verification: {error}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<i32, Box<dyn Error>> {
    let arguments: Vec<OsString> = env::args_os().skip(1).take(4).collect();
    let valid = match arguments.as_slice() {
        [flag, _] => flag == "--check-model",
        [flag, _, _] => {
            flag == "--export-model" || flag == "--export-game" || flag == "--export-game-poses"
        }
        _ => false,
    };
    if !valid {
        return Err("Expected --check-model SOURCE, or --export-model/--export-game/--export-game-poses SOURCE DIRECTORY".into());
    }
    let container = env::var("SPICE_CAD_CONTAINER")?;
    if container.is_empty() || container.starts_with('-') {
        return Err("Invalid verification container name".into());
    }
    let owner = env::var("SPICE_CAD_OWNER")?;
    let (user, group) = owner.split_once(':').ok_or("Expected numeric UID:GID")?;
    user.parse::<u32>()?;
    group.parse::<u32>()?;
    let status = Command::new("docker")
        .args(["exec", "--env", "APPDIR=/app/squashfs-root", &container])
        .args([
            "timeout",
            "--signal=KILL",
            "110",
            "/app/squashfs-root/AppRun",
        ])
        .args(&arguments)
        .status()?;
    match arguments.as_slice() {
        [_, _, output] if Path::new(output).try_exists()? => {
            let ownership = Command::new("docker")
                .args(["exec", &container, "chown", "-R", &owner, "--"])
                .arg(output)
                .status()?;
            if !ownership.success() {
                return Err(format!("Cannot restore export ownership: {ownership}").into());
            }
        }
        _ => {}
    }
    Ok(status.code().unwrap_or(1))
}
