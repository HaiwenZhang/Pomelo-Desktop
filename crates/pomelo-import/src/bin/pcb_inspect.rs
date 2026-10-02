//! Headless inspection with stable machine diagnostics and localized summaries.
use pomelo_core::{
    i18n::{Locale, MessageKey as Key, text},
    model::Diagnostic,
    task::CancellationToken,
};
use pomelo_import::{
    ImportContext, ImportError, ImportOptions, TextEncoding,
    allegro::{
        header::{BrdHeader, HEADER_BYTES},
        index::{BrdIndex, IndexLimits, IndexSummary},
    },
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File},
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Serialize)]
struct CaseResult {
    schema_version: u32,
    path: PathBuf,
    bytes: u64,
    sha256: Option<String>,
    stage: &'static str,
    status: &'static str,
    header: Option<BrdHeader>,
    index: Option<IndexSummary>,
    index_data_path: Option<PathBuf>,
    error: Option<Diagnostic>,
    locale: &'static str,
    localized_message: Option<String>,
    encoding: &'static str,
}

#[derive(Clone, Copy)]
enum Stage {
    Header,
    Index,
}
impl Stage {
    fn name(self) -> &'static str {
        match self {
            Self::Header => "header",
            Self::Index => "index",
        }
    }
}

fn main() -> ExitCode {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    let mut locale = Locale::English;
    match extract_locale(&mut args, &mut locale)
        .and_then(|()| extract_encoding(&mut args))
        .and_then(|encoding| {
            run(
                &args,
                locale,
                &ImportOptions {
                    text_encoding: encoding,
                    ..ImportOptions::default()
                },
            )
        }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", error.message.display(locale));
            if let Some(path) = &error.path {
                eprintln!("{}", path.display());
            }
            if let Some(details) = &error.technical_details {
                eprintln!("{}: {details}", text(locale, Key::TechnicalDetails));
            }
            ExitCode::FAILURE
        }
    }
}

fn extract_locale(args: &mut Vec<OsString>, locale: &mut Locale) -> Result<(), Diagnostic> {
    if let Some(index) = args.iter().position(|arg| arg == "--locale") {
        let value = args
            .get(index + 1)
            .ok_or_else(|| missing_value("--locale"))?;
        let tag = value.to_string_lossy();
        *locale = Locale::ALL
            .into_iter()
            .find(|locale| locale.tag() == tag)
            .ok_or_else(|| {
                let mut error =
                    Diagnostic::error("CLI_UNSUPPORTED_LOCALE", Key::CliUnsupportedLocale);
                error.message = error.message.arg("locale", tag.as_ref());
                error
            })?;
        args.drain(index..index + 2);
        if args.iter().any(|arg| arg == "--locale") {
            return Err(usage());
        }
    }
    Ok(())
}

fn extract_encoding(args: &mut Vec<OsString>) -> Result<TextEncoding, Diagnostic> {
    let Some(index) = args.iter().position(|arg| arg == "--encoding") else {
        return Ok(TextEncoding::default());
    };
    let value = args
        .get(index + 1)
        .ok_or_else(|| missing_value("--encoding"))?
        .to_string_lossy();
    let encoding = TextEncoding::from_tag(&value).ok_or_else(|| {
        let mut error = Diagnostic::error("CLI_UNSUPPORTED_ENCODING", Key::CliUnsupportedEncoding);
        error.message = error.message.arg("encoding", value.as_ref());
        error
    })?;
    args.drain(index..index + 2);
    if args.iter().any(|arg| arg == "--encoding") {
        return Err(usage());
    }
    Ok(encoding)
}

