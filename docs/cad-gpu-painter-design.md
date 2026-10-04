# GPUI 三端 CAD 直接渲染方案

日期：2026-10-02。状态：设计方案；2026-10-03 已在 `references/zed` 实现底层接口与三端接入代码，尚未完成三端实窗验收和应用迁移。本文总结本次确认的目标：维护较小的 GPUI 补丁，公开统一的 `GpuPainter` 注册与绘制接口，由应用分别实现 D3D11、Metal、wgpu 渲染后端，使 Windows、macOS、Linux 呈现相同的 CAD 场景和视觉效果。

主画面直接写入 GPUI 当前帧的颜色目标，不经过应用离屏颜色纹理和最终纹理合成。应用可以创建深度、模板、拾取 ID、阴影等辅助附件。几何内核、CAD 算法、业务着色器和 GPU 缓存留在应用中；GPUI 管理场景顺序、窗口资源和提交呈现。

实现范围与验证结果见 [GpuPainter 底层实现记录](gpu-painter-implementation.md)。本文描述后续三端 CAD 架构，不表示三端已经可用，也不修改现有 Windows 交付的验收结果。当前实施记录见 [ADR 0001](adr/0001-native-gpu-rendering.md)，现有补丁说明见 [GPUI 补丁说明](gpui-dependencies.md)。

## 当前基础与目标边界

项目锁定 `gpui-kit 0.7.0`、`gpui-pre 0.3.7`，快照对应 Zed 修订 `1a28cff4b409169bac058bca40dfbfeb7621d19b`。以下后端信息来自该版本本地源码检查，不推定其他 GPUI 版本具有相同结构。

| 平台 | GPUI 后端 | 应用着色器 | 当前状态 |
| --- | --- | --- | --- |
| Windows | D3D11，位于 `gpui-pre-windows` | HLSL | 已有自定义回调补丁及 PCB 渲染实现；完整 CAD 3D 尚未实现 |
| macOS | Metal，位于 `gpui-pre-apple`，窗口位于 `gpui-pre-macos` | MSL | 接入补丁及应用渲染实现待开发 |
| Linux | wgpu，位于 `gpui-pre-wgpu`；X11 与 Wayland 窗口位于 `gpui-pre-linux` | WGSL | 接入补丁及应用渲染实现待开发 |

现有补丁使用 Windows 专属的 `NativeGpuRenderer`、D3D11 上下文和 `PaintSurface.native_gpu`。它已有窗口注册、不可变帧数据、场景排序、状态隔离和生命周期机制，可以在此基础上演进。`GpuPainter` 是拟采用的项目补丁 API 名称，不是上游已发布接口。

统一 API 指注册、调用、空间信息和生命周期不依赖操作系统。底层上下文仍公开 D3D11、Metal、wgpu 的类型化能力，应用据此创建资源和编码绘制。若连后端类型也全部隐藏，就需要另建 pipeline、buffer、附件和命令抽象，会扩大 GPUI 补丁，本方案不采用。

## 架构与职责

```text
CAD 文档与几何内核
    ↓ 几何细分、边线、稳定对象 ID、资源 revision
共享场景与不可变帧快照
    ↓ 相机、材质、灯光、可见性、选择状态
GPUI 元素调用 Window::paint_gpu
    ↓ 场景排序、bounds、clip
GpuPainter 回调
    ├─ Windows：D3D11 + HLSL
    ├─ macOS：Metal + MSL
    └─ Linux：wgpu + WGSL
    ↓ 直接写 GPUI 当前颜色目标
GPUI 继续绘制 UI，统一提交并呈现
```

