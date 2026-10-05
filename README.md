<p align="center">
  <img src="crates/pomelo/assets/pomelo.svg" width="120" alt="Pomelo logo">
</p>

<h1 align="center">Pomelo Desktop · PCB Viewer</h1>

<p align="center">
  <strong>See every connection, layer by layer.</strong><br>
  A PCB viewer based on GPUI and GPUI Kit, supporting Cadence Allegro (.brd), Altium Designer, ODB++, PADS, Ansys HFSS 3D Layout(edb.def), and KiCad.
</p>

<p align="center">English · <a href="README_zh-CN.md">简体中文</a> · <a href="README_zh-TW.md">繁體中文</a> · <a href="README_ja.md">日本語</a> · <a href="README_ko.md">한국어</a></p>

<p align="center"><a href="https://github.com/HaiwenZhang/Pomelo-Desktop/releases">Download releases</a></p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-0078D4" alt="Windows, macOS, Linux">
  <img src="https://img.shields.io/badge/built_with-Rust_%2B_GPUI_Kit-58752c" alt="Rust + GPUI Kit">
  <img src="https://img.shields.io/badge/rendering-D3D11%20%7C%20Metal%20%7C%20wgpu-58752c" alt="D3D11, Metal, wgpu">
  <img src="https://img.shields.io/badge/status-in_development-d77d8a" alt="In development">
</p>

![PCB workspace](images/pcb-example-en.png)

## Your board, a clearer view

- **Keep designs local.** Import and view board files on your computer without uploading them. Viewing does not modify the source board.
- **Explore with native GPU rendering.** D3D11/HLSL, Metal/MSL, and wgpu/WGSL render traces, arcs, pads, vias, copper regions and holes, drawings, and MSDF board text.
- **Follow connections.** Search nets and components, navigate to results, and inspect selected objects.
- **Control the view.** Manage layers, color by layer or net, and adjust copper opacity, pad fills, and labels.
- **Work across boards.** Multiple documents, recent files, and saved view preferences make it easier to return to a design.
- **Choose your language.** English, Simplified Chinese, Traditional Chinese, Japanese, and Korean interfaces; light and dark themes.

> **In development:** the full BRD MVP acceptance checks remain incomplete. Visual fidelity, large-board performance, DPI/input methods, and long-running stability still need broader validation. This is a read-only viewer; it does not edit boards or run DRC. See [development progress](docs/development-progress.md) for verification scope.

## Supported formats

Currently supports **Cadence Allegro binary `.brd` files**. This extension does not imply support for other EDA tools that also use `.brd`. Import coverage and rendering accuracy depend on the file version and its contents. Other formats supported by the web version are not yet available in this desktop project.

## Quick start

On macOS, use a Metal-capable Mac and Xcode Command Line Tools. On Linux, install the development packages for fontconfig, FreeType, xkbcommon, Wayland/X11, ALSA, and OpenSSL, plus a Vulkan-capable GPU driver. The build commands below apply to all three platforms. On macOS, use Cmd in place of Ctrl for application shortcuts.

Use **Windows x64**, a graphics device and driver supporting **Direct3D 11**, **Git**, and Rust with the **MSVC C++ build tools and Windows SDK**. The repository pins Rust **1.98.1** in `rust-toolchain.toml`; dependency versions are recorded in `Cargo.lock`.

```powershell
git clone https://github.com/HaiwenZhang/Pomelo-Desktop.git
cd Pomelo-Desktop
cargo run -p pomelo --locked
```

Open a board with **Ctrl+O**, drag and drop, or a startup file path. Select a language from the application menu or toolbar.

![Welcome screen](images/welcome-en.png)

```powershell
cargo run -p pomelo --locked -- --locale en --encoding windows-1252 "C:\boards\example.brd"
```

`--locale` accepts `en`, `zh-CN`, `zh-TW`, `ja`, and `ko`. It applies only to this launch and does not overwrite the saved language preference. `--encoding` accepts `auto`, `utf-8`, `gbk`, `shift_jis`, `big5`, and `windows-1252`; the default is `auto`, which detects UTF-8, GBK, Shift JIS, Big5, or Windows-1252 from BRD text fields before strict decoding. Short or ambiguous text can be misidentified; use an explicit encoding to override detection. The selected encoding applies to files opened in that process. Interface language and source encoding are independent. Arguments after `--` are treated as file paths.

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