fn run(args: &[OsString], locale: Locale, options: &ImportOptions) -> Result<(), Diagnostic> {
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("{}", text(locale, Key::CliUsage));
        return Ok(());
    }
    let Some(command) = args.first().and_then(|arg| arg.to_str()) else {
        return Err(usage());
    };
    match command {
        "decode-scene" if args.len() >= 2 => {
            validate_options(&args[2..], &["--report"])?;
            let report = option(&args[2..], "--report")?;
            if report
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("brd"))
            {
                return Err(usage());
            }
            scene_probe(Path::new(&args[1]), &report, locale, options)?;
        }
        "decode-annotations" if args.len() >= 2 => {
            validate_options(&args[2..], &["--report"])?;
            let report = option(&args[2..], "--report")?;
            if report
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("brd"))
            {
                return Err(usage());
            }
            annotation_probe(Path::new(&args[1]), &report, locale, options)?;
        }
        "decode-connectivity" if args.len() >= 2 => {
            validate_options(&args[2..], &["--report"])?;
            let report = option(&args[2..], "--report")?;
            if report
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("brd"))
            {
                return Err(usage());
            }
            connectivity_probe(Path::new(&args[1]), &report, locale, options)?;
        }
        "decode-fixed" | "decode-records" | "decode-geometry" | "decode-padstack"
        | "decode-placement" | "decode-routing" | "decode-copper"
            if args.len() >= 2 =>
        {
            validate_options(&args[2..], &["--records", "--report"])?;
            let records = option(&args[2..], "--records")?;
            let report = option(&args[2..], "--report")?;
            if report
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("brd"))
            {
                return Err(usage());
            }
            decode_record_probe(
                Path::new(&args[1]),
                &records,
                &report,
                locale,
                options,
                command,
            )?;
        }
        "probe" | "index" if args.len() == 2 => {
            let stage = if command == "index" {
                Stage::Index
            } else {
                Stage::Header
            };
            let result = inspect(Path::new(&args[1]), false, stage, None, locale, options);
            println!(
                "{}",
                serde_json::to_string_pretty(&result).map_err(serialization_error)?
            );
            if let Some(error) = result.error {
                return Err(error);
            }
        }
        "manifest" | "check" => {
            let expected: &[&str] = if command == "check" {
                &["--cases-dir", "--report", "--stage", "--index-data-dir"]
            } else {
                &["--cases-dir", "--report"]
            };
            validate_options(&args[1..], expected)?;
            let directory = option(&args[1..], "--cases-dir")?;
            let report = option(&args[1..], "--report")?;
            let stage = if command == "check" {
                match option(&args[1..], "--stage")?.to_str() {
                    Some("header") => Stage::Header,
                    Some("index") => Stage::Index,
                    _ => {
                        return Err(Diagnostic::error(
                            "CLI_UNSUPPORTED_STAGE",
                            Key::CliUnsupportedStage,
                        ));
                    }
                }
            } else {
                Stage::Header
            };
            let index_data_dir = if args.iter().any(|arg| arg == "--index-data-dir") {
                if !matches!(stage, Stage::Index) {
                    return Err(usage());
                }
                let directory = option(&args[1..], "--index-data-dir")?;
                fs::create_dir_all(&directory).map_err(|error| io_error(error, &directory))?;
                Some(directory)
            } else {
                None
            };
            let mut paths = fs::read_dir(&directory)
                .map_err(|error| io_error(error, &directory))?
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<std::io::Result<Vec<_>>>()
                .map_err(|error| io_error(error, &directory))?;
            paths.retain(|path| {
                path.is_file()
                    && path
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("brd"))
            });
            paths.sort();
            if paths.is_empty() {
                let mut error = Diagnostic::error("CLI_NO_CASES", Key::CliNoCases);
                error.message = error
                    .message
                    .arg("path", directory.to_string_lossy().as_ref());
                return Err(error);
            }
            // Do not truncate a board by accidentally using it as the report path.
            if report
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("brd"))
            {
                return Err(usage());
            }
            if let Some(parent) = report
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                fs::create_dir_all(parent).map_err(|error| io_error(error, parent))?;
            }
            let mut writer =
                BufWriter::new(File::create(&report).map_err(|error| io_error(error, &report))?);
            let mut versions = BTreeMap::new();
            let mut failures = 0_u32;
            let mut total_bytes = 0_u64;
            for path in &paths {
                let result = inspect(
                    path,
                    command == "manifest",
                    stage,
                    index_data_dir.as_deref(),
                    locale,
                    options,
                );
                total_bytes = total_bytes.checked_add(result.bytes).ok_or_else(|| {
                    ImportError::ResourceLimit {
                        actual: u64::MAX,
                        limit: u64::MAX,
                    }
                    .diagnostic()
                })?;
                if let Some(header) = &result.header {
                    *versions.entry(header.version).or_insert(0_u32) += 1;
                }
                if result.status == "failed" {
                    failures += 1;
                }
                serde_json::to_writer(&mut writer, &result).map_err(serialization_error)?;
                writeln!(writer).map_err(|error| io_error(error, &report))?;
            }
            writer.flush().map_err(|error| io_error(error, &report))?;
            println!(
                "{}",
                serde_json::json!({"stage":stage.name(), "cases":paths.len(), "failed":failures,
                "bytes":total_bytes, "versions":versions, "report":report, "scene_validated":false})
            );
            if failures != 0 {
                let mut error = Diagnostic::error("CLI_PROBES_FAILED", Key::CliProbesFailed);
                error.message = error.message.arg("count", failures);
                return Err(error);
            }
        }
        _ => return Err(usage()),
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecodeRequest {
    schema_version: u32,
    sha256: String,
    source_size: u64,
    spans: Vec<ProbeSpan>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeSpan {
    offset: u32,
    byte_length: u32,
    key: u32,
    record_type: u8,
}

fn decode_record_probe(
    source: &Path,
    request: &Path,
    report: &Path,
    locale: Locale,
    options: &ImportOptions,
    mode: &str,
) -> Result<(), Diagnostic> {
    use pomelo_import::allegro::{
        decoder::{DecodeLimits, DecodedRecord, RecordDecoder},
        index::{FileOffset, RecordKey, RecordSpan},
    };
    let mut request_bytes = Vec::new();
    File::open(request)
        .map_err(|error| io_error(error, request))?
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut request_bytes)
        .map_err(|error| io_error(error, request))?;
    if request_bytes.len() > 2 * 1024 * 1024 {
        return Err(ImportError::ResourceLimit {
            actual: request_bytes.len() as u64,
            limit: 2 * 1024 * 1024,
        }
        .diagnostic()
        .with_path(request));
    }
    let input: DecodeRequest = serde_json::from_slice(&request_bytes).map_err(|error| {
        Diagnostic::error("CLI_DECODE_REQUEST_INVALID", Key::CliDecodeRequestInvalid)
            .with_details(error.to_string())
            .with_path(request)
    })?;
    if input.schema_version != 1 {
        return Err(
            Diagnostic::error("CLI_DECODE_REQUEST_INVALID", Key::CliDecodeRequestInvalid)
                .with_path(request),
        );
    }
    if input.spans.len() > 4096 {
        return Err(ImportError::IndexLimit {
            actual: input.spans.len() as u64 * 16,
            limit: 4096 * 16,
        }
        .diagnostic()
        .with_path(request));
    }
    let cancellation = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancellation,
        progress: &|_| {},
    };
    let bytes = pomelo_import::source::read_path(source, options, &context)
        .map_err(|error| error.diagnostic().with_path(source))?;
    if input.source_size != bytes.len() as u64
        || input.sha256 != format!("{:x}", Sha256::digest(&bytes))
    {
        return Err(
            Diagnostic::error("CLI_SOURCE_MISMATCH", Key::CliSourceMismatch).with_path(source),
        );
    }
    if matches!(
        mode,
        "decode-geometry"
            | "decode-padstack"
            | "decode-placement"
            | "decode-routing"
            | "decode-copper"
    ) {
        return semantic_probe(bytes, input, source, report, locale, options, mode);
    }
    let header = BrdHeader::read(&bytes, options.text_encoding)
        .map_err(|error| error.diagnostic().with_path(source))?;
    let decoder = RecordDecoder::new(&bytes, &header, options.text_encoding);
    let limits = DecodeLimits::default();
    let stage = if mode == "decode-fixed" {
        "fixed-records"
    } else {
        "records"
    };
    if let Some(parent) = report
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| io_error(error, parent))?;
    }
    let mut writer = BufWriter::new(File::create(report).map_err(|error| io_error(error, report))?);
    let mut failed = 0_u32;
    for span in &input.spans {
        let span = RecordSpan {
            offset: FileOffset(span.offset),
            byte_length: span.byte_length,
            key: RecordKey(span.key),
            record_type: span.record_type,
        };
        let decoded = if mode == "decode-fixed" {
            decoder
                .decode_fixed(&span, &context)
                .map(DecodedRecord::Fixed)
        } else {
            decoder.decode(&span, &limits, &context)
        };
        let (fields, error) = match decoded {
            Ok(record) => (Some(record), None),
            Err(error) => {
                failed += 1;
                (None, Some(error.diagnostic().with_path(source)))
            }
        };
        let message = error.as_ref().map(|error| error.message.display(locale));
        serde_json::to_writer(&mut writer, &serde_json::json!({ "schema_version": 1, "stage": stage, "path": source, "version": header.version,
            "offset": span.offset.0, "byte_length": span.byte_length, "key": span.key.0, "record_type": span.record_type, "fields": fields,
            "error": error, "locale": locale.tag(), "localized_message": message, "encoding": options.text_encoding.tag(), "scene_validated": false }))
            .map_err(serialization_error)?;
        writeln!(writer).map_err(|error| io_error(error, report))?;
    }
    writer.flush().map_err(|error| io_error(error, report))?;
    println!(
        "{}",
        serde_json::json!({ "stage": stage, "records": input.spans.len(), "failed": failed, "report": report, "scene_validated": false })
    );
    if failed != 0 {
        let mut error = Diagnostic::error("CLI_PROBES_FAILED", Key::CliProbesFailed);
        error.message = error.message.arg("count", failed);
        return Err(error);
    }
    Ok(())
}

