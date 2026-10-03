//! Native executable entry point.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod actions;
mod app;
mod assets;
mod document;
mod main_window;
mod services;
mod settings;
mod welcome;
mod workbench;
use services::{prefs, recent, startup};
mod i18n;
mod panels;
mod theme;
mod tooltips;
#[cfg(target_os = "windows")]
use panels::inspector;
#[cfg(target_os = "windows")]
mod native_gpu_demo;
#[cfg(target_os = "windows")]
mod viewport;

fn main() -> std::process::ExitCode {
    app::run()
}
