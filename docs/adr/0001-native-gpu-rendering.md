# ADR 0001：复用 GPUI 原生 GPU 渲染管线

日期：2026-10-01。状态：Windows 原生三角形已通过实窗验证，用户手动确认无遮挡；通用回调与 pomelo-render 三种 PCB 图元实验已实窗通过。结果见 [验证记录](../gpu-native-validation.md)。完整 PCB/跨平台尚未验收。

## 决策与原因

用户明确允许不使用 wgpu，并接受像 GPUI 一样为各平台编写 shader。当前 Windows PCB 视口采用 GPUI 同设备的 D3D11/HLSL 路线，复用它的设备、绘制排序、窗口目标和呈现流程。这样可以省去独立渲染器与 GPUI 之间的跨设备纹理桥接；能否满足大板性能仍需实测。

用户随后确认三端采用统一 GPU 注册/绘制入口和 `pomelo-render` 平台后端，**当前只开发 Windows**。macOS/Metal/MSL 和 Linux/wgpu/WGSL 作为后续方案记录，不进入当前实施与发布门槛。完整分层与目标目录见 [研发计划 6.3](../native-pcb-viewer-development-plan.md#63-三端架构方案与当前-windows-开发范围)，Windows 实施顺序与验收见 [6.4](../native-pcb-viewer-development-plan.md#64-当前-windows-实施计划与验收)。

三端采用按平台手写 shader：Windows HLSL、后续 macOS MSL、Linux WGSL，共享几何、批次输入和画面语义。当前不引入跨平台 shader 转译依赖，不新增 macOS/Linux 的业务 shader 或占位后端；2026-10-03 按用户要求移除独立 wgpu 实验代码与直接依赖，只保留历史报告。D3D11 后端及其测试禁止使用 wgpu；共享场景、字体度量与交互逻辑不依赖具体平台。

2026-10-02 实施范围：M0–M6 只交付 Windows D3D11/HLSL 后端、通用 GPUI 补丁维护及真实 BRD 验收。按研发计划 6.4 的顺序推进真实场景、常驻批次、交互、资源恢复和 release 发布；各项诊断同步五语 i18n。macOS/Linux 的上下文、业务 shader、应用构建和打包在后续移植阶段启动。

Windows 产品调用链固定为：应用提交帧快照 → GPUI 场景排序与裁剪 → D3D11 状态隔离 → `pomelo-render` 绘制 PCB 批次 → GPUI 恢复状态并继续绘制界面 → GPUI 交换链呈现。生产视口不使用每帧全画布像素回读；复杂 Zone 从共享轮廓与孔洞三角化结果生成 GPU 批次，方孔 SDF 保留为已验证的小场景实验。

现有 wgpu 三角形已经在 Windows GPU 上验证，但显示路径为 GPU → CPU 回读 → GPUI 图片上传。该历史记录保留为算法参考，不作为原生接入或正式性能通过的证据。见 [实验记录](../gpu-triangle-validation.md)。

## 锁定版本的未打补丁源码事实

本项目为 `gpui-kit 0.7.0` / `gpui-pre 0.3.7`，相关快照描述对应 Zed 修订 `1a28cff4b409169bac058bca40dfbfeb7621d19b`。以下为本机 Cargo registry 源码检查结果，不假定其他 GPUI 分支具有相同接口。

| 平台 | 当前 GPUI 后端 | PCB shader 方案 | 接入位置 |
| --- | --- | --- | --- |
| Windows | Direct3D 11 / DXGI | HLSL，由 pomelo-render 编译并缓存管线 | `gpui-pre-windows` 的 `DirectXRenderer`，共享 D3D11 设备与绘制上下文 |
| macOS（后续） | Metal，框架使用 `metal 0.33` | MSL，由 pomelo-render 管理业务 shader 与管线 | 给 `gpui-pre-apple` 增加通用 encoder 回调，应用借用 GPUI 设备与同帧 command buffer；尚未实现 |
| Linux（后续） | wgpu，框架声明版本 `29.0.4` | WGSL，由 pomelo-render 管理业务 shader 与管线 | 给 `gpui-pre-wgpu` 增加通用 pass 回调，X11/Wayland 窗口转发注册；尚未实现 |

可复核源码文件：

- `gpui-pre/src/elements/canvas.rs`：回调仅接收 `Window`、`App` 和画布范围，未提供任意 GPU shader 提交接口。
- `gpui-pre/src/window.rs`：公开 `paint_path` / `paint_quad`；`paint_surface` 受 macOS 配置限制。
- `gpui-pre/src/scene.rs`：图元进入场景排序、重放和 `PrimitiveBatch`。
- `gpui-pre-windows/src/directx_renderer.rs`：按 scene batch 绘制并由框架 present；Windows `draw_surfaces` 当前为空实现。
- `gpui-pre-windows/src/shaders.hlsl`、`gpui-pre-apple/src/shaders.metal`、`gpui-pre-wgpu/src/shaders.wgsl`：现有平台 shader。

Windows 后端可对照 [锁定修订的上游源码](https://github.com/zed-industries/zed/blob/1a28cff4b409169bac058bca40dfbfeb7621d19b/crates/gpui_windows/src/directx_renderer.rs)。外部纹理合成仍有上游设计讨论；[PR 60573](https://github.com/zed-industries/zed/pull/60573) 在本次查询时已关闭、未合并，不能作为本项目已经可用的公开接口。

Linux 采用框架内部 wgpu 不强制 Windows 或 macOS 使用 wgpu。独立 `wgpu 30.0.1` 依赖已删除；若接入框架 wgpu，版本、设备、队列与资源类型必须对齐，两个版本的 texture 不能直接互传。

## 接入结构

```text
BoardScene / Camera / 可见性 / 场景 revision
    → pomelo-render：共享几何准备、批次与精度拆分
    → PCB viewport：提交带裁剪和排序信息的绘制图元
    → GPUI Scene / PrimitiveBatch
    → GPUI 通用回调 → pomelo-render 平台后端：GPU 缓存 + HLSL / MSL / WGSL
    → GPUI 窗口目标 → GPUI present
```

Windows 补丁现提供 `NativeGpuRenderer` trait、`Window::register_gpu_renderer` 注册入口和 `Window::paint_gpu` 场景绘制入口。GPUI 接收不透明的帧快照，提供借用的 device/context、bounds、clip，负责生命周期及呈现；`pomelo-render` 实现实际几何、shader、管线和 GPU 缓存。PCB 类型与 HLSL 不进入补丁。接口仍是项目补丁 API，并非上游已发布 API。

补丁至少涉及 GPUI 核心场景/绘制入口和 Windows 渲染 crate；正式跨平台版本还需 Apple、wgpu 渲染 crate 的通用入口适配；业务平台 shader 保留在 `pomelo-render`。不能只给 `canvas()` 增加回调便认为完成接入。补丁以 `patches/gpui/` 版本化保存，构建前从固定归档重建到 `.cache/gpui/`，不保留整份 `vendor/`，不直接修改用户全局 Cargo registry；记录基线、许可证、差异与升级方式，保持 GPUI Kit 的类型版本一致。

所有平台共享几何语义、相机、高低位坐标、图层顺序、颜色和选择 ID。平台仅实现提交与 shader；线段/圆弧/焊盘覆盖、抗锯齿、孔洞、透明度等通过相同测试场景比较。Rust 缓冲布局明确字段、字节偏移、对齐和大小；分别核对 HLSL、Metal、WGSL 的 uniform/storage 规则，不假定同一个 `repr(C)` 自动兼容三种 shader。

### 后续平台上下文与资源准备

统一的是应用调用方式及场景语义，平台上下文按目标系统提供类型明确的借用设备与绘制接口。当前 Windows 上下文直接提供 D3D11 device/context；未来 macOS 提供 Metal device/encoder，Linux 提供框架同版本的 wgpu device/queue/pass，并附带目标格式、采样数与 bounds/clip。不得把 GPUI 核心依赖回指到依赖它的平台渲染 crate；需要的原生 API 类型依赖按目标平台声明并对齐版本。

后续接口设计采用“准备资源/上传 → 编码绘制”的阶段边界，分别约束可执行的操作及资源存活期。常驻几何按 revision 缓存，每帧数据使用独立区域或明确偏移，避免同一帧的多个 draw 最终读取被覆盖的常量；在途帧完成前不得复用仍被 GPU 引用的存储。现有 Windows 小场景仍使用单 draw 回调，阶段拆分属于正式批次设计，不属于已验证结果。

Metal/wgpu 计划在正常场景顺序中结束框架当前 encoder/pass，使用保留已有目标内容的自定义 encoder/pass，再继续 GPUI 绘制；具体重绑定、存储/加载、缓冲同步及成本在目标平台实测。Windows 继续使用已验证的 D3D11 状态保存/恢复方式。业务 renderer 不自行提交竞争 command buffer 或 present。

当前只推进 Windows 的批次、缓存及产品功能，保持平台边界清晰；macOS/Linux 的上下文、注册转发、shader、补丁归档和发布实现留到单独启动的移植阶段。新增后端的错误从第一条诊断开始使用共享稳定错误码和五语 i18n。

当前 M0–M6 的应用与 GPU 交付目标均为 Windows x64 / D3D11 / HLSL。平台实现及原生 API 依赖通过 `cfg` 和 Cargo target dependencies 隔离；现阶段只接入 Windows，后续平台沿用相同注册/绘制语义并提供各自的类型化借用上下文。

## 生命周期与合成约束

- GPUI 继续拥有窗口设备、交换链和 present。应用不能在同一个 HWND 上另建竞争交换链。
- 新图元参与场景插入、排序、清理、重放和批次迭代；内容裁剪、滚动、DPI、弹窗覆盖和透明混合必须遵循 GPUI 语义。
- GPU 缓冲按场景 revision 缓存；平移缩放更新相机参数，颜色/选择更新独立小缓冲，不逐帧重新上传整板。
- 缓存身份包含设备 generation；设备恢复、窗口关闭、标签卸载和预算回收不能留下失效句柄。场景重放所引用资源有明确存活期。
- 各 draw 显式绑定所需管线状态，处理自定义绘制后 GPUI 后续批次的状态；不破坏目标、viewport、scissor、混合和资源绑定。
- 绘制可使用 GPU 中间目标、MSAA 或 GPU 内部 resolve；禁止正式交互呈现依赖每帧全画面 CPU 回读。只在截图或测试时回读并单独记录成本。
- 驱动原始错误保留为技术详情；初始化失败、shader/管线失败、设备丢失与资源预算诊断先定义稳定错误及 rust-i18n 五语消息，再接入用户提示。

## 分阶段验证门槛

1. **Windows 自定义 shader 三角形**：使用 GPUI 的 D3D11 设备和窗口目标执行自己的 HLSL；实际 GPUI 画布显示，三角形正常呈现不经过 CPU 回读。记录依赖修订、GPU/驱动、shader、提交路径与窗口证据。调用现成 `paint_path` 可以建立基线，但不算自定义 shader 验证。
2. **同窗口合成**：验证菜单/弹窗覆盖、边界裁剪、resize、最小化/恢复及 DPI 改变。恢复后图形与输入坐标一致，不能只验证独立窗口或离屏 PNG。
3. **常驻批次与真实板**：先用受控几何测资源更新，再使用冻结 Web 输出的真实板场景验证算法；随后接 Rust 导入器。正式程序不依赖 Web 或 Node。
4. **大板与资源治理**：按主计划测 release 帧耗时、上传量、CPU/GPU 内存、设备恢复、关闭/重开与多标签回收；三角形通过不等于 M0 完整通过。
5. **跨平台（后续单独启动）**：Windows 主线完成后，建议按 macOS → Linux 移植 MSL/WGSL；先复用三种图元验证，再用真实场景核对像素、拾取与精度。macOS/Linux 必须实机验收，Linux 分别检查 X11/Wayland；交叉编译不足以证明 shader 正确，当前 Windows 发布不等待这两端。

## 备选与维护成本

现有 `paint_path` / `paint_quad` 已由框架 GPU 渲染，适合快速建立正确性基线；大量对象的 CPU 场景构建、上传和绘制成本需实测。自定义 GPU 批次路线需要维护多个平台 shader 和框架补丁，升级时要复核场景格式、构建与设备恢复路径。该成本列入 M0 重新估算。

若原生扩展的维护成本或性能不达标，再评估 wgpu 同设备纹理合成。Windows 当前 D3D11 与独立 wgpu DX12 设备不天然兼容，需要专门设计共享和同步；上游实验分支不等于已发布支持。独立原生窗口可以直接 GPU 呈现，但嵌入主工作区还涉及裁剪、弹窗层级、焦点与 DPI，首轮不将其作为主线。