fn scene_probe(
    source: &Path,
    report: &Path,
    locale: Locale,
    options: &ImportOptions,
) -> Result<(), Diagnostic> {
    use pomelo_import::allegro::{
        database::BrdDatabase,
        decoder::DecodeLimits,
        semantics::scene::{SceneBuilder, SceneLimits},
    };
    let cancellation = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancellation,
        progress: &|_| {},
    };
    let database = BrdDatabase::read_path(
        source,
        options,
        &IndexLimits::default(),
        DecodeLimits::default(),
        &context,
    )
    .map_err(|e| e.diagnostic().with_path(source))?;
    let result = SceneBuilder::new(&database, SceneLimits::default()).build(&context);
    let error = result
        .as_ref()
        .err()
        .map(|e| e.diagnostic().with_path(source));
    #[derive(Serialize)]
    struct Report<'a> {
        schema_version: u8,
        kind: &'static str,
        stage: &'static str,
        path: &'a Path,
        sha256: String,
        bytes: usize,
        version: u16,
        encoding: &'static str,
        source_records: usize,
        source_strings: usize,
        scene_validated: bool,
        counts: Option<serde_json::Value>,
        error: Option<&'a Diagnostic>,
        locale: &'static str,
        localized_message: Option<String>,
    }
    // Serialize directly from the scene; a serde_json::Value would duplicate the entire board.
    let data = Report {
        schema_version: 1,
        kind: "metadata",
        stage: "scene",
        path: source,
        sha256: format!("{:x}", Sha256::digest(database.source_bytes())),
        bytes: database.source_bytes().len(),
        version: database.header().version,
        encoding: options.text_encoding.tag(),
        source_records: database.index().records().len(),
        source_strings: database.index().strings.len(),
        scene_validated: false,
        counts: result.as_ref().ok().map(|s| serde_json::json!({
            "layers":s.layers.len(),"special_layers":s.special_layers.len(),"nets":s.nets.len(),"segments":s.segments.len(),
            "vias":s.vias.len(),"pins":s.pins.len(),"components":s.components.len(),"zones":s.zones.len(),"outline":s.outline.len(),
            "texts":s.texts.len(),"drawing_layers":s.drawing_layers.len(),"drawings":s.drawings.len(),"diagnostics":s.diagnostics.len(),"bounds":1,
        })),
        error: error.as_ref(),
        locale: locale.tag(),
        localized_message: error.as_ref().map(|d| d.message.display(locale)),
    };
    if let Some(parent) = report.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|e| io_error(e, parent))?;
    }
    let mut writer = BufWriter::new(File::create(report).map_err(|e| io_error(e, report))?);
    serde_json::to_writer(&mut writer, &data).map_err(serialization_error)?;
    writeln!(writer).map_err(|e| io_error(e, report))?;
    if let Ok(scene) = &result {
        macro_rules! rows {
            ($($field:ident),+ $(,)?) => { $(for (index, item) in scene.$field.iter().enumerate() {
                scene_row(&mut writer, stringify!($field), index, item, report)?;
            })+ };
        }
        rows!(
            layers,
            special_layers,
            segments,
            vias,
            pins,
            components,
            zones,
            outline,
            texts,
            drawing_layers,
            drawings,
            diagnostics
        );
        for (index, (id, name)) in scene.nets.iter().enumerate() {
            scene_row(&mut writer, "nets", index, &(id, name), report)?;
        }
        scene_row(&mut writer, "bounds", 0, &scene.bounds, report)?;
        scene_row(&mut writer, "complete", 0, &data.counts, report)?;
    }
    writer.flush().map_err(|e| io_error(e, report))?;
    println!(
        "{}",
        serde_json::json!({"stage":"scene", "status":if error.is_none(){"passed"}else{"failed"},"report":report,"scene_validated":false})
    );
    if let Some(error) = error {
        return Err(error);
    }
    Ok(())
}

