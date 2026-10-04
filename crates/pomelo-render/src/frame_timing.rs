//! Opt-in local window diagnostics. Cadence measures callbacks, not scanout/presented FPS.
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{SyncSender, sync_channel},
    },
    time::Instant,
};

#[derive(serde::Deserialize)]
struct Config {
    output: PathBuf,
    #[serde(default)]
    continuous: bool,
}

struct Profiler {
    epoch: Instant,
    sender: SyncSender<serde_json::Value>,
    continuous: bool,
    continuous_marker: Option<PathBuf>,
    disabled: AtomicBool,
}
static PROFILER: OnceLock<Option<Profiler>> = OnceLock::new();

fn profiler() -> Option<&'static Profiler> {
    PROFILER
        .get_or_init(|| {
            let mut marker = None;
            let config = if let Some(output) = std::env::var_os("POMELO_GUI_FRAME_PROFILE") {
                Config {
                    output: output.into(),
                    continuous: false,
                }
            } else {
                // An opt-in sibling marker allows GUI launch through the same exe
                // without modifying the user's environment or creating another app id.
                if !cfg!(any(debug_assertions, feature = "frame-profiling")) {
                    return None;
                }
                let path = std::env::current_exe()
                    .ok()?
                    .with_extension("frame-profile.json");
                marker = Some(path.clone());
                serde_json::from_slice::<Config>(&std::fs::read(path).ok()?).ok()?
            };
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(config.output)
                .ok()?;
            let (sender, receiver) = sync_channel::<serde_json::Value>(256);
            std::thread::Builder::new()
                .name("frame-profile-writer".into())
                .spawn(move || {
                    let mut file = BufWriter::new(file);
                    while let Ok(event) = receiver.recv() {
                        if serde_json::to_writer(&mut file, &event).is_err()
                            || file.write_all(b"\n").is_err()
                            || file.flush().is_err()
                        {
                            break;
                        }
                    }
                })
                .ok()?;
            Some(Profiler {
                epoch: Instant::now(),
                sender,
                continuous: config.continuous,
                continuous_marker: marker,
                disabled: AtomicBool::new(false),
            })
        })
        .as_ref()
}

/// Starts a timer only when local diagnostics are explicitly enabled.
pub fn begin() -> Option<Instant> {
    active().map(|_| Instant::now())
}

fn active() -> Option<&'static Profiler> {
    let profile = profiler()?;
    if profile.disabled.load(Ordering::Relaxed) {
        return None;
    }
    if profile
        .continuous_marker
        .as_ref()
        .is_some_and(|path| !path.exists())
    {
        profile.disabled.store(true, Ordering::Relaxed);
        return None;
    }
    Some(profile)
}

/// A diagnostic marker can request continuous full-window repaint for cadence sampling.
pub fn continuous() -> bool {
    active().is_some_and(|profile| profile.continuous)
}

/// Enqueues a small record without disk I/O or waiting in the UI/render callback.
/// Payload construction is skipped when disabled; a full channel drops records.
pub fn record(
    name: &'static str,
    started: Option<Instant>,
    payload: impl FnOnce() -> serde_json::Value,
) {
    let Some(started) = started else {
        return;
    };
    let Some(profile) = profiler() else {
        return;
    };
    let elapsed = started.elapsed().as_micros();
    let _ = profile.sender.try_send(serde_json::json!({
        "event":name, "pid":std::process::id(),
        "at_us":started.duration_since(profile.epoch).as_micros(),
        "elapsed_us":elapsed, "data":payload(),
    }));
}
