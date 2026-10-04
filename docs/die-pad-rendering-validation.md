# Die pad 显示分类与拾取验证

> 本文保留历史验证记录，其中引用的辅助脚本及 pomelo-core/import/render 的 example 探针已移除；旧探针命令不再可执行。当前验证使用各 crate 的测试代码。

日期：2026-10-03。参考最新本地 Web `C:/Users/Zen/Desktop/gitrepo/pomelo`，Web 与 `E:/brd_cases` 只读。UI 界面字体保持原状，未新增界面文案或修改翻译资源。

## 问题与修正

Web `board/shapes/pin.ts` 的 `pinDisplayCategory` 将 die pin 归入 `etch`；`render/primitive-batch-builder.ts` 将 `die-pad` 特殊层的 pin 几何按相同类别提交。原生 CPU 拾取及 MSDF 标签已经采用这个分类，但 D3D11 仍在普通 `Pin` 命令中绘制，导致显示开关和提升顺序不一致。

导入器将 die pad 映射到共享模型的 `LayerId::BOND_TOP`（131072）。本次调整 `backend/d3d11/copper_renderer.rs`，在该层的 `Trace` 命令中提交解析焊盘、自定义填充和非填充边界，并跳过该层的普通 `Pin` 命令。可见性使用 `LayerPrimitive::Traces`，提升顺序使用 `DisplayCategory::Trace`；owner 仍为类型化 `SelectedObject::Pin`，不新增业务对象。其他层的普通 pin/via 路径保留原分类。

业务图元仍由 `pomelo-render` 管理，通过 GPUI 同设备 D3D11 入口绘制。未修改 GPU ABI、HLSL、UI 字体或引入 wgpu。

## 自动验证

| 范围 | 结果与证据 |
| --- | --- |
| 修正前正式管线复现 | 120 组中 60 组分类/顺序失败；`die-pad-before.log` 与 `die-pad-before/d3d11-die-pads.json` |
| 修正后真实 D3D11 | 120 组全部通过；`die-pad-after.log` 与 `die-pad-after/d3d11-die-pads.json` |
| 真实 BRD 与 Web 拾取/选择 | 29 个 die pad、116 个点、3 档缩放、10 个状态，3,480 次全部匹配，0 差异；`die-pad-camera/report.json` |
| 普通与特殊图元硬件回归 | 背钻、走线端帽/圆弧/裁剪、自定义焊盘边界、铜皮重叠孔洞四项显式通过；`die-pad-regression-*.log` |
| 工作区全特性测试 | 401 通过、0 失败、14 项硬件/外部案例默认忽略；`die-pad-workspace.log` |
| 格式与全目标全特性 Clippy `-D warnings` | 通过；`die-pad-clippy-final.log` |
| Windows debug/Release 构建 | 通过；`die-pad-build.log`、`die-pad-release-build.log` |

以上证据位于 `.cache/canvas-parity/`。硬件测试使用物理 D3D11 设备与正式 `BoardRenderer`，不使用软件设备或 WebGPU。120 组覆盖圆形、矩形、自定义带方孔 × 铜线独显/普通焊盘独显/铜线提升/默认重叠/隐藏 die 层 × 填充开关 × DPI 1/2 × 翻板。独立屏幕位置采样材料内部、外边界以及自定义孔中心，检查黄 die pad、蓝普通 pin 和黑背景的可见性/覆盖关系；解析边缘的抗锯齿混合允许蓝色下层透出。此项验证显示语义，不是整幅 Web 截图逐像素比较。

每种几何的焊盘缓存只构建一次；自定义网格/边界缓存也各一次。显示切换前后上传分别保持：解析圆/矩形各 256 字节；自定义场景解析实例 128 字节、自定义网格 176 字节、边界实例 1,024 字节。

真实案例为 `camera_test_board.brd`，UTF-8，SHA-256 `45026cd064852090d3f7763f4cb751c66d6b3c472d90e1f8f927fbfb6b707d56`。第一个 die pin 为 18041，坐标 `(-1.3744, -1.4625)`，矩形宽高均为 0.0761 mm，所在特殊层为 `BOND TOP`。对照工具直接调用最新 Web `BoardIndex`；各点取中心、边内、边外和斜向，缩放为 100/650/3000 逻辑像素/mm。状态包含铜线/普通焊盘独立开关、非填充、分别提升 etch/pin、隐藏层、活动层和只显示 die 层，检查首选对象及 object/track/net/component 四种选择模式。报告保存实际 Web 源文件指纹。

## GPUI 呈现记录与限制

私有配置 `.cache/ui-validation/profiles/48963297f39f49f2a3fbc71558f2b243` 使用新 debug 二进制副本，SHA-256 `1628c8af4a37da2a103560e47b990b0c524e31dc5dfd8780a4bdcca6f824234b`。恢复上述 pin 坐标，1000 逻辑像素/mm，仅显示 die 层，设置铜线开启、普通焊盘关闭。

`die-pad-window-presented.json` 冻结 GPUI_D3D11、NVIDIA GeForce RTX 5080、硬件渲染、ready、9 次提交/呈现、`last_error: null`，焊盘发生实际 draw。预览进程运行时，Computer Use 未在可操作列表中找到它的窗口；本轮没有完成截图检查、窗口内点击拾取或键盘切换，不能把遥测视为实窗视觉验收。呈现记录冻结后已核实路径并停止本次私有预览进程；未操作已有窗口或日常配置。

Release SHA-256 为 `a658056b439978ef6397d1cd91065a0a110841a7f7ef73239ffa8b283c9953fd`，PE 子系统为 2（Windows GUI），构建通过但尚未新增 Release 实窗验证。源码、参考 Web、应用与探针指纹见 `die-pad-provenance.json`。当前工作区已有构建资源依赖声明；为继续使用 `--locked`，锁文件仅补齐应用的既有 `embed-resource` 与 `resvg 0.46.0` 引用，未升级依赖版本。

这次验证覆盖 die pad 显示分类，完整整板视觉对照、全部特殊层组合、多 DPI 实窗/设备恢复、性能和发布验收仍未完成。

## 复验

```powershell
$env:POMELO_CANVAS_GPU_REPORT_DIR = Join-Path $PWD '.cache/canvas-parity/die-pad-after'
cargo +stable test -p pomelo-render --all-features --lib --locked --offline hardware_die_pads_follow -- --ignored
cargo +stable build -p pomelo-render --example canvas_pick_probe --locked --offline
& C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/.bin/tsx.cmd scripts/check-canvas-picking-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo target/debug/examples/canvas_pick_probe.exe E:/brd_cases/camera_test_board.brd .cache/canvas-parity/die-pad-camera utf-8 die-pads
cargo +stable test --workspace --all-features --locked --offline
cargo +stable clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
```
