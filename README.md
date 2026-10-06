<p align="center">
  <img src="crates/pomelo/assets/pomelo.svg" width="120" alt="Pomelo logo">
</p>

<h1 align="center">Pomelo Desktop · PCB Viewer</h1>

<p align="center">
  <strong>See every connection, layer by layer.</strong><br>
  A local, read-only PCB viewer built with GPUI and GPUI Kit.
</p>

<p align="center">English · <a href="README_zh-CN.md">简体中文</a> · <a href="README_zh-TW.md">繁體中文</a> · <a href="README_ja.md">日本語</a> · <a href="README_ko.md">한국어</a></p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-0078D4" alt="Windows, macOS, Linux">
  <img src="https://img.shields.io/badge/built_with-Rust_%2B_GPUI_Kit-58752c" alt="Rust + GPUI Kit">
  <img src="https://img.shields.io/badge/rendering-D3D11%20%7C%20Metal%20%7C%20wgpu-58752c" alt="D3D11, Metal, wgpu">
  <img src="https://img.shields.io/badge/status-in_development-d77d8a" alt="In development">
</p>

## Your board, a clearer view

- **Keep designs local.** Import and view board files on your computer without uploading them. Viewing does not modify the source board.
- **Explore with native GPU rendering.** D3D11/HLSL, Metal/MSL, and wgpu/WGSL render traces, arcs, pads, vias, copper regions and holes, drawings, and MSDF board text.
- **Follow connections.** Search nets and components, navigate to results, and inspect selected objects.
- **Control the view.** Manage layers, color by layer or net, and adjust copper opacity, pad fills, and labels.
- **Work across boards.** Multiple documents, recent files, and saved view preferences make it easier to return to a design.
- **Choose your language.** English, Simplified Chinese, Traditional Chinese, Japanese, and Korean interfaces; light and dark themes.

> **In development:** compatibility, visual fidelity, performance, and platform stability need further validation. This is a read-only viewer; it does not edit boards or run DRC. See [development notes](docs/development-progress.md).

## Supported formats

Supports `.brd`, `.PcbDoc`, ODB++ (`.tgz`, `.tar`, `.tar.gz`), `.pcb`, `.kicad_pcb`, and `edb.def`. Import coverage depends on the file version and saved data; unsupported content is reported through diagnostics.

All board readers are implemented in Rust in `crates/pomelo-import`; no JavaScript engine or Node.js is used.

## Quick start

Install Git and the repository-pinned Rust **1.98.1** toolchain. Dependency versions are recorded in `Cargo.lock`.

| Platform | Build and runtime requirements |
| --- | --- |
| Windows x64 | MSVC C++ build tools, Windows SDK, and a Direct3D 11-capable device and driver |
| macOS | A Metal-capable Mac and Xcode Command Line Tools |
| Linux | Development packages for fontconfig, FreeType, xkbcommon, Wayland/X11, ALSA, and OpenSSL, plus a Vulkan-capable driver |

The commands below apply to all three platforms. On macOS, use Cmd in place of Ctrl for application shortcuts.

```powershell
git clone https://github.com/HaiwenZhang/Pomelo-Desktop.git
cd Pomelo-Desktop
cargo run -p pomelo --locked
```

Open a board with **Ctrl+O**, drag and drop, or a startup file path. Select a language from the application menu or toolbar.

```powershell
cargo run -p pomelo --locked -- --locale en --encoding windows-1252 "example.brd"
```

`--locale` accepts `en`, `zh-CN`, `zh-TW`, `ja`, and `ko`. It applies only to this launch and does not overwrite the saved language preference. `--encoding` accepts `auto`, `utf-8`, `gbk`, `shift_jis`, `big5`, and `windows-1252`, with `auto` as the default; specify an encoding if detection is inaccurate. Interface language and source encoding are independent. The encoding option applies to files opened in that process. Arguments after `--` are treated as file paths.

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

Dependency versions and sources are defined in `Cargo.toml` and `Cargo.lock`. See [GPUI dependency notes](docs/gpui-dependencies.md) and [GPU backends](docs/native-gpu-backends.md).

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
| macOS arm64 / x64 | `.dmg`, `.app.zip` | `scripts/build-macos.sh` |
| Ubuntu 24.04+ amd64 / arm64 | `.deb`, `.tar.gz` | `scripts/build-ubuntu.sh` |

Requires Rust and native build dependencies. macOS packaging also needs ImageMagick; Windows needs Inno Setup 6; Ubuntu packaging needs `dpkg-dev` and `desktop-file-utils`. Outputs go to `dist`.

The shell scripts accept `--offline`, `--toolchain NAME`, `--output-dir PATH`, and `--binary PATH` to package an existing executable. Release builds use `cargo build --release --locked` without Python.

Ubuntu releases are built on 24.04 to retain the 24.04 ABI baseline; CI also installs and checks the packages on Ubuntu 26.04. Building locally on a newer Ubuntu can introduce newer library requirements.

macOS defaults to ad-hoc signing and macOS 14.0 minimum. `MACOS_SIGNING_IDENTITY` selects an existing keychain identity; Apple notarization is not configured.

[Release workflow](.github/workflows/release.yml): pushing `vX.Y.Z` builds all platforms and publishes packages plus `SHA256SUMS` after validation. The tag must match the workspace version in `Cargo.toml`. Manual runs only upload Actions artifacts.

## Project structure and documentation

| Location | Purpose |
| --- | --- |
| [crates/pomelo-core](crates/pomelo-core) | Board model, search, selection, and localization |
| [crates/pomelo-import](crates/pomelo-import) | File import and diagnostics |
| [crates/pomelo-render](crates/pomelo-render) | Scene preparation and D3D11, Metal, and wgpu renderers |
| [crates/pomelo](crates/pomelo) | GPUI desktop application and document management |
| [locales](locales) | Five-language interface resources |
| [docs](docs) | General development notes |

- [Documentation](docs/README.md)
- [Development progress](docs/development-progress.md)
- [i18n](docs/i18n.md)
- [GPU](docs/native-gpu-backends.md)

## Development roadmap

These capabilities are planned or need further implementation; they are not delivered features.

| Area | Planned capabilities |
| --- | --- |
| Everyday review | Point and polyline measurement, object clearance, multiple selection, persistent highlight groups |
| Views and output | Named views, navigation history, custom display settings, screenshots, and lightweight local exports |
| Engineering information | Property queries, per-layer pad structures, physical stackup, net groups, connection topology, and selected path lengths |
| Ongoing improvements | File compatibility, large-scene performance, platform stability, keyboard access, and five-language interfaces |

See the [review roadmap](docs/viewer-review-capabilities-plan.md) for priorities and scope, and the [implementation design](docs/viewer-review-development-design.md) for development and acceptance criteria.

The scope remains read-only viewing and review, without editing source designs or running DRC.

## Contributing

Contributions to file compatibility, rendering accuracy, performance, translations, and documentation are welcome. [Report an issue](https://github.com/HaiwenZhang/Pomelo-Desktop/issues) with the source tool/file version, OS and GPU details, reproduction steps, and import diagnostics. A small sample or comparison screenshot helps; remove confidential design data before sharing. Add focused regression tests for parser or geometry changes and screenshots for visual changes. Keep all five README versions in sync.

## License

Project code is licensed under [MIT](LICENSE). Bundled font assets retain their own licenses; see [Source Han Sans](assets/fonts/source-han-sans/README.md).

## Acknowledgments

Thanks to [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) and [GPUI Kit](https://github.com/longbridge/gpui-kit) for the GPU-accelerated UI framework and UI components that power Pomelo Desktop.
