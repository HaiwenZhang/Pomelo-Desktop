<p align="center">
  <img src="crates/pomelo/assets/pomelo.svg" width="120" alt="Pomelo logo">
</p>

<h1 align="center">Pomelo Desktop · PCB Viewer</h1>

<p align="center">
  <strong>逐层看清每一处连接。</strong><br>
  基于 GPUI 和 GPUI Kit 的本地只读 PCB 查看器。
</p>

<p align="center"><a href="README.md">English</a> · 简体中文 · <a href="README_zh-TW.md">繁體中文</a> · <a href="README_ja.md">日本語</a> · <a href="README_ko.md">한국어</a></p>

<p align="center"><a href="https://github.com/HaiwenZhang/Pomelo-Desktop/releases">下载发布版本</a></p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-0078D4" alt="Windows, macOS, Linux">
  <img src="https://img.shields.io/badge/built_with-Rust_%2B_GPUI_Kit-58752c" alt="Rust + GPUI Kit">
  <img src="https://img.shields.io/badge/rendering-D3D11%20%7C%20Metal%20%7C%20wgpu-58752c" alt="D3D11, Metal, wgpu">
  <img src="https://img.shields.io/badge/status-in_development-d77d8a" alt="In development">
</p>

## 更清晰地查看电路板

- **设计文件留在本地。** 在电脑上导入和查看电路板，无需上传；查看操作不会修改源文件。
- **原生 GPU 渲染。** 使用 D3D11/HLSL、Metal/MSL 与 wgpu/WGSL 显示走线、圆弧、焊盘、过孔、铜皮及孔洞、绘图和 MSDF 板文字。
- **追踪连接。** 搜索网络与元件，定位搜索结果，并检查选中对象的属性。
- **控制显示。** 管理图层，按图层或网络着色，调整铜皮透明度、焊盘填充和标签显示。
- **同时查看多块板。** 支持多文档、最近文件与视图偏好保存，方便继续查看设计。
- **选择熟悉的语言。** 提供简体中文、繁体中文、英文、日文、韩语界面，以及浅色和深色主题。

> **开发中：** 兼容性、显示精度、性能及各平台稳定性仍需验证。本项目用于只读查看，不编辑电路板或执行 DRC。见[开发说明](docs/development-progress.md)。

## 支持的格式

支持 `.brd`、`.PcbDoc`、ODB++ 归档（`.tgz`、`.tar`、`.tar.gz`）、`.pcb`、`.kicad_pcb` 和 `edb.def`。导入范围取决于文件版本和已保存数据，未支持的内容通过诊断提示。

所有格式解析均在 `crates/pomelo-import` 中使用 Rust 实现，无需 JavaScript 引擎或 Node.js。

## 快速开始

需要 Git 和仓库固定的 Rust **1.98.1**；依赖版本以 `Cargo.lock` 为准。

| 平台 | 构建与运行条件 |
| --- | --- |
| Windows x64 | MSVC C++ 构建工具、Windows SDK、支持 Direct3D 11 的显卡与驱动 |
| macOS | 支持 Metal 的 Mac、Xcode Command Line Tools |
| Linux | fontconfig、FreeType、xkbcommon、Wayland/X11、ALSA、OpenSSL 开发包及支持 Vulkan 的驱动 |

下方构建命令适用于三个平台。macOS 的应用快捷键使用 Cmd 代替 Ctrl。

```powershell
git clone https://github.com/HaiwenZhang/Pomelo-Desktop.git
cd Pomelo-Desktop
cargo run -p pomelo --locked
```

使用 **Ctrl+O**、拖放或启动文件路径打开电路板。在应用菜单或工具栏中选择界面语言。

```powershell
cargo run -p pomelo --locked -- --locale zh-CN --encoding windows-1252 "example.brd"
```

`--locale` 支持 `en`、`zh-CN`、`zh-TW`、`ja`、`ko`，仅对本次启动生效，不覆盖已保存的语言偏好。`--encoding` 支持 `auto`、`utf-8`、`gbk`、`shift_jis`、`big5`、`windows-1252`，默认 `auto`；自动识别不准确时可显式指定编码。界面语言与源文件编码独立，编码选项作用于本次进程打开的文件。`--` 后的参数作为文件路径。

## 快捷操作

| 操作 | 按键 |
| --- | --- |
| 打开文件 | `Ctrl+O` |
| 关闭文档 | `Ctrl+W` |
| 重新加载文档 | `Ctrl+R` |
| 下一个／上一个文档 | `Ctrl+Tab / Ctrl+Shift+Tab` |
| 适应电路板 | `F2` |
| 聚焦搜索 | `Ctrl+K` |
| 设置 | `Ctrl+,` |
| 切换左侧／右侧面板 | `Ctrl+B / Ctrl+Shift+B` |

