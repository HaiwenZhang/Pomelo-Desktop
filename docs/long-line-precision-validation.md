# 长直线深度缩放精度验证

日期：2026-10-03。参考最新 Web 的 `src/lib/render/primitive.wgsl` 和 `line-frame.wgsl`；Web 源码及 BRD 案例只读。本次修改 Windows PCB 的 HLSL 距离算法，UI 界面字体、布局和五语消息保持现状，没有引入 wgpu。

## 问题与实现

原生直线 fragment 在 f32 中计算 `ap - ab * t`。当端点远在视口外而细线被深度放大时，大数相减丢失亚像素距离；已有高位/残差坐标仍不足以避免这一步的消减误差。

最新 Web 对投影长度超过 16,384 逻辑像素的直线建立补偿精度的局部坐标：单位切向量、有符号法向偏移、起点与终点的沿线投影。原生 `trace.hlsl` 已采用相同算法，在 vertex 阶段用已有双浮点加法/乘法构造坐标，fragment 用屏幕点计算法向距离与圆端帽距离。

阈值按逻辑像素计算，除去 DPI；短线继续走原路径，完全离屏的退化四边形跳过补偿计算。仅复用 vertex 输出并设置局部种类标记，不修改共享场景、128 字节实例 ABI、CPU 拾取、上传预算或缓存身份。普通走线、Zone 直线边界、自定义焊盘直线边界及绘图直线均使用这条管线。圆弧路径保持原实现。

## 先复现，再修正

硬件测试 `hardware_long_lines_match_f64_capsules_at_deep_zoom` 在真实 D3D11 设备上运行生产 HLSL，将诊断图像与独立 f64 最近点圆端帽算法及 Web 的 smoothstep 覆盖率比较。参考算法不使用 shader 的补偿乘法或法向坐标实现。

| 阶段 | 场景 | 结果 |
| --- | --- | --- |
| 修改前 | 初始 432 组 | 157 组失败，最差 RGB 误差 179 |
| 修改后 | 扩展为 960 组 | 全部通过，最大 RGB 误差 1，最高平均 RGB 误差约 0.02405 |
| 原有硬件回归 | 走线/圆弧/端帽/裁剪、铜皮孔洞、焊盘轮廓/类型身份/缓存 | 三项显式执行通过 |
| 工作区测试 | 全工作区、所有特性、锁文件、离线 | 396 通过，12 个硬件/外部案例测试默认忽略，零失败 |
| 静态检查与构建 | fmt、全目标全特性 Clippy `-D warnings`、Windows debug 构建 | 通过 |

960 组覆盖四种直线用途、四个方向（含负向斜线）、200 / 20,000 / 10,000,000 逻辑像素/mm、线身/两端帽/两侧完全离屏、DPI 1/2 和正反面。源坐标约为 `(12345.6789, -9876.54321) mm`，长度约 80 mm；画布具有非零窗口偏移，翻板场景还使用内缩内容裁剪。每组在 128×128 硬件目标上逐 RGB 比较，门槛为最大误差 ≤ 4、平均误差 ≤ 0.05；可见场景同时要求参考有实际覆盖像素。

同一源几何的 20 次相机/DPI/翻转变化后，缓存构建次数仍为 1，上传仍为单实例 128 字节。读回仅存在于硬件诊断测试，生产渲染不进行 CPU 像素读回。

## 真实 FPC 窗口

最新 debug 可执行文件的独立副本在私有配置中打开 USBC_FPC，恢复一条真实斜线边界的视角：

- Segment 31511，Track 14548，Net 644，Layer 1；宽 0.056 mm、长度约 11.999695 mm。
- 源端点 `(-9.1848, 0.44)` → `(-19.7023, 6.2171)`，来自最新 Web 对同一 BRD 的解析。
- 相机为 50,000 逻辑像素/mm，对应约 600,000 逻辑像素的投影长度。只显示所属图层，关闭铜皮、钻孔、板文字和绘图以观察线边。
- 边界正常显示；点击线边内侧拾取并选择 Segment 31511。选择点阵与边缘连续，随后翻板和平移后仍保持连续边界及同一对象选择。
- `GPUI_D3D11`、NVIDIA GeForce RTX 5080，ready 为 true、GPU 无错误。翻板和平移后走线缓存仍为 1，已上传字节仍为 400,768，没有因相机变化重传整板。

窗口以正常 Alt+F4 关闭，进程退出码为 0。配置与可执行文件均为项目缓存下的独立副本，不写入日常用户偏好。UI 字体保持现状。本次没有改变控件、命令、焦点或消息；设计检查围绕真实 PCB 查看任务，实际完成已有拾取、翻板、平移流程，不扩展为全 UI/DPI 验收声明。

验证二进制与 `target/debug/pomelo.exe` 的 SHA-256 一致：

```text
c51f1b7121357613e1a85b2cb7d6b96909420cc2e2e79df057e40d82c20d45be
```

## 证据与复验

原始证据位于 `.cache/canvas-parity/`：

- `long-lines-before.log`、`long-lines-before/d3d11-long-line-precision.json`：修改前失败结果。
- `long-lines-after.log`、`long-lines-after/d3d11-long-line-precision.json`：960 组最终硬件结果，包含 adapter、shader SHA-256 和各组误差。
- `long-lines-provenance.json`：Web 两份 WGSL、原生 HLSL、BRD 和应用二进制指纹。
- `long-lines-hardware_*.log`、`long-lines-workspace.log`、`long-lines-clippy.log`、`long-lines-build.log`。
- `long-line-window-source.json`、`long-line-window-launch.json`：真实走线与独立预览的相机/版本。
- `long-line-window-flipped-evidence.json`、`long-line-window-panned-evidence.json`：冻结窗口遥测。
- `long-line-window-selected.jpg`、`long-line-window-flipped.jpg`、`long-line-window-panned.jpg`：未经编辑的窗口截图。

```powershell
$env:POMELO_CANVAS_GPU_REPORT_DIR = "$PWD/.cache/canvas-parity/long-lines-after"
python scripts/cargo.py +stable test -p pomelo-render --all-features --lib --locked --offline hardware_long_lines_match_f64_capsules_at_deep_zoom -- --ignored
```

本记录证明指定直线管线在高倍率下的距离精度，并包含真实窗口验证；不是 WebGPU 与 D3D11 的整板截图对照，也不是帧率或显存性能报告。曲线铜皮的缩放适配、特殊背钻图元、全案例、多 DPI/设备恢复及发布验收仍待完成，见 [画布总体对照记录](canvas-web-parity-validation.md)。当前只开发 Windows。
