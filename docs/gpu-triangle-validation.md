# wgpu + WGSL 三角形验证

验证日期：2026-10-01。结论：**最小 GPU 绘制验证通过，已在 GPUI 原生窗口的画布中实际显示三角形。** 完整 PCB 渲染器和 GPU 共享纹理合成尚未完成。

## 实测环境与结果

Windows x64，Rust 1.98.1，`wgpu 30.0.1`、`gpui-kit 0.7.0` / `gpui-pre 0.3.7`；硬件为 NVIDIA GeForce RTX 5080。请求固定后端，不允许软件适配器通过验收。

| 实验 | 结果 | 证据 |
| --- | --- | --- |
| DX12，768×512 | 通过；102,986 个三角形像素 | [JSON](gpu-validation/triangle-dx12.json)、[GPU 输出 PNG](gpu-validation/triangle-dx12.png) |
| DX12，257×129 | 通过；8,622 个三角形像素，验证非对齐宽度的行去填充 | [JSON](gpu-validation/triangle-dx12-257x129.json) |
| Vulkan，768×512 | 通过；102,986 个三角形像素 | [JSON](gpu-validation/triangle-vulkan.json) |
| GPUI 原生画布 | 实窗观察红色顶点、绿色左下顶点、蓝色右下顶点及连续插值；重新绘制恢复正确图像；最大化及深/浅主题正确合成 | `pomelo --gpu-demo`，computer-use 实窗检查 |

原型使用 WGSL 顶点/片元入口、`RenderPipeline`、`RenderPass::draw(0..3, 0..1)` 和硬件队列提交。无 CPU 三角形光栅化，也不加载预制图片代替 GPU 绘制。回读后检查透明背景、三角形覆盖比例、三个内部采样点的主色及方向；空图或错误长度不能通过。

## 呈现路径与边界

```text
WGSL → wgpu / DX12 → 离屏 RGBA8 纹理
     → staging buffer / CPU 回读 → RGBA 转 BGRA
     → GPUI RenderImage → 原生窗口画布
```

此实验证明锁定的库、WGSL 编译、真实 GPU 绘制、数据回读和 GPUI 显示能够连通。用户界面使用五语消息契约，GPU 错误保留稳定错误码，原始驱动原因单独进入技术详情。

CPU 回读仅作为原型/诊断路径。当前画布按比例显示固定 768×512 输出，窗口尺寸变化不会自动重建同尺寸 GPU 目标；“重新绘制”重新执行设备初始化和绘制。正式视口仍需常驻设备/管线、按物理尺寸更新目标、共享纹理与同步、菜单/弹窗叠放、DPI/拾取、设备丢失和真实大板性能验证。JSON 的耗时包括 CPU 等待和回读，是单次 debug 记录，不能解释为 GPU 时间或 PCB 帧率。

## 复现

在仓库根目录执行；本机 `+stable` 为 1.98.1。正常运行不依赖浏览器或 Node。

```powershell
# GPUI 画布，Windows 固定 DX12
cargo +stable run -p pomelo --locked -- --gpu-demo

# 输出 PNG 和同名 JSON；每次输出前必须通过像素检查
cargo +stable run -p pomelo-render --example triangle_probe --locked -- dx12 .cache/gpu-triangle.png
cargo +stable run -p pomelo-render --example triangle_probe --locked -- dx12 .cache/gpu-triangle-257x129.png 257 129
cargo +stable run -p pomelo-render --example triangle_probe --locked -- vulkan .cache/gpu-triangle-vulkan.png
```

画布按钮可重新绘制；`Ctrl+R` 重新绘制，`Ctrl+T` 切换主题，`Ctrl+Q` 退出。macOS 使用相应 Command 快捷键；macOS/Metal 与 Linux 尚未实测，不宣称通过。

实现位置：`crates/pomelo-render/src/triangle.rs`、`src/shaders/triangle.wgsl`、`examples/triangle_probe.rs`、`crates/pomelo/src/gpu_demo.rs`。原型模式与 BRD 文件打开流程独立，不改变导入能力声明。
