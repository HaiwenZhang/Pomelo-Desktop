//! Native executable entry point.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod actions;
mod app;
mod assets;
mod document;
mod i18n;
mod main_window;
mod panels;
mod services;
mod settings;
mod shortcuts;
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
    // Experimental solidcore comparison (docs/pcb-3d-solidcore-experiment-plan.md):
    // POMELO_3D_BACKEND=kernel-thread|kernel-ops builds 3D solid meshes through the kernel.
    #[cfg(feature = "kernel-3d")]
    pomelo_render::scene::kernel3d::Backend3d::from_env()
        .install(pomelo_render::scene::board3d::PreviewStackup::default().chord_mm);
    app::run()
}