fn scene_row<T: Serialize + ?Sized>(
    writer: &mut BufWriter<File>,
    kind: &str,
    index: usize,
    data: &T,
    report: &Path,
) -> Result<(), Diagnostic> {
    #[derive(Serialize)]
    struct Row<'a, T: ?Sized> {
        kind: &'a str,
        index: usize,
        data: &'a T,
    }
    serde_json::to_writer(&mut *writer, &Row { kind, index, data }).map_err(serialization_error)?;
    writeln!(writer).map_err(|e| io_error(e, report))
}

fn annotation_probe(
    source: &Path,
    report: &Path,
    locale: Locale,
    options: &ImportOptions,
) -> Result<(), Diagnostic> {
    use pomelo_core::{
        i18n::Message,
        model::{BoardDrawing, BoardText, DrawingLayer, LayerId},
    };
    use pomelo_import::allegro::{
        database::BrdDatabase,
        decoder::DecodeLimits,
        semantics::{
            drawing::{DrawingBuilder, DrawingLimits},
            layers::drawing_layer,
            text::{TextBuilder, TextLimits},
        },
    };
    let cancellation = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancellation,
        progress: &|_| {},
    };
    let database = BrdDatabase::read_path(
        source,
        options,
        &IndexLimits::default(),
        DecodeLimits::default(),
        &context,
    )
    .map_err(|error| error.diagnostic().with_path(source))?;
    #[derive(Serialize)]
    struct Annotations {
        texts: Vec<BoardText>,
        drawing_layers: Vec<DrawingLayer>,
        drawings: Vec<BoardDrawing>,
        diagnostics: Vec<Diagnostic>,
    }
    let result = (|| -> Result<Annotations, ImportError> {
        let mut texts = TextBuilder::new(&database, TextLimits::default()).build(&context)?;
        let mut drawings = DrawingBuilder::new(&database, DrawingLimits::default())
            .build(&texts.texts, &context)?;
        if !drawings.drawings.is_empty()
            && !texts
                .drawing_layers
                .iter()
                .any(|l| l.id == LayerId::DIMENSION)
        {
            texts
                .drawing_layers
                .push(drawing_layer(0xf901, String::new()));
            texts
                .drawing_layers
                .sort_by_key(|l| (!l.default_visible, l.id));
        }
        texts.diagnostics.append(&mut drawings.diagnostics);
        Ok(Annotations {
            texts: texts.texts,
            drawing_layers: texts.drawing_layers,
            drawings: drawings.drawings,
            diagnostics: texts.diagnostics,
        })
    })();
    let error = result
        .as_ref()
        .err()
        .map(|e| e.diagnostic().with_path(source));
    #[derive(Serialize)]
    struct Report<'a> {
        schema_version: u8,
        stage: &'static str,
        path: &'a Path,
        sha256: String,
        bytes: usize,
        version: u16,
        encoding: &'static str,
        source_records: usize,
        source_strings: usize,
        scene_validated: bool,
        annotations: Option<&'a Annotations>,
        error: Option<&'a Diagnostic>,
        locale: &'static str,
        localized_message: Option<String>,
        localized_layers: Vec<String>,
        localized_diagnostics: Vec<String>,
    }
    let result_ref = result.as_ref().ok();
    let data = Report {
        schema_version: 1,
        stage: "annotations",
        path: source,
        sha256: format!("{:x}", Sha256::digest(database.source_bytes())),
        bytes: database.source_bytes().len(),
        version: database.header().version,
        encoding: options.text_encoding.tag(),
        source_records: database.index().records().len(),
        source_strings: database.index().strings.len(),
        scene_validated: false,
        annotations: result_ref,
        error: error.as_ref(),
        locale: locale.tag(),
        localized_message: error.as_ref().map(|d| d.message.display(locale)),
        localized_layers: result_ref.map_or_else(Vec::new, |r| {
            r.drawing_layers
                .iter()
                .map(|l| l.display_name(locale))
                .collect()
        }),
        localized_diagnostics: result_ref.map_or_else(Vec::new, |r| {
            r.diagnostics
                .iter()
                .map(|d| Message::display(&d.message, locale))
                .collect()
        }),
    };
    if let Some(parent) = report.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|e| io_error(e, parent))?;
    }
    let mut writer = BufWriter::new(File::create(report).map_err(|e| io_error(e, report))?);
    serde_json::to_writer(&mut writer, &data).map_err(serialization_error)?;
    writeln!(writer).map_err(|e| io_error(e, report))?;
    writer.flush().map_err(|e| io_error(e, report))?;
    println!(
        "{}",
        serde_json::json!({ "stage": "annotations", "texts": result_ref.map_or(0, |r| r.texts.len()), "drawings": result_ref.map_or(0, |r| r.drawings.len()), "report": report, "scene_validated": false })
    );
    if let Some(error) = error {
        return Err(error);
    }
    Ok(())
}

