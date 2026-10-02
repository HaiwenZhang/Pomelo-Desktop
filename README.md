# Pomelo-Desktop

Rust + GPUI Kit 原生 PCB Viewer，当前只开发 Windows，PCB 渲染使用 GPUI 原生 GPU 入口与 pomelo-render 自有 HLSL，国际化使用 rust-i18n，支持英/简中/繁中/日/韩。

目前可在桌面后台串行导入 Allegro BRD，构建共享完整 CPU `BoardScene`，并在 Windows GPU 视口显示走线、圆弧和板框。基础几何/padstack、网络、放置、走线/过孔及键合对象有 156 案例的分层证据；六代表板完整文字/尺寸字段、场景统计/对象顺序/边界通过对照。打开流程包含排队、取消与阶段进度；铜皮已准备上传数据，GPU 合成、其他 PCB 图元、交互和完整验收仍在开发。范围见 [文字与场景验证](docs/brd-annotation-scene-validation.md) 和 [板框与铜皮准备验证](docs/outline-copper-preparation-validation.md)。

```powershell
python scripts/cargo.py run -p pomelo --locked
python scripts/cargo.py run -p pomelo --locked -- --locale ja --encoding windows-1252 E:\brd_cases\AGILEX_I_SERIES.brd
python scripts/cargo.py run -p pomelo --locked -- --native-gpu-demo
python scripts/cargo.py run -p pomelo --locked -- --gpu-demo
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
