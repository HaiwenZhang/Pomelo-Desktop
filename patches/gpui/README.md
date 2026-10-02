# GPUI 通用原生 GPU 补丁

基线为 `gpui-pre` / `gpui-pre-windows` **0.3.7**（GPUI Kit 0.7.0），对应 Zed `1a28cff4b409169bac058bca40dfbfeb7621d19b`。归档与补丁 SHA-256 由 `manifest.json` 固定。上游源码及许可证从官方 crate 归档保留到生成目录，不把完整源码放进 `vendor/`。

## 构建

需要 Git、Python 3.12+ 和仓库指定 Rust 工具链。从仓库根目录执行：

```powershell
python scripts/cargo.py +stable run -p pomelo --locked -- --native-gpu-demo
python scripts/cargo.py +stable test --workspace --locked
```

本机 stable 与锁定的 1.98.1 相同；其他机器使用已安装的指定工具链。包装器每次先校验源码、补丁及生成缓存，再调用 Cargo。输入未改变时只校验，不重复解压；版本或补丁改变后从原始归档重建，绝不在已打补丁的源码上叠加同一补丁。

也可以先运行 `python scripts/prepare_gpui.py`，再使用普通 Cargo / rust-analyzer。干净检出必须先准备依赖，`build.rs` 执行时依赖解析已经发生，不能承担此工作。CI 有独立准备步骤。

源码生成到 `.cache/gpui/`，Cargo `[patch.crates-io]` 只引用该目录。优先读取 Cargo 已缓存的、哈希匹配的 `.crate` 归档，不修改全局 registry；缺少时从 `static.crates.io` 下载并核验。`--offline` 禁止下载。补丁冲突或哈希错误会停止构建，不能静默回退到未修改的 GPUI。请勿同时重建缓存和运行其他 Cargo；准备锁只防止两个准备进程相互覆盖，不覆盖整个 Cargo 生命周期。

不要手改生成缓存：下次校验会重建。升级时在单独临时副本修改，导出相对于新基线的补丁，审查差异，再更新归档/补丁哈希和 Cargo.lock。构建 wrapper 不会替你选择新上游版本。Windows debug 的 GPUI 自带 shader 仍依赖此缓存路径；发布需单独验证 release 打包。

## 框架边界

唯一补丁 `0001-native-gpu-callback.patch` 提供通用窗口级入口，目前实现 Windows D3D11：

```rust,ignore
// 应用创建视图时注册；renderer 由 pomelo-render 实现。
let handle = window.register_gpu_renderer(renderer)?;
// Element/canvas 的 paint 阶段提交，不透明数据随场景存活。
window.paint_gpu(bounds, &handle, std::sync::Arc::new(frame_snapshot))?;
```

- `NativeGpuRenderer::draw` 接收借用的 GPUI device/context、物理像素 viewport/bounds/clip 和 `Any + Send + Sync` 数据。`reset` 在设备更换、窗口销毁前释放应用 GPU 资源。
- 命令复用 `PaintSurface` 的场景顺序、裁剪信息及重放。注册句柄限定所属窗口，禁止跨窗口提交。弱注册表不会永久保留已关闭视图；在途/缓存 scene 的强引用保证回调和帧数据存活。
- D3D11.1 `SwapDeviceContextState` 隔离并恢复整套管线状态。回调结束清空自己的绑定，释放窗口 RTV 引用，避免阻塞 `ResizeBuffers`。GPUI 负责绑定目标、viewport 和最终 present。
- 回调同步发生在 GPUI 渲染线程。不得自行 present、创建竞争交换链、跨线程使用/保留 context，或在回调中重入 GPUI/同一注册句柄。应用必须遵守 clip；所有自有设备资源须在 `reset` 中清空。
- `NativeGpuHandle::status` 给出提交、成功 present 和错误信息。回调失败后恢复状态，继续绘制 GPUI 界面；应用用本地化消息解释原始技术错误。

补丁不含 PCB、三角形、HLSL、顶点格式或业务管线。`crates/pomelo-render/src/native_gpu/` 独立持有走线/圆盘/Zone、shader、常量布局、D3D11 管线与资源缓存。新增 PCB 图元只改该 crate；增加平台后端仍需扩展通用平台上下文和框架适配，不能据此宣称 macOS/Linux 已支持。

`pomelo-render` 的 Windows FFI 模块局部允许 `unsafe`，其余模块保持 `deny(unsafe_code)`；其他 workspace crate 仍为 `forbid`。每处 COM 调用/内存复制写明安全边界，Rust/HLSL 常量大小和偏移有编译期断言。

维护 patch 减少整份 vendor 源码的入库负担，仍需维护上游接口兼容性。升级必须重新跑构建、测试、裁剪/覆盖层/resize、设备恢复及真实 GPU 验证。实验结果见 [验证记录](../../docs/gpu-native-validation.md)。
