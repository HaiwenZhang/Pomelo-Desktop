# 原生 GPU 后端

`pomelo-render` 维护共享场景准备与各平台渲染实现：

| 平台 | 后端 | 着色器 |
| --- | --- | --- |
| Windows | D3D11 | WGSL 经 naga 转换为 HLSL |
| macOS | Metal | WGSL 经 naga 转换为 MSL |
| Linux | wgpu | 直接使用 WGSL |

应用通过 GPUI 的 GPU 绘制入口接入当前设备和帧目标；业务几何、批次、缓存和着色器由渲染 crate 管理。构建按目标平台启用对应后端。

`NativeGpuRenderer::Snapshot` 与 `GpuPainterHandle<Snapshot>` 共享快照类型，`paint_gpu` 接收对应的 `Arc<Snapshot>`。当前三个后端通过独立 pass 的 painter 注册入口接入，以便管理 PCB 绘制所需的 stencil 附件。Metal 和 wgpu 的共享 pass 接口需另外实现对应的 shared painter。

像素测试使用合成几何，并需要相应平台的实际 GPU。平台实现存在不代表跨平台视觉、性能和长期稳定性已完成验收。通用检查入口见[开发与验证](development-progress.md)。

## 代码布局

平台实现位于 `src/backend/d3d11/`、`metal/`、`wgpu/`。各目录的 `mod.rs` 负责适配入口，`driver.rs` 管理资源与命令。公共批次及绘制策略位于 `backend/common/` 与共享 renderer 模块。

只维护 `src/shaders/*.wgsl` 一套源码。`backend/wgsl.rs` 统一拼装调色板、材质和透明度，校验 WGSL 并在 Windows/macOS 创建管线时通过 naga 转换。Windows 保留 `d3d11/shaders.rs` 作为 FXC 编译边界，入口名称取自 naga 反射结果；Metal 使用生成的 MSL。平台接口通过 `backend::native` 提供给应用。

统一绑定为 group 0：帧常量 binding 0、几何 binding 1、图集 binding 2、采样器 binding 3。D3D11 对应 b0、原始字节缓冲视图 t0、图集 t2 和采样器 s0；Metal 对应 buffer(0)、buffer(1)、texture(0)、sampler(0)，buffer(2) 提供几何运行时数组的字节长度。CPU 常量结构和实例 stride 由测试与 WGSL 布局核对。

## 构建与安装包

WGSL 通过 `include_str!` 嵌入程序，naga 是 Rust 库依赖；不需要额外的着色器文件、naga 命令行工具或外部转换步骤。现有 Windows、macOS 和 Linux 安装包脚本继续使用 Cargo 构建，无需改变签名、安装器或资源复制流程。Windows 仍使用原有 FXC，macOS 仍使用系统 Metal 编译器；升级时提交 Cargo 配置和锁文件即可。

所有宿主平台的测试均生成 HLSL 和 MSL，Windows 测试还实际使用 FXC 编译全部顶点、片元及铜皮清除入口。Metal 编译及 GPU 像素测试仍需在对应平台运行。

## 绘制与资源

每帧使用当前目标的 bounds、clip、DPI 和透明度，完整视口用于相机计算，有效裁剪用于 scissor。不能将当前帧目标保存到后续帧使用。

Metal 的单次画布回调共用渲染编码器及附件，返回前结束编码。wgpu 的上传与绘制接入当前编码器。设备替换或窗口销毁时释放并重建关联资源。

空裁剪和已上传的空曲线覆盖应跳过对应绘制，不能继续访问不存在的顶点缓冲区。错误展示不应改变画布布局，从而引起相机或缓存的重复失效。

GPUI 的准备与绘制回调错误保留在 `status().last_error` 中，成功绘制不会自动清除；需要消费错误时使用 `take_error()`。`status().encoded` 仅表示回调成功编码，不代表 GPU 执行完成。

## 平台检查

验证相同合成场景的裁剪、孔洞、层序、文字、透明度和高亮，同时检查窗口缩放、DPI、连续刷新及资源恢复。像素断言和实际窗口交互分别记录，CPU 准备通过不能代替 GPU 绘制通过。