| 层 | 负责内容 |
| --- | --- |
| 文档与几何内核 | 拓扑、约束、编辑、撤销重做、文件格式、精确几何计算 |
| 场景准备 | 网格与边线、实例数据、空间索引、可见性、LOD、资源版本 |
| 应用渲染器 | 业务 shader、pipeline、GPU 缓存、深度附件、多 pass、拾取与高亮 |
| GPUI 补丁 | 注册句柄、场景绘制命令、当前帧上下文、状态恢复、生命周期通知 |
| GPUI 原有能力 | 窗口、UI、输入、布局、刷新、交换链及呈现 |

应用根据实际渲染后端选择适配器；wgpu painter 接口不限定 Linux，也不改变 GPUI 各平台默认后端。三个后端不在每帧同时执行。它们共享画面语义，但分别实现 shader、资源创建、绑定和 draw 命令；只替换着色器不足以完成移植。

## 公共 API

核心只定义平台无关的生命周期与目标信息。类型化绘制回调属于后端 crate，由适配器接入核心：

```rust
// gpui
pub trait GpuPainter: Any + Send {
    fn reset(&mut self, reason: GpuResetReason);
}

pub enum GpuResetReason {
    DeviceReplaced,
    WindowDestroyed,
}

pub struct GpuPaintTarget {
    pub size: [u32; 2],
    pub bounds: Bounds<ScaledPixels>,
    pub clip: Bounds<ScaledPixels>,
    pub scale_factor: f32,
    pub sample_count: u32,
    pub opacity: f32,
}

// gpui_apple；Windows/Linux 在对应后端提供同样形式的 trait 和工厂。
pub trait MetalPainter: Send + 'static {
    fn paint(
        &mut self,
        context: &mut MetalPaintContext<'_>,
        data: &(dyn Any + Send + Sync),
    ) -> anyhow::Result<()>;
    fn reset(&mut self, reason: GpuResetReason);
}
pub fn metal_painter(painter: impl MetalPainter) -> impl GpuPainter;
```

`MetalPaintContext`、`D3D11PaintContext`、`WgpuPaintContext` 分别由 `gpui_apple`、`gpui_windows`、`gpui_wgpu` 导出，均包含 `target: GpuPaintTarget` 和当前帧借用的原生资源。核心不导出原生 backend enum。适配器通过安全的 `Any` 类型检查分派，帧上下文保持普通 Rust 借用，不通过裸指针擦除生命周期。

窗口公开以下入口：

```rust
pub fn register_gpu_painter(
    &mut self,
    painter: impl GpuPainter,
) -> anyhow::Result<GpuPainterHandle>;

pub fn paint_gpu(
    &mut self,
    bounds: Bounds<Pixels>,
    painter: &GpuPainterHandle,
    data: Arc<dyn Any + Send + Sync>,
) -> anyhow::Result<()>;
```

应用使用 `window.register_gpu_painter(gpui_apple::metal_painter(painter))` 或另两个后端的工厂注册。注册时验证适配器是否匹配后端。注册句柄限定所属窗口。`paint_gpu` 仅在元素 paint 阶段使用，将绘制命令加入场景；GPU 回调在渲染阶段同步执行。场景持有句柄和数据的强引用，窗口生命周期表持有弱引用。`GpuPainterHandle::status()` 保留为诊断入口，GPUI 的内部注册类型不承担应用绘制职责。

`size` 是完整颜色目标的物理像素尺寸；`bounds` 是元素完整区域；`clip` 是元素区域、父级裁剪和目标边界的交集，使用同一个目标坐标系。应用根据完整元素设置相机和 viewport，根据有效裁剪设置 scissor。不能用被裁剪后的尺寸计算相机宽高比。高 DPI 下的细线和标注通过 `scale_factor` 转换。

颜色格式及其编码约定由后端上下文描述，必须明确线性与 sRGB 转换、alpha 约定和目标采样数。空裁剪应跳过调用。连续绘制及场景重放保持插入顺序，不随意合并或重排具有副作用的回调。

### 后端上下文能力