/// This subsystem probe is development evidence, never an assertion of whole-scene support.
fn connectivity_probe(
    source: &Path,
    report: &Path,
    locale: Locale,
    options: &ImportOptions,
) -> Result<(), Diagnostic> {
    use pomelo_import::allegro::{
        database::BrdDatabase,
        decoder::DecodeLimits,
        semantics::connectivity::{NetworkLimits, NetworkMap},
    };
    let cancellation = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancellation,
        progress: &|_| {},
    };
    let database = BrdDatabase::read_path(
        source,
        options,
        &IndexLimits::default(),
        DecodeLimits::default(),
        &context,
    )
    .map_err(|error| error.diagnostic().with_path(source))?;
    let result = NetworkMap::build(&database, &NetworkLimits::default(), &context);
    let error = result
        .as_ref()
        .err()
        .map(|error| error.diagnostic().with_path(source));
    #[derive(Serialize)]
    struct Report<'a> {
        schema_version: u8,
        stage: &'static str,
        path: &'a Path,
        sha256: String,
        bytes: usize,
        version: u16,
        encoding: &'static str,
        source_records: usize,
        source_strings: usize,
        scene_validated: bool,
        network: Option<&'a NetworkMap>,
        error: Option<&'a Diagnostic>,
        localized_message: Option<String>,
    }
    let data = Report {
        schema_version: 1,
        stage: "connectivity",
        path: source,
        sha256: format!("{:x}", Sha256::digest(database.source_bytes())),
        bytes: database.source_bytes().len(),
        version: database.header().version,
        encoding: options.text_encoding.tag(),
        source_records: database.index().records().len(),
        source_strings: database.index().strings.len(),
        scene_validated: false,
        network: result.as_ref().ok(),
        error: error.as_ref(),
        localized_message: error.as_ref().map(|error| error.message.display(locale)),
    };
    if let Some(parent) = report
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| io_error(error, parent))?;
    }
    let mut writer = BufWriter::new(File::create(report).map_err(|error| io_error(error, report))?);
    serde_json::to_writer(&mut writer, &data).map_err(serialization_error)?;
    writeln!(writer).map_err(|error| io_error(error, report))?;
    writer.flush().map_err(|error| io_error(error, report))?;
    if let Some(error) = error {
        return Err(error);
    }
    Ok(())
}

