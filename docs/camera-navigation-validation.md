# 相机导航与整板范围验证

> 本文保留历史验证记录，其中引用的辅助脚本及 pomelo-core/import/render 的 example 探针已移除；旧探针命令不再可执行。当前验证使用各 crate 的测试代码。

日期：2026-10-03。参考最新本地 Web `C:/Users/Zen/Desktop/gitrepo/pomelo`，Web 源码和 `E:/brd_cases` 案例只读。**桌面及 Web UI 界面字体保持不变**；本轮没有修改主题、字体资源或翻译文件。Windows 仍使用 GPUI 同设备 D3D11/HLSL，没有引入 wgpu。

## 实现

- 共享 `pomelo-core::interaction::ViewportNavigation` 与实际 Web `BoardViewport` 对齐：首次有效布局适应整板，此后调整窗口或侧栏尺寸保留相机；显式“适应整板”才重新计算缩放。已有恢复相机在首次布局后仍保留。
- 整板适应采用 `0.86 × min(可用宽 / max(板宽, 0.001), 可用高 / max(板高, 0.001))`。Desktop 侧栏位于画布外，直接使用画布逻辑尺寸，不重复扣除 Web 的浮动侧栏 inset。通用 `Camera::fit` 的像素 padding 接口继续保留供诊断调用。
- 定位采用 `0.72 × min(400, 可用宽 / max(目标宽, 3), 可用高 / max(目标高, 3))`，最高 288 逻辑 px/mm。鼠标锚点缩放下限调整至 0.01 px/mm，上限仍为 10⁷；翻板保持相机比例。
- 界面百分比改为当前比例相对最近显式整板适应比例：适应为 100%，放大 1.2 倍为 120%；平移、定位、翻板或 resize 不重置基准。恢复视角的初始基准由首次有效布局建立。
- 导入器对存储板框与图形板框都计入完整线段范围，包含圆弧极值与线宽；尺寸绘图和注释仍不扩大整板边界。
- 静态首次上传进度从额外底部行移到画布内。进度出现/消失不改变画布尺寸，避免首次相机按较小画布适应；动态曲边准备继续只影响完整就绪状态。复用已有五语 `TraceUpload` 和 `ZoomPercent` 消息，不新增未翻译界面文字。

## 独立 Web 对照

`scripts/check-camera-navigation-parity.mts` 直接加载当前 Web 的 `BoardViewport` 和实际 BRD 导入器，仅模拟 canvas 的逻辑边界；没有重新实现 Web 相机公式。原生探针执行正式共享导航类。运行时 Node 24.18.1 / V8 13.6.233.17-node.50。

四组合成边界及三块真实板边界，分别覆盖四种画布尺寸与正反面。操作包含初次布局、显式适应、resize、极小/极大锚点缩放、平移、翻板、点/横长/竖长目标定位。合计 **56 组、924 个状态，0 差异**；比例与百分比最大相对误差为 0，中心及三处屏幕反投影点最大逻辑像素误差为 **0.00029103830456733704 px**。

比例与百分比容差为 10⁻¹²，逻辑像素位置容差为 0.001 px。远坐标在最高倍率下，原生绝对中心与 Web 相对中心的 f64 加减会产生上述末位差异，不宣称逐位相同。报告沿用字段名 `max_physical_point_error`，实际按逻辑比例换算，只有 DPI=1 时等于物理像素误差。修正前的合成对照为 528 个状态全部不匹配；状态同时比较相机和 UI 百分比，不能将这个数字解释为全部几何位置错误。

真实板还使用实际 `pcb_inspect decode-scene` 独立核对原生场景边界，三块板的最大边界误差均为 **0 mm**（门禁容差 10⁻⁶ mm）。

| 案例 | 编码 | Web 与原生整板边界（mm） |
| --- | --- | --- |
| USBC_FPC.brd | UTF-8 | (-78.50570880652585, -46.72444643528062) → (7.94, 33.111933164716575) |
| camera_test_board.brd | UTF-8 | (-70.25, -57.6) → (96.95, 57.6) |
| AGILEX_I_SERIES.brd | Windows-1252 | (-9.8933, -9.462261999999999) → (308.93766, 203.737718) |

旧 FPC 原生最大边界为 `(2.0202, 33.047200000000004)`，漏计图形板框及完整曲边范围；该问题会导致相机公式正确但实际整板视野不同。修正前后 JSONL 与最终对照保存在 `.cache/canvas-parity/`，源 BRD 未修改。

## 自动与 D3D11 验证

