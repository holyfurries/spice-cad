use std::{
    error::Error,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<String> = std::env::args().skip(1).take(4).collect();
    if !(2..=3).contains(&arguments.len()) {
        return Err(
            "Usage: profile-native EXECUTABLE [COMPARISON_EXECUTABLE] NEW_OUTPUT_DIRECTORY".into(),
        );
    }
    let executables = arguments[..arguments.len() - 1]
        .iter()
        .map(fs::canonicalize)
        .collect::<Result<Vec<_>, _>>()?;
    let directory = &arguments[arguments.len() - 1];
    fs::create_dir(directory)?;
    let directory = fs::canonicalize(directory)?;
    if Path::new("/tmp/.X11-unix/X109").exists() {
        return Err("Display :109 is occupied; choose a free display in the harness".into());
    }
    let mut server = OwnedProcess(
        Command::new("Xvfb")
            .args([":109", "-screen", "0", "1600x1000x24", "-nolisten", "tcp"])
            .stdout(Stdio::null())
            .stderr(File::create(directory.join("xvfb.log"))?)
            .spawn()?,
    );
    for _ in 0..100 {
        if Path::new("/tmp/.X11-unix/X109").exists() {
            break;
        }
        if server.0.try_wait()?.is_some() {
            return Err("Xvfb exited".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    if !Path::new("/tmp/.X11-unix/X109").exists() {
        return Err("Xvfb startup timed out".into());
    }
    let ticks_per_second: f64 = command("getconf", &["CLK_TCK"])?.trim().parse()?;
    let mut reports = Vec::new();
    let mut expected_adapter = None;
    for sample in 0..3 {
        for (variant, executable) in executables.iter().enumerate() {
            let log = directory.join(format!("sample-{sample}-variant-{variant}.log"));
            let started = Instant::now();
            let mut app = OwnedProcess(
                Command::new(executable)
                    .arg("--profile")
                    .current_dir(&directory)
                    .env("DISPLAY", ":109")
                    .env_remove("WAYLAND_DISPLAY")
                    .env("XDG_DATA_HOME", directory.join("data"))
                    .stdout(Stdio::null())
                    .stderr(File::create(&log)?)
                    .spawn()?,
            );
            let mut first_ui_ms = None;
            let mut model_upload_ms = None;
            for _ in 0..1500 {
                let text = read(&log)?;
                if first_ui_ms.is_none() && text.contains("Profile: first_ui ") {
                    first_ui_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
                }
                if text.contains("Profile: first_model_upload ") {
                    model_upload_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
                    break;
                }
                if app.0.try_wait()?.is_some() {
                    return Err(format!("Application exited: {text}").into());
                }
                thread::sleep(Duration::from_millis(10));
            }
            let first_ui_ms = first_ui_ms.ok_or("First UI timed out")?;
            let model_upload_ms = model_upload_ms.ok_or("Model upload timed out")?;
            let text = read(&log)?;
            let adapter = text
                .lines()
                .find_map(|line| line.strip_prefix("Graphics: "))
                .ok_or("Missing graphics adapter")?;
            if adapter.chars().any(char::is_control) {
                return Err("Invalid graphics adapter name".into());
            }
            if let Some(expected) = &expected_adapter {
                if expected != adapter {
                    return Err("Graphics adapter changed between profiling runs".into());
                }
            } else {
                expected_adapter = Some(adapter.to_owned());
            }
            let adapter_json = adapter.replace('\\', "\\\\").replace('"', "\\\"");
            thread::sleep(Duration::from_millis(500));
            let pid = app.0.id().to_string();
            let status = read(&PathBuf::from(format!("/proc/{pid}/status")))?;
            let rss_kib: u64 = status
                .lines()
                .find_map(|line| line.strip_prefix("VmRSS:"))
                .ok_or("Missing RSS")?
                .split_whitespace()
                .next()
                .ok_or("Invalid RSS")?
                .parse()?;
            let idle_started = Instant::now();
            let ticks_before = cpu_ticks(&pid)?;
            thread::sleep(Duration::from_secs(2));
            let ticks = cpu_ticks(&pid)?
                .checked_sub(ticks_before)
                .ok_or("CPU counter moved backwards")?;
            let idle_cpu_percent =
                ticks as f64 / ticks_per_second / idle_started.elapsed().as_secs_f64() * 100.0;
            let window = command(
                "xdotool",
                &["search", "--all", "--pid", &pid, "--name", "Spice CAD"],
            )?;
            let window = window.lines().next().ok_or("Missing CAD window")?;
            command("xdotool", &["windowfocus", window])?;
            command("xdotool", &["key", "ctrl+Return"])?;
            for index in 0..40 {
                let width = (1100 + index % 2 * 200).to_string();
                let height = (650 + index % 2 * 120).to_string();
                command("xdotool", &["windowsize", window, &width, &height])?;
                thread::sleep(Duration::from_millis(20));
            }
            thread::sleep(Duration::from_millis(400));
            let text = read(&log)?;
            let resize: Vec<&str> = text
                .lines()
                .filter(|line| line.starts_with("Profile: resize "))
                .collect();
            if resize.len() < 5 {
                return Err("Too few resize updates".into());
            }
            let uploads_max = resize
                .iter()
                .filter_map(|line| value(line, "uploads"))
                .fold(0.0, f64::max);
            if uploads_max > 2.0 {
                return Err("Resizing uploaded geometry again".into());
            }
            let resize_before_rebuild = resize
                .iter()
                .filter(|line| value(line, "uploads") == Some(1.0))
                .count();
            if resize_before_rebuild == 0 {
                return Err("No resize update before rebuilt geometry uploaded".into());
            }
            let mut cpu_ms: Vec<f64> = resize
                .iter()
                .filter_map(|line| value(line, "ui_cpu_ms"))
                .collect();
            cpu_ms.sort_by(f64::total_cmp);
            let cpu_p95_ms = cpu_ms[(cpu_ms.len() * 95 / 100).min(cpu_ms.len() - 1)];
            let phases = [
                "before_event_loop",
                "adapter_selected",
                "device_and_egui_ready",
                "viewport_ready",
                "first_ui",
            ]
            .iter()
            .filter_map(|name| {
                let prefix = format!("Profile: {name} ");
                text.lines()
                    .find(|line| line.starts_with(&prefix))
                    .and_then(|line| value(line, "elapsed_ms"))
                    .map(|elapsed| format!("\"{name}\":{elapsed}"))
            })
            .collect::<Vec<_>>()
            .join(",");
            reports.push(format!("{{\"sample\":{sample},\"variant\":{variant},\"adapter\":\"{adapter_json}\",\"startup_ms\":{{{phases}}},\"first_ui_observed_ms\":{first_ui_ms:.3},\"model_upload_observed_ms\":{model_upload_ms:.3},\"rss_kib\":{rss_kib},\"idle_cpu_percent\":{idle_cpu_percent:.3},\"resize_frames\":{},\"resize_frames_before_rebuild_upload\":{resize_before_rebuild},\"resize_ui_cpu_p95_ms\":{cpu_p95_ms:.3},\"geometry_uploads_max\":{uploads_max}}}", resize.len()));
            println!(
                "Sample {sample} variant {variant}: UI {first_ui_ms:.1} ms, model {model_upload_ms:.1} ms, RSS {rss_kib} KiB, resize {} frames",
                resize.len()
            );
        }
    }
    fs::write(
        directory.join("results.json"),
        format!(
            "{{\n\"method\":\"Three launches per executable, alternating variants when comparing two builds; OS caches not flushed. Spawn through observed stderr marker, sampled every 10 ms. Model marker follows CPU GPU-buffer creation. Resize is X11 windowsize requests during a rebuild. UI CPU excludes renderer, GPU completion and presentation. RSS includes mapped graphics-driver pages; excludes child processes and GPU allocations. Idle CPU is one logical core over two seconds after a 500 ms settling interval. Adapter identity is checked across all runs. Startup phases are milliseconds since main entry.\",\n\"samples\":[{}]\n}}\n",
            reports.join(",\n")
        ),
    )?;
    Ok(())
}

struct OwnedProcess(Child);
unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        match self.0.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {}
            Err(error) => eprintln!("Profile: process status: {error}"),
        }
        if let Ok(pid) = i32::try_from(self.0.id()) {
            if unsafe { kill(pid, 15) } != 0 {
                eprintln!(
                    "Profile: process termination: {}",
                    std::io::Error::last_os_error()
                );
            }
            for _ in 0..100 {
                match self.0.try_wait() {
                    Ok(Some(_)) => return,
                    Ok(None) => thread::sleep(Duration::from_millis(10)),
                    Err(_) => break,
                }
            }
        }
        if let Err(error) = self.0.kill() {
            eprintln!("Profile: process cleanup: {error}");
        }
        if let Err(error) = self.0.wait() {
            eprintln!("Profile: process wait: {error}");
        }
    }
}

fn command(program: &str, arguments: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new(program)
        .args(arguments)
        .env("DISPLAY", ":109")
        .output()?;
    if !output.status.success() {
        return Err(format!("{program}: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn read(path: &Path) -> Result<String, Box<dyn Error>> {
    let mut text = String::new();
    File::open(path)?.take(65536).read_to_string(&mut text)?;
    Ok(text)
}

fn cpu_ticks(pid: &str) -> Result<u64, Box<dyn Error>> {
    let stat = read(&PathBuf::from(format!("/proc/{pid}/stat")))?;
    let (_, fields) = stat.rsplit_once(')').ok_or("Invalid process stat")?;
    let fields: Vec<&str> = fields.split_whitespace().collect();
    let user: u64 = fields.get(11).ok_or("Missing user CPU")?.parse()?;
    let system: u64 = fields.get(12).ok_or("Missing system CPU")?.parse()?;
    user.checked_add(system)
        .ok_or_else(|| "CPU counter overflow".into())
}

fn value(line: &str, name: &str) -> Option<f64> {
    line.split_whitespace()
        .filter_map(|part| part.split_once('='))
        .find_map(|(key, value)| {
            if key == name {
                value.parse().ok()
            } else {
                None
            }
        })
}
