# 构建依赖

项目使用 Rust、GPUI 和 GPUI Kit。依赖及 feature 配置以根目录的 `Cargo.toml` 为准，精确解析结果以 `Cargo.lock` 为准；构建时使用 `--locked`。

GPUI 从 workspace 声明的 Git 来源获取。`support/gpui-compat` 通过 Cargo 的包名适配机制，让组件库共享同一套 GPUI 类型。更新依赖时同步锁文件并验证目标平台的构建与渲染。

```sh
cargo run -p pomelo --locked
cargo test --workspace --locked
```

首次构建需要获取依赖。缓存齐全后可使用 `--offline`。正常桌面构建和运行不需要 Node.js 或其他项目工作区。

平台原生构建条件与打包命令见[项目介绍](../README_zh-CN.md)，渲染职责见[原生 GPU 后端](native-gpu-backends.md)。

## 更新与兼容

Git 来源统一由 workspace 管理；GPUI Kit 和包名适配层须共享兼容的 GPUI 类型，避免混入不同来源的窗口、实体或宏类型。更新依赖应同时检查 Cargo 配置、锁文件及平台 feature，不能仅修改介绍中的版本号。

依赖更新后检查应用构建、组件布局、GPU 裁剪、透明度、覆盖层及资源恢复。后端扩展接口来自项目使用的 fork，具体 API 以锁定源码为准，不能推定其他版本或上游具有相同接口。

## 平台与打包

Windows 需要 MSVC 构建工具和 Windows SDK，macOS 需要 Xcode Command Line Tools，Linux 需要窗口、字体、音频及相关开发库和图形驱动。完整平台条件以 README 和构建脚本为准。

打包入口位于 `scripts/`；安装包、签名和发布工作流不由本文修改。更新字体或其他随包资源时，需一并维护相应许可证及来源清单。
