//! Native executable entry point.
mod actions;
mod app;
mod document;
mod main_window;
mod services;
mod settings;
mod welcome;
mod workbench;
use services::{prefs, recent, startup};
mod gpu_demo;
mod i18n;
#[cfg(target_os = "windows")]
mod panels;
mod theme;
#[cfg(target_os = "windows")]
use panels::inspector;
#[cfg(target_os = "windows")]
mod native_gpu_demo;
#[cfg(target_os = "windows")]
mod viewport;

fn main() {
    app::run();
}
