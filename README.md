<p align="center">
  <img src="crates/pomelo/assets/pomelo.svg" width="120" alt="Pomelo logo">
</p>

<h1 align="center">Pomelo Desktop · PCB Viewer</h1>

<p align="center">
  <strong>See every connection, layer by layer.</strong><br>
  A native PCB viewer built with Rust and GPUI Kit. Explore Cadence Allegro boards on Windows with local file processing and GPU rendering.
</p>

<p align="center">English · <a href="README_zh-CN.md">简体中文</a> · <a href="README_zh-TW.md">繁體中文</a> · <a href="README_ja.md">日本語</a> · <a href="README_ko.md">한국어</a></p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-Windows-0078D4" alt="Windows">
  <img src="https://img.shields.io/badge/built_with-Rust_%2B_GPUI_Kit-58752c" alt="Rust + GPUI Kit">
  <img src="https://img.shields.io/badge/rendering-D3D11-58752c" alt="D3D11">
  <img src="https://img.shields.io/badge/status-in_development-d77d8a" alt="In development">
</p>

![PCB workspace](images/pcb-example-en.png)

## Your board, a clearer view

- **Keep designs local.** Import and view board files on your computer without uploading them. Viewing does not modify the source board.
- **Explore with native GPU rendering.** D3D11 and HLSL render traces, arcs, pads, vias, copper regions and holes, drawings, and MSDF board text.
- **Follow connections.** Search nets and components, navigate to results, and inspect selected objects.
- **Control the view.** Manage layers, color by layer or net, and adjust copper opacity, pad fills, and labels.
- **Work across boards.** Multiple documents, recent files, and saved view preferences make it easier to return to a design.
- **Choose your language.** English, Simplified Chinese, Traditional Chinese, Japanese, and Korean interfaces; light and dark themes.

> **In development:** the full BRD MVP acceptance checks remain incomplete. Visual fidelity, large-board performance, DPI/input methods, and long-running stability still need broader validation. This is a read-only viewer; it does not edit boards or run DRC. See [development progress](docs/development-progress.md) for verification scope.

## Supported formats

Currently supports **Cadence Allegro binary `.brd` files**. This extension does not imply support for other EDA tools that also use `.brd`. Import coverage and rendering accuracy depend on the file version and its contents. Other formats supported by the web version are not yet available in this desktop project.

## Quick start

Use **Windows x64**, a graphics device and driver supporting **Direct3D 11**, **Git**, **Python 3.12+**, and Rust with the **MSVC C++ build tools and Windows SDK**. The repository pins Rust **1.98.1** in `rust-toolchain.toml`; dependency versions are recorded in `Cargo.lock`.

```powershell
git clone https://github.com/HaiwenZhang/Pomelo-Desktop.git
cd Pomelo-Desktop
python scripts/cargo.py run -p pomelo --locked
```

Open a board with **Ctrl+O**, drag and drop, or a startup file path. Select a language from the application menu or toolbar.

![Welcome screen](images/welcome-en.png)

```powershell
python scripts/cargo.py run -p pomelo --locked -- --locale en --encoding windows-1252 "C:\boards\example.brd"
```

`--locale` accepts `en`, `zh-CN`, `zh-TW`, `ja`, and `ko`. It applies only to this launch and does not overwrite the saved language preference. `--encoding` accepts `utf-8`, `gbk`, `shift_jis`, `big5`, and `windows-1252`; the default is strict UTF-8. The selected encoding applies to files opened in that process. Interface language and source encoding are independent. Arguments after `--` are treated as file paths.

## Navigation

| Action | Control |
| --- | --- |
| Open file | `Ctrl+O` |
| Close document | `Ctrl+W` |
| Reload document | `Ctrl+R` |
| Next / previous document | `Ctrl+Tab / Ctrl+Shift+Tab` |
| Fit board | `F2` |
| Focus search | `Ctrl+K` |
| Settings | `Ctrl+,` |
| Toggle left / right panel | `Ctrl+B / Ctrl+Shift+B` |

## Build and development

The Cargo wrapper prepares the local GPUI GPU patches under `.cache/gpui/` without modifying the global Cargo registry. Run `python scripts/prepare_gpui.py` before using plain Cargo or rust-analyzer for the first time. See [GPUI patch notes](patches/gpui/README.md). Normal desktop use does not require Node.js or the web repository. macOS and Linux are planned; the native application and GPU backend currently target Windows.

```powershell
python scripts/cargo.py build -p pomelo --locked
python scripts/cargo.py test --workspace --locked
python scripts/cargo.py clippy --workspace --all-targets --locked -- -D warnings
python scripts/cargo.py run -p pomelo-import --bin pcb_inspect --locked -- --help
```

| Location | Purpose |
| --- | --- |
| [crates/pomelo-core](crates/pomelo-core) | Board model, search, selection, and localization |
| [crates/pomelo-import](crates/pomelo-import) | Allegro parsing and import diagnostics |
| [crates/pomelo-render](crates/pomelo-render) | Scene preparation, D3D11 renderer, and HLSL shaders |
| [crates/pomelo](crates/pomelo) | GPUI desktop application and document management |
| [locales](locales) | Five-language interface resources |
| [docs](docs) | Development plans and validation evidence |

- [Development plan](docs/native-pcb-viewer-development-plan.md)
- [Development progress](docs/development-progress.md)
- [i18n](docs/i18n.md)
- [GPU / ADR](docs/adr/0001-native-gpu-rendering.md)

## Contributing

Contributions to Allegro compatibility, rendering accuracy, performance, translations, and documentation are welcome. [Report an issue](https://github.com/HaiwenZhang/Pomelo-Desktop/issues) with the source tool/file version, Windows and GPU details, reproduction steps, and import diagnostics. A small sample or comparison screenshot helps; remove confidential design data before sharing. Add focused regression tests for parser or geometry changes and screenshots for visual changes. Keep all five README versions in sync.

## License

Project code is licensed under [MIT](LICENSE). Bundled font assets retain their own licenses; see [Source Han Sans](assets/fonts/source-han-sans/README.md) and [stroke fonts](assets/fonts/stroke/README.md).