fn semantic_probe(
    bytes: Vec<u8>,
    request: DecodeRequest,
    source: &Path,
    report: &Path,
    locale: Locale,
    options: &ImportOptions,
    mode: &str,
) -> Result<(), Diagnostic> {
    use pomelo_import::allegro::{
        database::{BrdDatabase, ReferenceLocation},
        decoder::{DecodeLimits, DecodedRecord, fixed::FixedRecord},
        index::{FileOffset, RecordKey},
        semantics::{
            CacheLimits,
            connectivity::{NetworkLimits, NetworkMap},
            copper::{CopperDecoder, CopperLimits},
            geometry::{GeometryDecoder, GeometryLimits, decode_edge},
            layers::read_layers,
            pad::PadDecoder,
            padstack::PadstackResolver,
            placement::{PlacementDecoder, PlacementLimits},
        },
    };
    let cancellation = CancellationToken::default();
    let context = ImportContext {
        cancellation: &cancellation,
        progress: &|_| {},
    };
    let database = BrdDatabase::read(
        bytes,
        options,
        &IndexLimits::default(),
        DecodeLimits::default(),
        &context,
    )
    .map_err(|error| error.diagnostic().with_path(source))?;
    let decoder = GeometryDecoder::new(&database, GeometryLimits::default())
        .map_err(|error| error.diagnostic().with_path(source))?;
    let layers =
        read_layers(&database, &context).map_err(|error| error.diagnostic().with_path(source))?;
    let stage = if mode == "decode-copper" {
        "copper"
    } else if mode == "decode-routing" {
        "routing"
    } else if mode == "decode-placement" {
        "placement"
    } else if mode == "decode-padstack" {
        "padstack"
    } else {
        "geometry"
    };
    let mut stacks = PadstackResolver::new(&database, layers.len() as u32, CacheLimits::default());
    let mut pads = PadDecoder::new(&database, CacheLimits::default(), GeometryLimits::default())
        .map_err(|error| error.diagnostic().with_path(source))?;
    let networks = if matches!(stage, "placement" | "routing" | "copper") {
        Some(
            NetworkMap::build(&database, &NetworkLimits::default(), &context)
                .map_err(|error| error.diagnostic().with_path(source))?,
        )
    } else {
        None
    };
    let mut placement = networks
        .as_ref()
        .filter(|_| matches!(stage, "placement" | "routing"))
        .map(|networks| {
            PlacementDecoder::new(
                &database,
                networks,
                layers.len() as u32,
                PlacementLimits::default(),
            )
        })
        .transpose()
        .map_err(|error| error.diagnostic().with_path(source))?;
    let mut copper = networks
        .as_ref()
        .filter(|_| stage == "copper")
        .map(|networks| {
            CopperDecoder::new(
                &database,
                networks,
                layers.len() as u32,
                CopperLimits::default(),
            )
        })
        .transpose()
        .map_err(|error| error.diagnostic().with_path(source))?;
    if let Some(parent) = report
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| io_error(error, parent))?;
    }
    let mut writer = BufWriter::new(File::create(report).map_err(|error| io_error(error, report))?);
    serde_json::to_writer(&mut writer, &serde_json::json!({ "schema_version": 1, "stage": stage, "kind": "metadata", "path": source,
        "sha256": request.sha256, "bytes": request.source_size, "scale": decoder.scale(), "layers": layers,
        "version": database.header().version, "encoding": options.text_encoding.tag(), "scene_validated": false })).map_err(serialization_error)?;
    writeln!(writer).map_err(|error| io_error(error, report))?;
    let mut failed = 0_u32;
    for span in &request.spans {
        let mut probe = || -> Result<serde_json::Value, ImportError> {
            let origin = ReferenceLocation {
                offset: FileOffset(span.offset),
                field: "GeometryProbe",
            };
            let record = database
                .get_at_offset(FileOffset(span.offset), &context)?
                .ok_or(ImportError::InvalidRecord {
                    offset: span.offset as usize,
                    field: "PROBE_INDEX_BOUNDARY",
                    value: u64::from(span.key),
                })?;
            if record.span.key.0 != span.key
                || record.span.byte_length != span.byte_length
                || record.span.record_type != span.record_type
            {
                return Err(ImportError::InvalidRecord {
                    offset: span.offset as usize,
                    field: "PROBE_INDEX_IDENTITY",
                    value: u64::from(span.key),
                });
            }
            if stage == "padstack" {
                return padstack_probe_value(&record, &database, &mut stacks, &mut pads, &context);
            }
            if let Some(copper) = copper.as_mut() {
                return match span.record_type {
                    0x28 => serde_json::to_value(copper.shape(record.span.key, &context)?),
                    0x0e | 0x24 => Ok(serde_json::json!({ "zone": copper.rectangle(record.span.key, &context)? })),
                    0x14 => Ok(serde_json::json!({ "outline": copper.graphic_outline(record.span.key, &context)? })),
                    _ => return Err(ImportError::ReferenceType { key: span.key, actual: span.record_type, expected: vec![0x28, 0x0e, 0x24, 0x14], offset: span.offset as usize, field: "CopperProbe" }),
                }.map_err(|_| ImportError::InvalidRecord { offset: span.offset as usize, field: "COPPER_SERIALIZATION", value: span.key as u64 });
            }
            if let Some(placement) = placement.as_mut() {
                if stage == "routing" {
                    return match span.record_type {
                        5 => Ok(
                            serde_json::json!({ "segments": placement.track(record.span.key, &context)? }),
                        ),
                        0x33 => Ok(
                            serde_json::json!({ "via": placement.via(record.span.key, &context)? }),
                        ),
                        _ => Err(ImportError::ReferenceType {
                            key: span.key,
                            actual: span.record_type,
                            expected: vec![5, 0x33],
                            offset: span.offset as usize,
                            field: "RoutingProbe",
                        }),
                    };
                }
                if span.record_type != 0x2d {
                    return Err(ImportError::ReferenceType {
                        key: span.key,
                        actual: span.record_type,
                        expected: vec![0x2d],
                        offset: span.offset as usize,
                        field: "PlacementProbe",
                    });
                }
                return serde_json::to_value(placement.footprint(record.span.key, &context)?)
                    .map_err(|_| ImportError::InvalidRecord {
                        offset: span.offset as usize,
                        field: "PLACEMENT_SERIALIZATION",
                        value: span.key as u64,
                    });
            }
            match &record.fields {
                DecodedRecord::Fixed(FixedRecord::Arc(_) | FixedRecord::Segment(_)) => Ok(
                    serde_json::json!({ "edge": decode_edge(&record, decoder.scale(), false)?, "hatch_edge": decode_edge(&record, decoder.scale(), true)? }),
                ),
                DecodedRecord::Fixed(FixedRecord::Track(track)) => Ok(
                    serde_json::json!({ "path": decoder.read_path(RecordKey(track.first_seg_ptr), false, None, ReferenceLocation { field: "FirstSegPtr", ..origin }, &context)? }),
                ),
                DecodedRecord::Fixed(FixedRecord::Graphic(graphic)) => Ok(
                    serde_json::json!({ "path": decoder.read_path(RecordKey(graphic.segment_ptr), false, None, ReferenceLocation { field: "SegmentPtr", ..origin }, &context)? }),
                ),
                DecodedRecord::Fixed(FixedRecord::Shape(shape)) => Ok(
                    serde_json::json!({ "contours": decoder.read_contours(shape.key, &context)? }),
                ),
                _ => Err(ImportError::ReferenceType {
                    key: span.key,
                    actual: span.record_type,
                    expected: vec![1, 5, 0x14, 0x15, 0x16, 0x17, 0x28],
                    offset: span.offset as usize,
                    field: "GeometryProbe",
                }),
            }
        };
        let (geometry, error) = match probe() {
            Ok(value) => (Some(value), None),
            Err(error) => {
                failed += 1;
                (None, Some(error.diagnostic().with_path(source)))
            }
        };
        let message = error.as_ref().map(|error| error.message.display(locale));
        serde_json::to_writer(&mut writer, &serde_json::json!({ "schema_version": 1, "stage": stage, "kind": "record", "path": source,
            "key": span.key, "offset": span.offset, "byte_length": span.byte_length, "record_type": span.record_type,
            (stage): geometry, "error": error, "locale": locale.tag(), "localized_message": message, "encoding": options.text_encoding.tag(), "scene_validated": false })).map_err(serialization_error)?;
        writeln!(writer).map_err(|error| io_error(error, report))?;
    }
    if matches!(stage, "padstack" | "placement" | "routing" | "copper") {
        let diagnostics = if let Some(copper) = copper.as_mut() {
            copper.take_diagnostics()
        } else {
            placement.as_mut().map_or_else(
                || pads.take_diagnostics(),
                PlacementDecoder::take_diagnostics,
            )
        };
        let diagnostics: Vec<_> = diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.with_path(source))
            .collect();
        let messages: Vec<_> = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.display(locale))
            .collect();
        serde_json::to_writer(&mut writer, &serde_json::json!({ "schema_version": 1, "stage": stage, "kind": "diagnostics", "diagnostics": diagnostics,
            "localized_messages": messages, "locale": locale.tag(), "scene_validated": false })).map_err(serialization_error)?;
        writeln!(writer).map_err(|error| io_error(error, report))?;
    }
    writer.flush().map_err(|error| io_error(error, report))?;
    println!(
        "{}",
        serde_json::json!({ "stage": stage, "records": request.spans.len(), "failed": failed, "report": report, "scene_validated": false })
    );
    if failed != 0 {
        let mut error = Diagnostic::error("CLI_PROBES_FAILED", Key::CliProbesFailed);
        error.message = error.message.arg("count", failed);
        return Err(error);
    }
    Ok(())
}