GPUI temporarily uses [HaiwenZhang/zed, gpui-pre-0.3.7-native-gpu](https://github.com/HaiwenZhang/zed/tree/gpui-pre-0.3.7-native-gpu), with `Cargo.lock` currently resolving to `8c92bda2dc9d718cd520d37fda20bca00fa3e274`. Cargo fetches the fork directly; no local GPUI patch preparation is required. Small package-name bridges keep GPUI Kit 0.7.0 on the same GPUI types. See [GPUI dependency notes](docs/gpui-dependencies.md). Normal desktop use does not require Node.js or the web repository. The PCB backends use D3D11 on Windows, Metal on macOS, and wgpu on Linux, sharing scene and batching logic while maintaining platform shaders independently. GPUI Kit uses the original registry packages; Cargo and rust-analyzer work directly without patch preparation.

```powershell
cargo build -p pomelo --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -p pomelo-import --bin pcb_inspect --locked -- --help
```

### Release packaging

| Platform / architecture | Package | Command |
| --- | --- | --- |
| Windows x64 | `.exe` | `scripts\build-windows.bat` |
| Windows ARM64 | `.exe` | `scripts\build-windows.bat --arch arm64` |
| macOS arm64 / x64 | `.dmg`, `.app.zip` | `scripts/build-macos.sh` |
| Ubuntu 24.04+ amd64 / arm64 | `.deb`, `.tar.gz` | `scripts/build-ubuntu.sh` |

Requires Rust and native build dependencies. macOS packaging also needs ImageMagick; Windows needs Inno Setup 6; Ubuntu packaging needs `dpkg-dev` and `desktop-file-utils`. Outputs go to `dist`.

The shell scripts accept `--offline`, `--toolchain NAME`, `--output-dir PATH`, and `--binary PATH` to package an existing executable. Release builds use `cargo build --release --locked` without Python.

Ubuntu releases are built on 24.04 to retain the 24.04 ABI baseline; CI also installs and checks the packages on Ubuntu 26.04. Building locally on a newer Ubuntu can introduce newer library requirements.

macOS defaults to ad-hoc signing and macOS 14.0 minimum. `MACOS_SIGNING_IDENTITY` selects an existing keychain identity; Apple notarization is not configured.

[Release workflow](.github/workflows/release.yml): pushing `vX.Y.Z` builds all platforms and publishes packages plus `SHA256SUMS` after validation. The tag must match the workspace version in `Cargo.toml`. Manual runs only upload Actions artifacts.


| Location | Purpose |
| --- | --- |
| [crates/pomelo-core](crates/pomelo-core) | Board model, search, selection, and localization |
| [crates/pomelo-import](crates/pomelo-import) | Allegro parsing and import diagnostics |
| [crates/pomelo-render](crates/pomelo-render) | Scene preparation and D3D11, Metal, and wgpu renderers |
| [crates/pomelo](crates/pomelo) | GPUI desktop application and document management |
| [locales](locales) | Five-language interface resources |
| [docs](docs) | Development plans and validation evidence |

- [Development plan](docs/native-pcb-viewer-development-plan.md)
- [Development progress](docs/development-progress.md)
- [i18n](docs/i18n.md)
- [GPU / ADR](docs/adr/0001-gpui-pre-0.3.7-native-gpuing.md)

## Contributing

Contributions to Allegro compatibility, rendering accuracy, performance, translations, and documentation are welcome. [Report an issue](https://github.com/HaiwenZhang/Pomelo-Desktop/issues) with the source tool/file version, OS and GPU details, reproduction steps, and import diagnostics. A small sample or comparison screenshot helps; remove confidential design data before sharing. Add focused regression tests for parser or geometry changes and screenshots for visual changes. Keep all five README versions in sync.

## License

Project code is licensed under [MIT](LICENSE). Bundled font assets retain their own licenses; see [Source Han Sans](assets/fonts/source-han-sans/README.md).

## Acknowledgments

Thanks to [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) and [GPUI Kit](https://github.com/longbridge/gpui-kit) for the GPU-accelerated UI framework and UI components that power Pomelo Desktop.