| 后端 | 借用的 GPU 能力 |
| --- | --- |
| D3D11 | device、immediate context、当前颜色 RTV、实际颜色格式 |
| Metal | device、当前 command buffer、当前颜色 texture、实际颜色格式 |
| wgpu | device、可变 command encoder、当前颜色 texture view、实际颜色格式 |

Metal 与 wgpu 必须允许应用创建带自有附件的新 pass，而不是只传入已开启的 UI pass。应用可以在同一帧编码多个窗口目标 pass 和辅助目标 pass。GPUI 在调用前结束自己的 pass，在调用后以保留颜色内容的方式恢复绘制。

公共接口不提供独立 present 或竞争交换链。初版不暴露可随意提交的 queue；应用上传使用 D3D11 映射/更新、Metal 自有 buffer 与 blit、wgpu staging buffer 与当前 encoder 等方式。若后续暴露 queue 写入能力，必须另行明确上传与当前帧提交的顺序，不能绕过 GPUI 的提交管理。

底层图形 API 能力依赖调用约定，不构成沙箱。类型借用可以限制一般使用的存活期，但应用仍须遵守不得提交、不得清除无关区域等约定。

## 主画面直接渲染

```text
GPUI 绘制 viewport 背景
    ↓
结束 GPUI 当前 pass 或保存 D3D11 状态
    ↓
应用绑定 GPUI 当前颜色目标和自己的深度附件
    ↓
实体 → 边线 → 选择高亮 → 操作手柄
    ↓
结束应用 pass，恢复 GPUI 状态
    ↓
GPUI 绘制文字、菜单、浮层
    ↓
GPUI 提交并呈现
```

直接渲染指使用 GPUI 的同一颜色目标，不要求共用同一个 pass。窗口颜色必须保留：Metal/wgpu 使用 `Load`，D3D11 不清除窗口颜色。应用可以清除自己的深度与模板附件。若需要 viewport 背景，优先由 GPUI 在回调前绘制背景图元，而非清除整个窗口。

每个写入窗口目标的 pass 都要显式设置 viewport、scissor、pipeline 和资源绑定。GPUI 恢复自己的状态，不依赖应用最后留下的绑定。透视投影和拾取坐标必须采用相同的 bounds、DPI 和坐标转换。深度纹理可初版按完整目标尺寸分配，便于直接使用窗口坐标；后续缩小到局部附件前须核对 API 尺寸约束和坐标映射。

应用不得保留当前帧颜色目标用于下一帧。D3D11 回调结束应解除自有绑定并释放对当前 RTV 的额外引用，避免阻塞交换链 resize。Metal/wgpu 的当前目标和编码器同样限定本次调用使用。

### 辅助目标与效果限制

GPU ID 拾取、阴影、遮挡查询相关数据可以使用应用自有目标；主画面仍然直接渲染。完全禁止任何辅助目标会限制这些功能，不属于本次确认的范围。

应用的深度附件必须匹配颜色目标的有效尺寸要求和采样数。当前目标是单采样时，不能仅增加多采样深度附件就获得 MSAA。若将来需要 MSAA，应扩展 GPUI 的窗口目标及 resolve 方案，或者另行改变主画面路线。

纯直接路径也不承诺完整 HDR、全屏后处理或跨帧颜色缓存。如果效果需要读取先前颜色，必须按具体 API 的采样、复制、附件冲突规则单独设计。场景无变化但 GPUI 重画并清除目标时，不能假设上帧 3D 颜色仍然存在；可缓存几何和命令准备结果，必要时仍重画主场景。

## 应用侧 CAD 实现

保留现有 crate 边界，新增代码按职责落入真实模块，不为尚未启动的后端创建空占位文件。建议后续结构如下：

```text
pomelo-core：文档、几何、相机、选择、空间索引
pomelo-render：
    scene3d/：网格、边线、实例、材质、灯光、帧快照
    painter3d.rs：GpuPainter 实现与后端选择
    backend/d3d11/、metal/、wgpu/：资源、pipeline、命令编码
    shaders/：对应的 HLSL、MSL、WGSL
pomelo：viewport 输入、不可变快照提交、异步结果与 UI 刷新
```