## 构建与开发

依赖版本与来源以 `Cargo.toml` 和 `Cargo.lock` 为准。见 [GPUI 依赖说明](docs/gpui-dependencies.md)及[GPU 后端](docs/native-gpu-backends.md)。

```powershell
cargo build -p pomelo --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -p pomelo-import --bin pcb_inspect --locked -- --help
```

### Release 打包

| 平台／架构 | 产物 | 命令 |
| --- | --- | --- |
| Windows x64 | `.exe` | `scripts\build-windows.bat` |
| Windows ARM64 | `.exe` | `scripts\build-windows.bat --arch arm64` |
| macOS arm64 / x64 | `.dmg`, `.app.zip` | `scripts/build-macos.sh` |
| Ubuntu 24.04+ amd64 / arm64 | `.deb`, `.tar.gz` | `scripts/build-ubuntu.sh` |

需要 Rust 和各平台原生构建依赖；macOS 另需 ImageMagick，Windows 另需 Inno Setup 6，Ubuntu 另需 `dpkg-dev` 和 `desktop-file-utils`。产物输出到 `dist`。

Shell 脚本支持 `--offline`、`--toolchain NAME`、`--output-dir PATH`，以及 `--binary PATH` 打包已有程序。正式构建使用 `cargo build --release --locked`，无需 Python。

Ubuntu Release 固定在 24.04 构建，保持 24.04 的 ABI 基线，并在 CI 中检查 26.04 安装和运行。若本地在更新的 Ubuntu 构建，可能引入更新的库版本要求。

macOS 默认临时签名，最低版本为 macOS 14.0。可设置 `MACOS_SIGNING_IDENTITY` 使用钥匙串中的签名身份，尚未配置 Apple 公证。

[Release 工作流](.github/workflows/release.yml)：推送 `vX.Y.Z` 标签后构建全部平台，通过验证后发布安装包和 `SHA256SUMS`；标签必须与 `Cargo.toml` 的 workspace 版本一致。手动触发仅上传 Actions 产物。

## 项目结构与文档

| 目录 | 用途 |
| --- | --- |
| [crates/pomelo-core](crates/pomelo-core) | 电路板模型、搜索、选择与国际化 |
| [crates/pomelo-import](crates/pomelo-import) | 文件导入与诊断 |
| [crates/pomelo-render](crates/pomelo-render) | 场景准备与 D3D11、Metal、wgpu 渲染 |
| [crates/pomelo](crates/pomelo) | GPUI 桌面应用与文档管理 |
| [locales](locales) | 五种语言的界面资源 |
| [docs](docs) | 通用开发说明 |

- [文档](docs/README.md)
- [开发进展与验证范围](docs/development-progress.md)
- [i18n](docs/i18n.md)
- [GPU](docs/native-gpu-backends.md)

## 开发计划

以下为待开发或待补齐的能力，尚不代表已交付功能。

| 阶段 | 计划内容 |
| --- | --- |
| 日常审阅 | 两点与折线测量、对象净距、任意多选、持久多组高亮 |
| 查看与输出 | 命名视图、导航历史、自定义显示、截图及轻量本地导出 |
| 工程信息 | 属性查询、逐层焊盘结构、物理叠层、网络分组、连接拓扑与指定路径长度 |
| 持续改进 | 文件兼容性、大型场景性能、跨平台稳定性、键盘操作与五语界面 |

详细优先级与功能边界见[审阅能力规划](docs/viewer-review-capabilities-plan.md)，实施与验收要求见[开发设计](docs/viewer-review-development-design.md)。

功能围绕只读查看与审阅展开，不编辑源设计或执行 DRC。

## 参与贡献

欢迎改进文件兼容性、渲染精度、性能、翻译和文档。[报告问题](https://github.com/HaiwenZhang/Pomelo-Desktop/issues)时，请附上源工具与文件版本、操作系统和 GPU 信息、复现步骤及导入诊断。小型样本或对照截图有助于定位问题；分享前请移除保密设计数据。解析或几何变更应提供针对性的回归测试，视觉变更应附截图，并保持五种语言的 README 同步。

## 许可证

项目代码采用 [MIT 许可证](LICENSE)。内置字体保留各自的许可证，详见[思源黑体](assets/fonts/source-han-sans/README.md)。

## 致谢

感谢 [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) 和 [GPUI Kit](https://github.com/longbridge/gpui-kit) 项目，为 Pomelo Desktop 提供 GPU 加速的 UI 框架和 UI 组件。
