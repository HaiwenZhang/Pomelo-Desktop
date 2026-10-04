# GPUI fork 依赖

当前依赖 [HaiwenZhang/zed 的 gpui-pre-0.3.7-native-gpu 分支](https://github.com/HaiwenZhang/zed/tree/gpui-pre-0.3.7-native-gpu)，基于 Zed `1a28cff`，包含 Metal、WGPU、D3D11 GPU Painter 接口和 Apple 依赖修复。根 Cargo.toml 统一声明 Git 分支；Cargo.lock 当前锁定提交 `8c92bda2dc9d718cd520d37fda20bca00fa3e274`，使用 `--locked` 构建可复现。

## 构建

```sh
cargo run -p pomelo --locked
cargo test --workspace --locked
```

首次构建需要访问 GitHub 和 Cargo registry。缓存依赖后可使用 `--offline`。普通 Cargo 和 rust-analyzer 可直接使用。构建不依赖本地 `references/zed`，不再准备或应用 GPUI / GPUI Kit 源码补丁。旧 `.cache/gpui` 和 `.cache/gpui-kit` 不参与构建。

## GPUI Kit 包名兼容

GPUI Kit 0.7.0 引用 `gpui-pre` 系列 0.3.7，fork 使用原生 Zed 包名。`support/gpui-compat` 的四个 workspace crate 通过 `[patch.crates-io]` 代替 `gpui-pre`、`gpui-pre-platform`、`gpui-pre-macros`、`gpui-pre-sum-tree`。这些小 crate 仅转发 feature 并重导出根 workspace 声明的 GPUI 类型与宏，所有依赖使用同一个 Git 来源。

项目仅构建 Windows、macOS 和 Linux 原生应用，已移除仅供 Web 目标使用的 `gpui-pre-web` 适配 crate。GPUI Kit 自身的 Web 条件依赖保留在 lockfile 中，不参与桌面构建。

此处的 `[patch.crates-io]` 是 Cargo 的包名适配机制，不是修改 GPUI 源码。GPUI Kit 和 gpui-base 使用 registry 原版包。`1a28cff` 的 `LineWrapper::wrap_line` 与 GPUI Kit 0.7.0 的调用兼容，因此已移除换行补丁、准备脚本和 `.cache/gpui-kit/gpui-base` 覆盖。

## 原生 GPU 后端

应用通过 `register_gpu_painter` / `paint_gpu` 使用 fork API。Windows 使用 D3D11，macOS 使用 Metal，Linux 使用 WGPU；业务几何、shader、缓存与测试由 `pomelo-render` 持有。平台依赖和 feature 保留在各 crate 的 target 配置中，版本统一由根 workspace 管理。

macOS 默认启用 `gpui-apple/runtime_shaders`，着色器在运行时编译，不要求构建机安装独立 Metal Toolchain。诊断中的 `GpuPainterStatus.encoded` 仅表示编码成功，不证明 GPU 完成或 present 成功；设备替换和窗口销毁时清空应用资源。

## 更新 fork

所有 Git 来源只在根 Cargo.toml 中声明。分支推送新提交后，主动运行 `cargo update -p gpui` 更新 Cargo.lock；其他使用相同来源的 GPUI 包随之更新。修改分支时也只需调整根 workspace 声明，然后重新生成 lockfile。更新后验证桌面构建及对应平台的检查、裁剪、覆盖层、resize 和资源恢复。若 GPUI Kit 将来支持原生 Zed 包名，可移除包名适配层。