fn preview_padstack(
    stack: &pomelo_import::allegro::decoder::variable::Padstack,
    pads: &mut pomelo_import::allegro::semantics::pad::PadDecoder<'_>,
    context: &ImportContext<'_>,
) -> Result<serde_json::Value, ImportError> {
    Ok(
        serde_json::json!({ "stack_key": stack.key.0, "start_layer": stack.start_layer, "layer_count": stack.layer_count,
        "pad_type": stack.pad_type, "regular_pads": pads.regular_pads(stack, context)?, "drill": pads.drill(stack, context)? }),
    )
}

fn resolved_padstack(
    resolved: Option<pomelo_import::allegro::semantics::padstack::ResolvedStack>,
    pads: &mut pomelo_import::allegro::semantics::pad::PadDecoder<'_>,
    context: &ImportContext<'_>,
) -> Result<serde_json::Value, ImportError> {
    let Some(resolved) = resolved else {
        return Ok(serde_json::Value::Null);
    };
    let preview = preview_padstack(&resolved.stack, pads, context)?;
    let backdrill = resolved
        .backdrill
        .as_ref()
        .map(|source| source.to_millimetres(pads.scale()));
    let effective = if let Some(definition) = &backdrill {
        let ordinary = pads.regular_pads(&resolved.stack, context)?;
        Some(pomelo_import::allegro::semantics::pad::apply_backdrill(
            &ordinary,
            definition,
            resolved.stack.layer_count,
            context,
        )?)
    } else {
        None
    };
    Ok(
        serde_json::json!({ "preview": preview, "embedded_layer": resolved.embedded_layer, "region_code": resolved.region_code, "die": resolved.die,
        "backdrill_source": resolved.backdrill, "backdrill": backdrill, "effective_pads": effective }),
    )
}

fn padstack_probe_value(
    record: &pomelo_import::allegro::database::LocatedRecord,
    database: &pomelo_import::allegro::database::BrdDatabase,
    stacks: &mut pomelo_import::allegro::semantics::padstack::PadstackResolver<'_>,
    pads: &mut pomelo_import::allegro::semantics::pad::PadDecoder<'_>,
    context: &ImportContext<'_>,
) -> Result<serde_json::Value, ImportError> {
    use pomelo_import::allegro::{
        decoder::{DecodedRecord, fixed::FixedRecord, variable::VariableRecord},
        index::RecordKey,
        semantics::padstack::bond_finger_angle,
    };
    match &record.fields {
        DecodedRecord::Variable(VariableRecord::Padstack(stack)) => {
            Ok(serde_json::json!({ "definition": preview_padstack(stack, pads, context)? }))
        }
        DecodedRecord::Fixed(FixedRecord::UnknownRecord0x2f(wrapper)) => {
            let owner = RecordKey(wrapper.unknown_array.get(1).copied().unwrap_or(0));
            let pin = stacks.resolve_pin(wrapper.key, owner, context)?;
            let via = stacks.resolve_via(wrapper.key, owner, context)?;
            Ok(
                serde_json::json!({ "pin": resolved_padstack(pin, pads, context)?, "via": resolved_padstack(via, pads, context)? }),
            )
        }
        DecodedRecord::Fixed(FixedRecord::PlacedPad(placed)) => {
            let key = RecordKey(placed.pad_ptr);
            let pad = if database
                .index()
                .record(key)
                .is_some_and(|span| span.record_type == 0x0d)
            {
                database.get(key, context)?
            } else {
                None
            };
            let resolved = match pad.map(|record| record.fields) {
                Some(DecodedRecord::Fixed(FixedRecord::Pad(pad))) => {
                    stacks.resolve_pin(RecordKey(pad.pad_stack), placed.key, context)?
                }
                _ => None,
            };
            Ok(serde_json::json!({ "pin": resolved_padstack(resolved, pads, context)? }))
        }
        DecodedRecord::Fixed(FixedRecord::Via(via)) => {
            let resolved = stacks.resolve_via(RecordKey(via.padstack), via.key, context)?;
            let angle = resolved
                .as_ref()
                .and_then(|resolved| bond_finger_angle(via, &resolved.stack, stacks.layer_count()));
            Ok(
                serde_json::json!({ "via": resolved_padstack(resolved, pads, context)?, "bond_finger_angle": angle }),
            )
        }
        _ => Err(ImportError::ReferenceType {
            key: record.span.key.0,
            actual: record.span.record_type,
            expected: vec![0x1c, 0x2f, 0x32, 0x33],
            offset: record.span.offset.0 as usize,
            field: "PadstackProbe",
        }),
    }
}

