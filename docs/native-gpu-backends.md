# 原生 GPU 后端与验证

## 代码布局

- `crates/pomelo-render/src/backend/d3d11/`：Windows 资源、管线与 GPUI 适配器，入口为 `mod.rs`。
- `crates/pomelo-render/src/backend/metal/`：macOS Metal 资源、管线与 GPUI 适配器。
- `crates/pomelo-render/src/backend/wgpu/`：Linux wgpu 资源、管线与 GPUI 适配器。
- `backend/common/`：D3D11、Metal、wgpu 共用的批次、增量上传、绘制参数与 stencil 操作。
- `backend/board.rs`、`copper_renderer.rs`、`pad_renderer.rs`：三个平台共用的图层顺序、场景缓存、显示选项与高亮策略。
- `backend/d3d11/shaders.rs`、`metal/shaders.rs`、`wgpu/shaders.rs`：各后端的着色器加载、编译或验证入口。
- `src/shaders/d3d11/`、`metal/`、`wgpu/`：各平台独立维护 HLSL、MSL、WGSL。运行时不执行着色器跨语言转换。

应用通过 `backend::native` 使用统一接口，由编译目标自动选择 Windows D3D11、macOS Metal 或 Linux wgpu。平台模块为内部实现，公共渲染类型由 `backend` 导出，`native` 集中提供兼容出口。三个平台目录都以 `mod.rs` 放置 GPUI 适配器，`driver.rs` 放置资源与命令实现，`shaders.rs` 放置着色器入口。

构建和测试均只启用目标平台的后端及着色器。wgpu 驱动、WGSL 验证与 GPU 像素测试仅在 Linux 编译；macOS 只测试 Metal，Windows 只测试 D3D11。各平台的像素与着色器测试放在 `tests/unit/backend/{d3d11,metal,wgpu}/`。

所有后端借用 GPUI 的设备及当前帧目标，不另建应用渲染设备，不自行提交或呈现 GPUI 帧，不在生产画布中进行 CPU 像素回读。wgpu 上传使用当前编码器复制 staging buffer，无需 GPUI 提供 queue。

Metal 每次画布回调只创建一个渲染编码器，所有批次共用颜色与 stencil 附件，返回时通过 RAII 结束编码。最初逐批次创建编码器，在大板显示多个图层后出现控件状态更新但屏幕停留旧帧的问题；合并编码器后已验证画面恢复刷新。

## 2026-10-04 本机验证

本机为 macOS ARM64。测试文件为用户提供的 `S5000C-64_DDR5_BGA_V0.61.brd`，以 `windows-1252` 源文件编码打开，解析诊断为 0。

- 实际 Metal 画布已显示整板，上传 321,476 个走线／轮廓实例、484,205 个解析焊盘实例，以及 1,017 个铜皮区域的 13,383,557 个顶点与 39,326,247 个索引。
- 运行诊断 `ready=true`、`last_error=null`；所有 1,017 个铜皮区域可见。
- 实际操作验证网络颜色切换、清除选择、适应整板与折叠侧栏，界面及画布随操作刷新。
- Metal 实机像素测试覆盖裁剪、继承透明度、背景保留、铜皮孔洞及单通道 4,096 批次提交。
- macOS 应用构建与全目标检查通过；渲染 crate 的 Clippy（`-D warnings`）通过。
- Windows GNU 和 Linux musl 的渲染 crate 全 feature、全目标 Clippy（`-D warnings`）交叉检查通过；Windows 的 `--all-features` 不再启用 wgpu。

GPU 测试需要实际设备，默认忽略；本机运行：

```sh
cargo test -p pomelo-render --features native-gpu --lib backend:: -- --include-ignored
```

测试使用独立的合成几何，不依赖 `tests/fixtures` 或用户的板文件。测试中的读回仅用于像素断言。

Linux 原生桌面窗口及 Vulkan 驱动尚未在本机验证；Windows 的 FXC 与像素测试也需要 Windows 实机。CI 已加入 Linux 应用检查、渲染单元测试与 Clippy。跨平台视觉一致性、持续交互性能及长期稳定性仍需各平台实机验收。

## 高倍缩放持续刷新的修复

高倍缩放会产生裁剪后为空的铜皮曲线覆盖。Metal/wgpu 原实现仍向该覆盖索取顶点缓冲区，报 `GPU_COPPER_VERTEX_MISSING`。错误信息作为普通布局子节点出现，将画布高度从 741 改为 670；新的视图使标签与曲线缓存失效，待曲线缓存暂时缺失时错误消失，高度恢复，形成重绘循环。

现在三个后端都跳过已完成上传的空曲线覆盖，保留其对原始铜皮网格的替代语义；错误详情使用浮层，不影响画布尺寸。Metal GPU 回归验证空覆盖既不会绘制原始网格，也不会访问缺失缓冲区。

本机 S5000C 在约 3054 逻辑像素/mm 下停止操作后，连续采样的绘制计数保持 2218、画布高度保持 741、标签与曲线版本不变，`ready=true`、`last_error=null`，确认后台重绘循环停止。

Zone 轮廓也加入上传完成判断和诊断字段，本板全部 340,064 个轮廓实例已上传。Zone 边框在所有 shapes alpha 值及填充模式下保持显示；Dynamic Zone 使用一物理像素细线，不再按 alpha 阈值隐藏。Metal 像素回归验证边框始终保留。
