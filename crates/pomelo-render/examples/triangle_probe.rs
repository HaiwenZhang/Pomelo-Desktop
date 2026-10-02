//! Hardware validation executable. JSON is stable machine data; errors are localized.
use std::{path::PathBuf, process::ExitCode};

use pomelo_core::i18n::{Locale, MessageKey, text};
use pomelo_render::triangle::{ProbeBackend, render_triangle};

fn main() -> ExitCode {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let locale = Locale::English;
    let (backend, output, width, height) = match parse_arguments(&arguments) {
        Ok(options) => options,
        Err(details) => {
            eprintln!(
                "{}\n{}: {details}",
                text(locale, MessageKey::GpuProbeUsage),
                text(locale, MessageKey::TechnicalDetails)
            );
            return ExitCode::FAILURE;
        }
    };
    let frame = match render_triangle(backend, width, height) {
        Ok(frame) => frame,
        Err(error) => {
            let diagnostic = error.diagnostic();
            eprintln!(
                "{}\n{}: {error}",
                diagnostic.message.display(locale),
                text(locale, MessageKey::TechnicalDetails)
            );
            return ExitCode::FAILURE;
        }
    };
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        image::save_buffer_with_format(
            &output,
            &frame.rgba,
            width,
            height,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )?;
        let report = serde_json::to_string_pretty(&frame.report)?;
        std::fs::write(output.with_extension("json"), &report)?;
        println!("{report}");
        Ok(())
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!(
                "{}\n{}: {error}",
                text(locale, MessageKey::FileIoFailed),
                text(locale, MessageKey::TechnicalDetails)
            );
            ExitCode::FAILURE
        }
    }
}

fn parse_arguments(
    arguments: &[String],
) -> Result<(ProbeBackend, PathBuf, u32, u32), &'static str> {
    if arguments.len() > 4 {
        return Err("GPU_PROBE_ARGUMENT_COUNT");
    }
    let backend = match arguments.first().map(String::as_str) {
        Some("dx12") => ProbeBackend::Dx12,
        Some("vulkan") => ProbeBackend::Vulkan,
        Some("metal") => ProbeBackend::Metal,
        None => ProbeBackend::native(),
        Some(_) => return Err("GPU_PROBE_BACKEND_INVALID"),
    };
    let output = PathBuf::from(
        arguments
            .get(1)
            .map(String::as_str)
            .unwrap_or(".cache/gpu-triangle.png"),
    );
    if output
        .extension()
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("png"))
    {
        return Err("GPU_PROBE_OUTPUT_REQUIRES_PNG");
    }
    let width = arguments
        .get(2)
        .map(|value| value.parse())
        .transpose()
        .map_err(|_| "GPU_PROBE_WIDTH_INVALID")?
        .unwrap_or(768);
    let height = arguments
        .get(3)
        .map(|value| value.parse())
        .transpose()
        .map_err(|_| "GPU_PROBE_HEIGHT_INVALID")?
        .unwrap_or(512);
    Ok((backend, output, width, height))
}