fn usage() -> Diagnostic {
    Diagnostic::error("CLI_USAGE", Key::CliUsage)
}
fn missing_value(name: &str) -> Diagnostic {
    let mut error = Diagnostic::error("CLI_MISSING_VALUE", Key::CliMissingValue);
    error.message = error.message.arg("option", name);
    error
}
fn validate_options(args: &[OsString], expected: &[&str]) -> Result<(), Diagnostic> {
    let mut seen = std::collections::BTreeSet::new();
    for chunk in args.chunks(2) {
        let name = chunk[0].to_str().ok_or_else(usage)?;
        if !expected.contains(&name) || !seen.insert(name) {
            return Err(usage());
        }
        if chunk.len() != 2 || chunk[1].to_string_lossy().starts_with("--") {
            return Err(missing_value(name));
        }
    }
    Ok(())
}
fn option(args: &[OsString], name: &str) -> Result<PathBuf, Diagnostic> {
    let position = args.iter().position(|arg| arg == name).ok_or_else(|| {
        let mut error = Diagnostic::error("CLI_MISSING_OPTION", Key::CliMissingOption);
        error.message = error.message.arg("option", name);
        error
    })?;
    args.get(position + 1)
        .map(PathBuf::from)
        .ok_or_else(|| missing_value(name))
}
fn io_error(error: std::io::Error, path: &Path) -> Diagnostic {
    ImportError::Io(error).diagnostic().with_path(path)
}
fn serialization_error(error: serde_json::Error) -> Diagnostic {
    Diagnostic::error("CLI_SERIALIZE_FAILED", Key::CliSerializeFailed)
        .with_details(error.to_string())
}
fn inspect(
    path: &Path,
    hash: bool,
    stage: Stage,
    index_data_dir: Option<&Path>,
    locale: Locale,
    options: &ImportOptions,
) -> CaseResult {
    let mut result = CaseResult {
        schema_version: 3,
        path: path.to_owned(),
        bytes: 0,
        sha256: None,
        stage: stage.name(),
        status: "passed",
        header: None,
        index: None,
        index_data_path: None,
        error: None,
        locale: locale.tag(),
        localized_message: None,
        encoding: options.text_encoding.tag(),
    };
    if let Err(error) = inspect_file(path, hash, stage, index_data_dir, options, &mut result) {
        result.status = "failed";
        result.localized_message = Some(error.message.display(locale));
        result.error = Some(error.with_path(path));
    }
    result
}
fn inspect_file(
    path: &Path,
    hash: bool,
    stage: Stage,
    index_data_dir: Option<&Path>,
    options: &ImportOptions,
    result: &mut CaseResult,
) -> Result<(), Diagnostic> {
    let mut file = File::open(path).map_err(|error| io_error(error, path))?;
    result.bytes = file
        .metadata()
        .map_err(|error| io_error(error, path))?
        .len();
    if matches!(stage, Stage::Index) {
        let cancellation = CancellationToken::default();
        let context = ImportContext {
            cancellation: &cancellation,
            progress: &|_| {},
        };
        let bytes = pomelo_import::source::read_path(path, options, &context)
            .map_err(|error| error.diagnostic())?;
        result.bytes = bytes.len() as u64;
        // Bind index evidence to the exact input, including the frozen case manifest.
        result.sha256 = Some(format!("{:x}", Sha256::digest(&bytes)));
        let index = BrdIndex::read(&bytes, options, &IndexLimits::default(), &context)
            .map_err(|error| error.diagnostic())?;
        result.index = Some(index.summary());
        if let Some(directory) = index_data_dir {
            let mut filename = path.file_name().ok_or_else(usage)?.to_os_string();
            filename.push(".idx");
            let output_path = directory.join(filename);
            write_index_data(&output_path, &index)?;
            result.index_data_path = Some(output_path);
        }
        result.header = Some(index.header);
        return Ok(());
    }
    let prefix_length = usize::try_from(result.bytes.min(HEADER_BYTES as u64)).map_err(|_| {
        ImportError::ResourceLimit {
            actual: result.bytes,
            limit: HEADER_BYTES as u64,
        }
        .diagnostic()
    })?;
    let mut prefix = vec![0; prefix_length];
    file.read_exact(&mut prefix)
        .map_err(|error| io_error(error, path))?;
    let parsed = BrdHeader::read(&prefix, options.text_encoding);
    if hash {
        let mut digest = Sha256::new();
        digest.update(&prefix);
        let mut buffer = vec![0; 1024 * 1024];
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|error| io_error(error, path))?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        result.sha256 = Some(format!("{:x}", digest.finalize()));
    }
    result.header = Some(parsed.map_err(|error| error.diagnostic())?);
    Ok(())
}

/// Development evidence, version 1: little-endian u32 fields, UTF-8 strings.
/// PMIDX001 + record count + string count; each record is offset/length/key/type.
/// Sorted strings follow as key/UTF-8 byte length/bytes, without padding.
fn write_index_data(path: &Path, index: &BrdIndex) -> Result<(), Diagnostic> {
    let write = || -> std::io::Result<()> {
        let mut writer = BufWriter::new(File::create(path)?);
        writer.write_all(b"PMIDX001")?;
        writer.write_all(&(index.records().len() as u32).to_le_bytes())?;
        writer.write_all(&(index.strings.len() as u32).to_le_bytes())?;
        for record in index.records() {
            for value in [
                record.offset.0,
                record.byte_length,
                record.key.0,
                u32::from(record.record_type),
            ] {
                writer.write_all(&value.to_le_bytes())?;
            }
        }
        let mut strings: Vec<_> = index.strings.iter().collect();
        strings.sort_unstable_by_key(|&(key, _)| key);
        for (&key, value) in strings {
            writer.write_all(&key.to_le_bytes())?;
            writer.write_all(&(value.len() as u32).to_le_bytes())?;
            writer.write_all(value.as_bytes())?;
        }
        writer.flush()
    };
    write().map_err(|error| io_error(error, path))
}