CAD 文档保持双精度几何；GPU 采用相机相对坐标或高低位拆分，防止大坐标和大缩放范围下抖动。精确模型与显示用细分网格分开管理。CPU 拾取及几何运算不以低精度 GPU 顶点作为唯一真值。

GPU 缓存以稳定资源 ID、revision 和设备身份组织。相机变化更新小块帧数据，几何变化增量上传；不因平移、旋转、缩放重新上传整个模型。面、边线和实例尽量批量绘制，辅助 UI 按各自深度规则绘制。透明物体采用统一的排序、深度写入和混合策略。

GPU 拾取结果通过应用通道异步返回，再通知 GPUI 刷新。请求包含文档、场景、相机或请求版本，避免旧结果覆盖当前选择；输入处理中不等待 GPU 完成。CPU 空间索引可作为基础拾取路径。动画、拖动和异步加载沿用 GPUI 现有刷新机制，不必另建 GPU 专用事件循环。

### 三端画面一致性

| 项目 | 共享规则 |
| --- | --- |
| 几何与相机 | 相同网格、单位、坐标系、细分参数和投影；后端适配裁剪空间差异 |
| buffer 布局 | 明确字段大小、偏移、对齐、矩阵排列及乘法顺序，分别核对三个 shader ABI |
| 面与边线 | 相同正反面、深度比较、深度写入、depth bias 和线宽语义 |
| 材质与灯光 | 相同公式、参数、法线变换与纹理输入 |
| 透明与颜色 | 相同排序、alpha 约定、线性计算和输出转换 |
| 选择与辅助图形 | 相同对象 ID、命中规则、遮挡和高亮语义 |
| 采样 | 相同过滤及效果设置；不支持时使用明确的共有降级方案 |

三份 shader 翻译同一套效果规范。视觉一致不等于逐像素相同，GPU 浮点、采样及光栅化差异通过测试容差判断。不能只比较三角形截图就宣称 CAD 效果一致。

## 生命周期与失败处理

`reset` 在设备替换或窗口销毁时释放缓存。普通 resize 由每帧目标描述驱动，仅重建尺寸相关附件。相同大小下目标格式或采样数变化，也必须更新相关 pipeline 和附件。

CPU 帧快照存活与 GPU 在途资源存活是两个问题。每帧 uniform、staging buffer 和读回数据使用独立区域或受完成状态约束的环形存储，不在 GPU 使用期间覆盖或回收。分别遵守各 API 的资源保留与提交规则，不能仅依赖回调返回或 `reset` 完成就认为 GPU 已执行完毕。

回调不得重入同一 painter 或持锁调用可能重新获取该锁的状态查询。绘制出错时，GPUI 恢复自己的状态并继续绘制 UI；已编码的部分命令不会因返回 `Err` 自动撤销。严重设备错误进入平台已有恢复流程。

诊断区分编码成功、窗口呈现调用成功和 GPU 执行完成。现有 `submitted/presented` 计数不能用于证明 GPU 完成或识别已经显示到屏幕的具体命令。设备代次、原始错误和资源统计可用于诊断，产品提示沿用共享稳定错误码及五语 i18n。

## GPUI 补丁修改范围

| 位置 | 修改内容 |
| --- | --- |
| `gpui-pre/src/gpui.rs` 与 GPU 模块 | 公共 `GpuPainter` 接口与类型，拆开 Windows 专属部分 |
| `gpui-pre/src/window.rs` | 统一注册、paint 阶段提交及窗口归属校验 |
| `gpui-pre/src/platform.rs` | 统一生命周期注册，平台转发 |
| `gpui-pre/src/scene.rs` | 有序 GPU 绘制命令、帧数据保留及重放 |
| `gpui-pre-windows` | 沿用状态保存恢复，额外提供当前 RTV、目标描述和 reset 原因 |
| `gpui-pre-apple` 与 `gpui-pre-macos` | Metal pass 切换、当前目标借用、注册转发与生命周期 |
| `gpui-pre-wgpu` 与 `gpui-pre-linux` | wgpu pass 切换、编码器借用、X11/Wayland 注册转发与生命周期 |

