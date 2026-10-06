//! Native executable entry point.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod actions;
mod app;
mod assets;
mod document;
mod i18n;
mod main_window;
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
mod native_gpu_demo;
mod panels;
mod services;
mod settings;
mod theme;
mod tooltips;
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
mod viewport;
mod welcome;
#[cfg(feature = "window-profiling")]
mod window_timing;
mod workbench;

fn main() -> std::process::ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() == Some(std::ffi::OsStr::new("--version"))
        && arguments.next().is_none()
    {
        println!("Pomelo {}", env!("CARGO_PKG_VERSION"));
        return std::process::ExitCode::SUCCESS;
    }
    app::run()
}
