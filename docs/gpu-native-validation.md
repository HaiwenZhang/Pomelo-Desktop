# GPUI 原生 D3D11 / HLSL 验证记录

日期：2026-10-01。结论：Windows 自定义 HLSL 三角形已在 GPUI 自身窗口中实际呈现，无每帧 CPU 回读。用户随后手动确认：“这个方案确实能渲染出来，也没遮挡”。该确认针对本次三角形原型，不扩大为完整 PCB 或跨平台验收。

## 第一次实验：三角形（历史证据）

- 硬件：NVIDIA GeForce RTX 5080；GPUI 报告 `is_software_emulated: false`，驱动 `610.88 r610_85`。
- 框架：GPUI Kit 0.7.0、GPUI pre 0.3.7，Zed 基线 `1a28cff4b409169bac058bca40dfbfeb7621d19b`。
- 提交：GPUI 原有 D3D11 device/context 执行 `native_triangle_vertex` / `native_triangle_fragment`，`DrawInstanced(3, 1, 0, 0)`，48 字节实例布局，最终由 GPUI 原有交换链 `Present`。
- 呈现链路没有 staging texture、GPU→CPU 回读、`RenderImage` 图片上传或独立交换链。报告中的无回读字段描述已检查的实现路径，不是 GPU 性能计数器测量。
- 自动化实窗检查：换色、超出画布的裁剪、对话框实际遮挡三角形及 Escape 关闭、深浅主题、最大化/恢复通过。用户另行完成上述手动确认。
- 工作区 40 项测试、Clippy `-D warnings`、格式检查及 Windows debug 构建通过。单元测试不代替实窗 GPU 证据。

[运行时呈现报告](gpu-validation/native-triangle-d3d11.json) 在 `Present` 成功后生成；截图为运行中的原生窗口。

![原生 D3D11 三角形](gpu-validation/native-triangle-d3d11.jpg)

![GPUI 对话框正确覆盖三角形](gpu-validation/native-triangle-overlay.jpg)

## 第二次实验：通用入口与 pomelo-render 的三种 PCB 图元

日期同为 2026-10-01，已在同一 RTX 5080 上完成实窗检查。**框架补丁只保留通用 GPU 注册与绘制入口，PCB 实现完全放在 `pomelo-render`。** 本节是自动化实窗检查结果，前述用户手动确认仅针对第一次三角形实验。

### 职责划分

| 层 | 当前实现 |
| --- | --- |
| GPUI 补丁 | `NativeGpuRenderer` trait；`Window::register_gpu_renderer`、`Window::paint_gpu`；窗口归属校验、不透明帧快照、scene 顺序/重放、D3D11 状态隔离、reset/present 生命周期 |
| `pomelo-render/src/native_gpu/mod.rs` | `PcbProbeRenderer` 实现 trait；三种图元几何、常量布局、管线缓存、诊断计数 |
| `pomelo-render/src/native_gpu/d3d11.rs` | 使用借用的 GPUI device/context 创建 shader、常量缓冲、blend/scissor 并执行 Draw；局部 unsafe FFI 边界 |
| `pomelo-render/src/native_gpu/pcb.hlsl` | 胶囊距离场走线、圆形距离场圆盘、外方形减内方形的 Zone、网格和边缘覆盖 |
| `pomelo/src/native_gpu_demo.rs` | 注册 renderer、提交不透明场景快照，提供五语实验界面和交互检查 |

走线/圆盘参考 Web 仓库 `src/lib/render/primitive.wgsl` 的距离场语义。这个方形 Zone 使用简单 SDF 差集 `max(outer, -inner)`；孔内像素丢弃，显示先前绘制的网格。Web 的通用多边形 `polygon.wgsl`/铜皮三角化尚未移植，不能据此宣称任意复杂 Zone 已支持。

实验场景按 160×80 单位等比缩放：走线宽 5、圆盘半径 14、Zone 外方形边长 36 / 内孔边长 16。每帧四次 Draw（网格与三种图元），112 字节常量缓冲，HLSL ABI 大小和关键偏移有编译期断言。管线懒创建并复用；这只是小场景实验，不是生产级实例批处理或大板性能结论。