工作区全目标全特性 **426 项通过、0 失败、17 项默认忽略**。新增六项共享导航集成测试覆盖适应比例、resize 保留、百分比基准、点定位上限、恢复相机、缩放下限及非法输入；新增两项导入合成测试覆盖仅图形板框、存储板框与线宽边界。Clippy 全目标全特性 `-D warnings`、格式检查、Windows debug/Release 构建通过。

另行显式执行 `hardware_trace_pixels_preserve_caps_arc_hole_clip_and_cache`，**1 项真实 D3D11 硬件测试通过**，包含新导航相机、锚点缩放、平移、板框与几何缓存复用。该测试此前把桌面导航像素与旧 renderer padding 后备相机比较，因正式整板比例改变而失败；现改为独立指定中心 `(10,10)`、比例 `5.504 px/mm` 的参考相机，并调整解析采样位置。旧后备相机的图元检查继续保留，没有改变 shader 或放宽像素断言。默认忽略测试不计作通过。

日志：`camera-workspace-final.log`、`camera-clippy-final.log`、`camera-fmt.log`、`camera-debug-build-final.log`、`camera-release-build-final.log` 和 `camera-hardware-trace.log`，均位于 `.cache/canvas-parity/`。

## Windows 实窗

以独立简中深色配置启动最终 Release，实际打开 FPC。首次最终画布为 `894.6666870117188 × 741.3333129882812` 逻辑像素，相机中心 `(-35.282854403262924, -6.806256635282022)`，比例 `7.985665837606998 px/mm`，与最终尺寸的 Web 整板公式一致。

Computer Use 实际观察窗口，并依次点击放大、翻板和适应按钮：**100% → 120% → 翻板仍为 120% → 适应回到 100% 且保持翻板**。对应比例为 `7.985665837606998 → 9.582799005128386 → 9.582799005128386 → 7.985665837606998`；ready 为 true，NVIDIA GeForce RTX 5080、GPUI_D3D11，呈现记录无 GPU 错误。没有操作用户已有窗口或写入日常配置；验证窗口正常退出。

冻结证据：`camera-window-initial.json`、`camera-window-zoom.json`、`camera-window-flipped.json`、`camera-window-refit.json` 和 `camera-launch-final.json`。这是指定按钮的实窗验证，不代表全部键盘、搜索定位、鼠标滚轮响应、多 DPI 或整板 WebGPU 截图对照通过。

本阶段 SHA-256：

- debug：`6bf6c452be8898d0d218db1b6a6e28db2097aeb9f1fc12cca814c8401a25d6f9`
- Release：`0033b464be6026faf2b1db074f8f9f49605d37d9edc738972160e7d1df081785`
- 相机探针：`6a6a5298204b1ac9c48d768c8f30ed6030af585846596c1b7a5fd67b614db67d`
- 保持原状的 `crates/pomelo/src/theme.rs`：`2cb472958b85ea4a32316961c2902b535903697719b9743231743bf1e5c0ed70`

参考 Web 六个源文件和 BRD 指纹在 `camera-real-final/report.json`，本阶段源码、二进制和证据指纹在 `camera-provenance.json`。此前验证记录为各阶段冻结结果，不因本次更新而改写其历史边界值和二进制指纹。

## 复验与剩余项

```powershell
cargo +stable build -p pomelo-core --example camera_navigation_probe --locked --offline
cargo +stable build -p pomelo-import --bin pcb_inspect --locked --offline
$env:POMELO_SCENE_BOUNDS_PROBE = "$PWD/target/debug/pcb_inspect.exe"
& C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/.bin/tsx.cmd scripts/check-camera-navigation-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo target/debug/examples/camera_navigation_probe.exe .cache/canvas-parity/camera-recheck E:/brd_cases/USBC_FPC.brd:utf-8 E:/brd_cases/camera_test_board.brd:utf-8 E:/brd_cases/AGILEX_I_SERIES.brd:windows-1252
Remove-Item Env:/POMELO_SCENE_BOUNDS_PROBE
cargo +stable test -p pomelo-render --all-features --lib --locked --offline hardware_trace_pixels_preserve_caps_arc_hole_clip_and_cache -- --ignored --nocapture
```

上述画布尺寸与缩放使用逻辑像素，不是独立 DPI 验收。仍需实测 Windows GPUI 与浏览器的滚轮 delta 映射；当前对照给定相同比例因子，没有验证两端每一格滚轮产生相同比例。搜索/元件定位的目标集合、完整 resize/恢复交互、全案例边界回归、整板视觉及性能/设备恢复/发布验收仍待完成。macOS/Linux 未开发、未验证。
