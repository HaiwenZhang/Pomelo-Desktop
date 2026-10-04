# GpuPainter 底层实现记录

日期：2026-10-03。实现位置：`references/zed`。本次新增 GPUI 直接渲染扩展，不改变 Pomelo 当前的 Cargo 依赖、不迁移现有 `NativeGpuRenderer` 调用，也不添加 CAD 业务着色器或着色器转译库。

## 已实现接口

核心接口位于 `references/zed/crates/gpui/src/gpu_painter.rs`，只包含生命周期、句柄、目标几何及场景数据。原生回调由各后端 crate 提供：

| 后端 crate | 应用实现的 trait | 借用上下文 | 注册适配器 |
| --- | --- | --- | --- |
| `gpui_apple` | `MetalPainter` | `MetalPaintContext` | `metal_painter(...)` |
| `gpui_windows` | `D3D11Painter` | `D3D11PaintContext` | `d3d11_painter(...)` |
| `gpui_wgpu` | `WgpuPainter` | `WgpuPaintContext` | `wgpu_painter(...)` |

以 Metal 为例：

```rust
use gpui::GpuResetReason;
use gpui_apple::{MetalPaintContext, MetalPainter, metal_painter};

impl MetalPainter for MyPainter {
    fn paint(
        &mut self,
        context: &mut MetalPaintContext<'_>,
        data: &(dyn Any + Send + Sync),
    ) -> anyhow::Result<()> {
        // 使用 context.device、command_buffer 和 color_target 编码绘制。
        // 保留已有颜色，并遵守 context.target 的裁剪和 opacity。
        Ok(())
    }

    fn reset(&mut self, reason: GpuResetReason) {
        // 释放设备资源；resize 不触发完整 reset。
    }
}

let painter = window.register_gpu_painter(metal_painter(MyPainter::new()))?;
// 在元素 paint 阶段调用；三个后端共用此入口：
window.paint_gpu(bounds, &painter, Arc::new(frame_snapshot))?;
```

每种原生上下文的 `target` 都是核心的 `GpuPaintTarget`，包含物理尺寸、bounds、clip、DPI、sample count 和 opacity。应用负责 viewport/scissor、着色器、pipeline 和深度附件。GPU 回调按场景顺序执行，不在 `paint_gpu` 调用时立即执行。

后端工厂返回实现 `gpui::GpuPainter` 的适配器。注册时检查适配器类型，错误后端在注册阶段返回错误。绘制时通过安全的 `Any` downcast 获取适配器，再由后端构造借用的原生上下文；不会把非静态帧引用塞进 `Any`，也不使用裸指针转换。核心 `GpuPainter` 只定义 `reset`，具体 `paint` 签名属于后端 trait，核心不存在包含原生类型的 backend enum。

`GpuPainterHandle::status()` 记录编码成功数、reset 数和最近错误，不把编码成功等同于 GPU 完成或显示完成。

## 三端接入

| 位置 | 行为 |
| --- | --- |
| `gpui_windows/src/directx_renderer.rs` | D3D11.1 状态切换，提供窗口 RTV，回调结束通过 RAII 清除应用绑定并恢复 GPUI 状态 |
| `gpui_apple/src/metal_renderer.rs` | 结束 GPUI encoder，提供同一 command buffer 和颜色 texture，回调后以 Load 恢复 GPUI encoder |
| `gpui_wgpu/src/wgpu_renderer.rs` | 结束 GPUI pass，提供当前 encoder 和颜色 view，回调后以 Load 恢复 GPUI pass |
| Windows、macOS、Linux X11/Wayland 窗口 | 将 painter 生命周期注册转发到所属 renderer |

GPU 绘制使用独立的 `GpuPaintSurface` 和 `Scene::gpu_paints`，通过 `Scene::render_batches()` 返回的 `SceneBatch` 与普通 UI 按顺序交织。原有 `PaintSurface`（包括 Apple 的 `image_buffer` 字段）、`Primitive`、`PrimitiveBatch` 和 `Window::paint_surface()` 保持上游定义。`Scene::batches()` 继续返回原有图元批次；新的三个后端绘制入口使用 `render_batches()`。排序、裁剪、缓存重放及清空支持独立的 GPU 命令，不再修改视频 payload。

核心 `gpui/Cargo.toml` 已恢复为上游版本，本次新增的 Metal、wgpu 和 D3D11 feature 依赖全部移除。原生资源类型只出现在对应后端 crate，沿用各后端已有的图形库依赖。Metal 示例移到 `gpui_apple/examples`，仅为 Apple 示例添加 macOS 下的 `gpui_platform` dev-dependency。上游原有的平台依赖不受影响。

平台 renderer 持有弱注册表，覆盖从未绘制过的 painter。设备替换前通知 `DeviceReplaced`；窗口销毁通知 `WindowDestroyed` 并清空注册表，后续 Drop 不重复通知。wgpu 恢复时将注册表转移到新 renderer，保留存活的应用句柄。

主画面不创建应用颜色中间目标。深度、模板和拾取等辅助目标由应用管理，必须匹配实际附件要求。每个应用 encoder/pass 都须在返回前结束；当前帧目标不得跨帧缓存，回调不得自行提交、present、跨线程使用上下文或重入 GPUI。失败不撤销已经编码的绘制命令。