### 实测结果

- 走线两端为圆头；圆盘保持圆形；Zone 的方形孔洞明确透出网格。
- 换色正常，裁剪实验把走线移过左边界，仅绘制画布内部分。
- GPUI 对话框在动画结束后覆盖圆盘，遮罩压暗场景，Escape 关闭后图元恢复。
- 最大化/恢复、深浅主题正常；包含这些操作的报告记录 33 次成功提交/呈现、132 次应用 Draw，管线创建次数仍为 1。
- Windows 最终 debug 构建、40 项工作区测试、Clippy `--all-targets -D warnings`、格式检查通过；五语键与参数、源码 i18n 门禁通过。
- 删除 `vendor/` 后仍可构建并运行；离线从校验过的 crate 归档应用补丁成功，再次准备命中完整哈希校验缓存。GPUI 补丁不含业务几何或 HLSL。
- 实测修复了两处问题：Windows 私有 tempfile 目录导致交互进程读 debug shader 被拒绝，改用继承工作区访问权限的生成目录；HLSL 保留字冲突改名。冲突期间 GPUI 界面仍正常，应用使用五语失败摘要并保留原始技术详情。

[最终构建呈现报告](gpu-validation/pcb-native-d3d11.json)；[换色/缩放/主题操作后的缓存报告](gpu-validation/pcb-native-resize.json)。`cpu_pixel_readback: false` 来自已检查的提交实现，不是性能计数器结果。应用只映射小型常量缓冲上传数据，不回读帧像素，不使用 RenderImage，也不另建交换链。

![三种 PCB 图元，孔洞透出网格](gpu-validation/pcb-native-d3d11.jpg)

![GPUI 对话框覆盖圆盘](gpu-validation/pcb-native-overlay.jpg)

![走线跨边界裁剪](gpu-validation/pcb-native-clip.jpg)

### 复现

```powershell
cargo +stable run -p pomelo --locked -- --native-gpu-demo
```

`Ctrl+R` 换色，`Ctrl+C` 切换裁剪，`Ctrl+O` 打开覆盖层，`Escape` 关闭，`Ctrl+T` 切主题，`Ctrl+Q` 退出。文案提供英文、简中、繁中、日文、韩文。可选 `--triangle` 保留 pomelo-render 内的三角形模式；历史三角形的框架专用入口已经删除。

需要诊断报告时，在启动进程前把 `POMELO_NATIVE_GPU_TRACE` 指向可写文件。应用在 UI render 时读取框架已成功呈现的累计计数，因此初始报告可能是 pending；进行一次换色/主题等重绘后读取。界面状态字段是读取时的值，计数是此前已完成的提交，不能作为精确单帧关联或帧耗时基准。

`pomelo-render` 的 HLSL 源码嵌入可执行程序，首次使用时编译；GPUI 自带 shader 在 debug 模式仍从生成源码路径加载，需保留 `.cache/gpui/`。正式 release 和打包单独验收。

## 维护方式与未验证范围

构建 wrapper 每次校验固定源码归档、版本化补丁及生成文件，必要时重新解压并打补丁；Cargo 通过 `[patch.crates-io]` 指向 `.cache/gpui/`。不修改全局 registry，不再维护完整 `vendor/`。说明及接口约束见 [GPUI 补丁说明](gpui-dependencies.md)。仍需维护补丁与上游的兼容性，不能把“删除 vendor”理解为零框架维护。

原始 GPUI 的 `Element` / `Render` trait 不提供 GPU context。现在 `pomelo-render` 实现的是项目补丁新增的 `NativeGpuRenderer`，不是继承现成的上游 GPU 扩展 trait。

目前只验证 Windows D3D11 小场景。设备丢失已有 reset 通知和缓存清理代码，但未强制触发设备丢失验收；跨窗口拒绝/关闭资源回收尚需专门自动化覆盖。macOS/Linux、release、DPI 切换、真实 BRD 几何、大规模批次、拾取及性能都未通过本轮验收。
