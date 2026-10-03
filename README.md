# Pomelo-Desktop

Rust + GPUI Kit 原生 PCB Viewer，当前只开发 Windows，PCB 渲染使用 GPUI 原生 GPU 入口与 pomelo-render 自有 HLSL，国际化使用 rust-i18n，支持英/简中/繁中/日/韩。

目前已接入 Allegro BRD 后台导入、原生 D3D11 视口、走线/圆弧、焊盘/过孔、铜皮孔洞、绘图和 MSDF 板文字，并提供图层控制、网络/元件搜索、选择检查、多文档、最近文件及视图恢复。156 案例的分层解析和部分代表板/交互已有差分与实窗证据，不能等同于完整 MVP 验收。搜索锚点的应用接入、多语排序、全案例整板、DPI/输入法、性能/长期运行和正式发布仍待完成，具体范围见 [开发进展](docs/development-progress.md) 与 [研发计划](docs/native-pcb-viewer-development-plan.md)。

```powershell
python scripts/cargo.py run -p pomelo --locked
python scripts/cargo.py run -p pomelo --locked -- --locale ja --encoding windows-1252 E:\brd_cases\AGILEX_I_SERIES.brd
python scripts/cargo.py test --workspace --locked
python scripts/cargo.py clippy --workspace --all-targets --locked -- -D warnings
python scripts/cargo.py run -p pomelo-import --bin pcb_inspect --locked -- --locale ja --help
python scripts/cargo.py run -p pomelo-import --bin pcb_inspect --locked -- --locale zh-CN --encoding windows-1252 check --stage index --cases-dir E:\brd_cases --report .cache\indexes.jsonl
```

构建需要 Git 和 Python 3.12+。包装器每次检查并准备 GPUI 通用 GPU 补丁，源码生成在 `.cache/gpui/`，不修改全局 Cargo registry。首次使用普通 Cargo 或 rust-analyzer 前先执行 `python scripts/prepare_gpui.py`。详见 [补丁说明](patches/gpui/README.md)。

工具链固定为 `1.98.1`。本机已经安装相同版本的 stable 工具链时，可用 `python scripts/cargo.py +stable ...`；依赖版本以 `Cargo.lock` 为准。打开文件可使用 `Ctrl+O`、拖放或启动参数；语言在应用菜单/工具栏选择器中切换。

桌面启动支持 `--locale en|zh-CN|zh-TW|ja|ko` 和 `--encoding utf-8|gbk|shift_jis|big5|windows-1252`。语言覆盖仅作用于本次启动，不改写保存的语言偏好；编码默认严格 UTF-8，选定编码应用于该次进程打开的文件。语言和源文件编码相互独立。`--` 后的参数全部作为文件路径。

- [研发计划](docs/native-pcb-viewer-development-plan.md)
- [实际进展与验证范围](docs/development-progress.md)
- [BRD 索引实现与差分验证](docs/brd-index-validation.md)
- [BRD 按需记录解码与字段验证](docs/brd-record-decoder-validation.md)
- [BRD 图层、路径与铜皮轮廓验证](docs/brd-geometry-validation.md)
- [BRD Padstack、焊盘与钻孔验证](docs/brd-padstack-validation.md)
- [BRD 完整网络归属与封装/引脚放置验证](docs/brd-connectivity-placement-validation.md)
- [BRD 走线、过孔与键合对象验证](docs/brd-routing-validation.md)
- [BRD 铜皮网格与板框验证](docs/brd-copper-validation.md)
- [i18n 开发契约与术语](docs/i18n.md)
- [wgpu + WGSL 三角形验证](docs/gpu-triangle-validation.md)
- [原生 GPU 实窗验证](docs/gpu-native-validation.md)
- [视口导航实现与验证范围](docs/viewport-navigation-validation.md)
- [板框 GPU 与铜皮批次准备](docs/outline-copper-preparation-validation.md)
- [原生 GPU 渲染路线决策](docs/adr/0001-native-gpu-rendering.md)

`E:\brd_cases` 等外部样本与 `.cache/` 报告不随仓库分发。正常桌面运行不需要 Web 仓库或 Node；跨语言差分脚本仅用于研发验证。

## Windows 安装包

安装 [Inno Setup 6](https://jrsoftware.org/isinfo.php)、Python 3.12+ 和项目要求的 Rust/MSVC 构建工具后，在仓库目录执行：

```powershell
python scripts/build-windows.py
# 指定编译器位置或使用本机 stable 工具链：
python scripts/build-windows.py --iscc "C:\Program Files (x86)\Inno Setup 6\ISCC.exe" --toolchain stable
```

脚本通过现有 GPUI 包装器编译 Windows x64 release，版本读取工作区 Cargo.toml，输出 `dist/Pomelo-<版本>-windows-x64-setup.exe`。支持 `--offline` 和 `--output-dir`。安装到当前用户的 Programs/Pomelo，无需管理员权限，创建开始菜单快捷方式，并提供可选桌面快捷方式和卸载功能。

`crates/pomelo/build.rs` 从 `assets/pomelo.svg` 自动生成 16/24/32/48/64/128/256 像素 ICO 并嵌入 exe；安装包、卸载程序、快捷方式和应用卸载列表共用项目图标。修改 SVG 后重新构建即可，无需手动同步 ICO。普通 Cargo 构建也会嵌入图标。安装包尚未配置代码签名。
