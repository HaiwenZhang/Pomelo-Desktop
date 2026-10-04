<p align="center">
  <img src="crates/pomelo/assets/pomelo.svg" width="120" alt="Pomelo logo">
</p>

<h1 align="center">Pomelo Desktop · PCB Viewer</h1>

<p align="center">
  <strong>逐层看清每一处连接。</strong><br>
  使用 Rust 与 GPUI Kit 构建的原生 PCB 查看器。在 Windows 上通过本地文件处理与 GPU 渲染，探索 Cadence Allegro 电路板。
</p>

<p align="center"><a href="README.md">English</a> · 简体中文 · <a href="README_zh-TW.md">繁體中文</a> · <a href="README_ja.md">日本語</a> · <a href="README_ko.md">한국어</a></p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-Windows-0078D4" alt="Windows">
  <img src="https://img.shields.io/badge/built_with-Rust_%2B_GPUI_Kit-58752c" alt="Rust + GPUI Kit">
  <img src="https://img.shields.io/badge/rendering-D3D11-58752c" alt="D3D11">
  <img src="https://img.shields.io/badge/status-in_development-d77d8a" alt="In development">
</p>

![电路板工作区](images/pcb-example-zh-cn.png)

## 更清晰地查看电路板

- **设计文件留在本地。** 在电脑上导入和查看电路板，无需上传；查看操作不会修改源文件。
- **原生 GPU 渲染。** 使用 D3D11 与 HLSL 显示走线、圆弧、焊盘、过孔、铜皮及孔洞、绘图和 MSDF 板文字。
- **追踪连接。** 搜索网络与元件，定位搜索结果，并检查选中对象的属性。
- **控制显示。** 管理图层，按图层或网络着色，调整铜皮透明度、焊盘填充和标签显示。
- **同时查看多块板。** 支持多文档、最近文件与视图偏好保存，方便继续查看设计。
- **选择熟悉的语言。** 提供简体中文、繁体中文、英文、日文、韩语界面，以及浅色和深色主题。

> **开发中：** 完整 BRD MVP 验收尚未完成。显示精度、大板性能、DPI／输入法和长期运行稳定性仍需更广泛验证。本项目是只读查看器，不编辑电路板，也不执行 DRC。验证范围见[开发进展](docs/development-progress.md)。

## 支持的格式

当前支持 **Cadence Allegro 二进制 `.brd` 文件**。这不代表支持其他 EDA 工具使用的同名扩展名。导入范围与显示精度取决于文件版本及其内容。Web 版支持的其他格式尚未接入本桌面项目。

## 快速开始

需要 **Windows x64**、支持 **Direct3D 11** 的显卡与驱动、**Git**、**Python 3.12+**，以及 Rust、**MSVC C++ 构建工具和 Windows SDK**。仓库通过 `rust-toolchain.toml` 固定 Rust **1.98.1**，依赖版本以 `Cargo.lock` 为准。

```powershell
git clone https://github.com/HaiwenZhang/Pomelo-Desktop.git
cd Pomelo-Desktop
python scripts/cargo.py run -p pomelo --locked
```

使用 **Ctrl+O**、拖放或启动文件路径打开电路板。在应用菜单或工具栏中选择界面语言。

![欢迎界面](images/welcome-zh-cn.png)

```powershell
python scripts/cargo.py run -p pomelo --locked -- --locale zh-CN --encoding windows-1252 "C:\boards\example.brd"
```

`--locale` 支持 `en`、`zh-CN`、`zh-TW`、`ja`、`ko`，仅对本次启动生效，不覆盖已保存的语言偏好。`--encoding` 支持 `utf-8`、`gbk`、`shift_jis`、`big5`、`windows-1252`，默认严格 UTF-8；所选编码作用于本次进程打开的文件。界面语言与源文件编码相互独立。`--` 后的参数全部作为文件路径。

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

Cargo 包装器会在 `.cache/gpui/` 中准备本地 GPUI GPU 补丁，不修改全局 Cargo registry。首次使用普通 Cargo 或 rust-analyzer 前，执行 `python scripts/prepare_gpui.py`。详见[补丁说明](patches/gpui/README.md)。正常桌面运行不需要 Node.js 或 Web 仓库。macOS 和 Linux 属于后续规划；当前原生应用与 GPU 后端面向 Windows。

```powershell
python scripts/cargo.py build -p pomelo --locked
python scripts/cargo.py test --workspace --locked
python scripts/cargo.py clippy --workspace --all-targets --locked -- -D warnings
python scripts/cargo.py run -p pomelo-import --bin pcb_inspect --locked -- --help
```

| 目录 | 用途 |
| --- | --- |
| [crates/pomelo-core](crates/pomelo-core) | 电路板模型、搜索、选择与国际化 |
| [crates/pomelo-import](crates/pomelo-import) | Allegro 解析与导入诊断 |
| [crates/pomelo-render](crates/pomelo-render) | 场景准备、D3D11 渲染与 HLSL 着色器 |
| [crates/pomelo](crates/pomelo) | GPUI 桌面应用与文档管理 |
| [locales](locales) | 五种语言的界面资源 |
| [docs](docs) | 研发计划与验证证据 |

- [研发计划](docs/native-pcb-viewer-development-plan.md)
- [开发进展与验证范围](docs/development-progress.md)
- [i18n](docs/i18n.md)
- [GPU / ADR](docs/adr/0001-native-gpu-rendering.md)

## 参与贡献

欢迎改进 Allegro 兼容性、渲染精度、性能、翻译和文档。[报告问题](https://github.com/HaiwenZhang/Pomelo-Desktop/issues)时，请附上源工具与文件版本、Windows 和 GPU 信息、复现步骤及导入诊断。小型样本或对照截图有助于定位问题；分享前请移除保密设计数据。解析或几何变更应提供针对性的回归测试，视觉变更应附截图，并保持五种语言的 README 同步。

## 许可证

项目代码采用 [MIT 许可证](LICENSE)。内置字体保留各自的许可证，详见[思源黑体](assets/fonts/source-han-sans/README.md)与[笔画字体](assets/fonts/stroke/README.md)。