## 示例

`references/zed/crates/gpui_apple/examples/gpu_painter_metal.rs` 展示 macOS 上的直接绘制：应用自己创建 Metal shader、pipeline 和深度附件，先画近三角形再画远三角形，使用深度测试控制遮挡，并与普通 GPUI UI 同窗口显示。

在 Zed 根目录构建：

```sh
cargo build -p gpui_apple --example gpu_painter_metal \
  --features gpui_apple/runtime_shaders
./target/debug/examples/gpu_painter_metal
# 自动观察首次回调结果后退出：
./target/debug/examples/gpu_painter_metal --smoke-test
```

示例只验证 Metal 用法；三端后端不要求应用分别维护 shader 源码，后续可由应用库通过 Naga 或 GLSL/SPIR-V 工具链生成各自 shader。本次不包含该转译工具链。

## 验证与限制

- 在原始 Zed workspace 执行 `cargo check -p gpui -p gpui_apple -p gpui_macos -p gpui_wgpu --features gpui_apple/runtime_shaders --offline --locked`，通过。
- 在原始 Zed workspace 中执行 `cargo test -p gpui -p gpui_apple --lib gpu_painter::tests --features gpui_apple/runtime_shaders --offline --locked`，6 项单元测试通过：适配器类型检查、调用结果与错误恢复，设备恢复、弱引用释放、未绘制 painter 的销毁通知、场景重放持有 painter/帧快照、目标裁剪与 DPI/opacity 元数据。
- 按 Zed 的 `AGENTS.md` 使用 `./script/clippy -p gpui -p gpui_apple -p gpui_macos -p gpui_wgpu --offline --locked`；所选 crate 的 all-targets/all-features、release、deny-warnings 检查通过。未安装 cargo-shear，因此脚本未执行其后续可选检查。
- Metal 示例编译检查和可执行文件构建通过。实窗 smoke test 输出 `GPUPAINTER_SMOKE encoded=1 error=None`，验证应用 MSL 编译、pipeline/深度附件创建及直接绘制回调。该计数不证明 GPU 已完成；深度遮挡截图和完整交互验收尚未执行。
- 本机缺少 Xcode Metal Toolchain，使用上游已有 `runtime_shaders` feature 验证；没有修改上游默认预编译方式。
- Windows、Linux X11/Wayland 目标构建与实窗验收尚未执行；wgpu painter 接口按渲染后端提供，已去掉 Linux 限定，并在 macOS 上通过编译检查；不因此改变各平台默认 renderer 的选择。
- 尚未切换 Pomelo 依赖。后续依赖切换需统一 GPUI Kit 的整个类型来源；不能让原来的 `gpui-pre` 和这份 `gpui` 混用。
- 依照 Zed 根目录 `AGENTS.md`，已在其 `README.md` 顶部保留人工审阅提示。

设计目标和 CAD 分层见 [GPUI 三端 CAD 直接渲染方案](cad-gpu-painter-design.md)。

## Zed 风格与上游贡献

Rust 类型采用 Zed 现有的 acronym 命名方式：`GpuPainter`、`GpuPaintTarget`、`GpuPainterHandle`、`GpuResetReason`；方案名称仍为 GPUPainter。依赖版本通过 workspace 管理，模块声明与 crate 根重导出分开组织，renderer 使用已有的集中导入方式。锁采用项目已有的 `parking_lot::Mutex`，不再为标准库锁添加 poison 恢复代码。

`Window::register_gpu_painter` 和 `Window::paint_gpu` 位于 `window.rs`。句柄构造和所属窗口查询仅为 `pub(crate)`，内部状态字段保持私有；注册表和回调执行接口因后端位于独立 crate 必须跨 crate 可见，以 `#[doc(hidden)]` 标明实现用途。`snap_bounds` 和 `snapped_content_mask` 保持私有。平台无关逻辑使用模块内普通单元测试；实窗与 GPU 编码行为另由示例验证，单元测试不冒充硬件验证。

提交上游前仍需按 `CONTRIBUTING.md` 与维护者讨论扩展接口，确认新增接口的上游设计。wgpu 的无条件依赖和 `PaintSurface` 字段破坏已修复。Windows/Linux 构建、实际绘制、视频路径回归及性能验证仍需补齐。本地 lint 通过不等于已获上游设计认可。

Zed 允许 AI 辅助，但要求贡献者理解并人工审阅代码，不接受自主代理代为提交。README 顶部的审阅提示按仓库要求保留，由人类作者在完成审阅后处理。

兼容性验证：核心 `gpui/Cargo.toml` 与上游完全一致，未向核心新增任何图形库依赖。`PaintSurface`、`Primitive`、`PrimitiveBatch`、`Window::paint_surface()` 和 Metal 视频绘制函数保持上游定义。场景测试覆盖 UI/GPU/UI 顺序、重放、原批次接口和清空释放。Linux X11/Wayland 注册入口现在显式处理缺失 renderer，返回错误而非对 `Option<WgpuRenderer>` 直接调用方法。