为保持源代码兼容性，使用独立 `GpuPaintSurface` 和 `SceneBatch`，保留 `PaintSurface` 的原有字段、视频绘制路径及 `Primitive`/`PrimitiveBatch` 变体。`Scene::render_batches()` 交织 GPU 绘制和原有图元；`Scene::batches()` 保留原有返回类型。GPU 命令参与排序、裁剪、缓存重放和清空。

核心 crate 不增加图形后端依赖，`gpui/Cargo.toml` 保持上游版本。Metal、D3D11、wgpu 上下文及适配器分别放在各自后端 crate，沿用已有的图形库依赖；依赖方向始终为后端依赖核心。Linux 应用与 `gpui_wgpu` 采用一致的 wgpu 版本，不能混用不同版本资源类型。

继续通过 `patches/gpui/manifest.json` 固定源码与 patch 哈希，扩展涉及的源码包、Cargo overrides 和排除项；`scripts/prepare_gpui.py` 重建 `.cache/gpui/`。不修改全局 Cargo registry，也不提交完整 vendor 源码。补丁不包含 CAD 顶点格式、几何算法或业务 shader。

## 性能与验收

直接路径省去应用主颜色纹理和最终采样合成，但仍有自有深度显存、多 pass 切换和状态恢复成本。Metal 的 store/load、目标分辨率、透明过绘、细分规模及 draw 数量都会影响表现；目前没有三端 CAD benchmark，不能据此承诺帧率或一定快于其他方案。

实现按以下顺序验收：

1. 统一接口与 Windows 迁移：保留现有 PCB 行为，验证排序、裁剪、错误恢复和 resize。
2. 三端基础 3D：同一带深度遮挡的场景，验证投影、法线、剔除、颜色和物理像素坐标。
3. CAD viewport：实体与边线、旋转缩放、选择高亮、操作手柄、CPU 或异步 GPU 拾取。
4. 框架集成：菜单覆盖、滚动裁剪、DPI 改变、多窗口、关闭重开、设备恢复和资源回收。
5. 三端效果：固定模型、相机、灯光与设置，比较截图及选择结果；Linux 分别验证 X11 和 Wayland。
6. 大模型性能：记录 CPU 场景准备与编码时间、GPU 各 pass 时间、上传字节、draw 数量、显存和交互帧耗时分布。计时能力不可用时明确记录限制。

测试可使用截图或读回，但正式交互主画面不得依赖每帧 CPU 回读。通过构建不等于通过实窗 GPU 验证，单个平台通过也不等于三端验收。

## 设计依据

- 项目基础：[现有补丁](../patches/gpui/0001-native-gpu-callback.patch)、[固定源码清单](../patches/gpui/manifest.json)、[代码组织](code-organization.md)、[ADR 0001](adr/0001-native-gpu-rendering.md)。
- Metal 的颜色、深度及附件加载保存机制：[Apple render passes](https://developer.apple.com/documentation/metal/render-passes)。
- wgpu 的 pass 与深度附件概念：[RenderPassDescriptor](https://docs.rs/wgpu/latest/wgpu/struct.RenderPassDescriptor.html)。该链接跟随最新版本，实际实现仍以 GPUI 锁定的 29.0.4 本地源码为准。
- 浏览器 canvas 直接目标与页面合成的区别：[WebGPU canvas context](https://www.w3.org/TR/webgpu/#dom-gpucanvascontext-getcurrenttexture)。浏览器结构仅帮助解释概念，不作为 GPUI 性能证据。
