# Windows 无控制台启动验证

日期：2026-10-03。用户反馈直接打开 `pomelo.exe` 会带出 PowerShell/终端窗口。

## 修正

`crates/pomelo/src/main.rs` 原先没有声明 Windows 子系统，默认链接为控制台程序。入口新增 `#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]`，对 Windows debug 和 Release 均生效。遵循 [Rust 官方子系统说明](https://doc.rust-lang.org/reference/runtime.html#the-windows_subsystem-attribute)，GUI 程序不创建或附着控制台。日常使用直接双击可执行文件，不需要启动脚本。

独立验证启动脚本移除 `-NoNewWindow` 和旧控制台注释，继续通过私有配置与经 SHA-256 核验的二进制副本启动。界面内的本地化导入诊断仍保留；GUI 进程的启动阶段 `eprintln!` 不再显示在交互控制台，本轮未新增日志落盘。

## 检查

Windows debug 构建通过（9.48 秒），Release 构建通过（1 分 03 秒），工作区全目标全特性 Clippy、格式、脚本语法及差分检查通过。直接读取两个产物的 PE Optional Header，`Subsystem` 均为 **2 / Windows GUI**，不是 3 / Windows CUI。没有增加只镜像入口属性的单元测试。

| 产物 | 字节数 | 生成时间 | SHA-256 |
| --- | --- | --- | --- |
| `target/debug/pomelo.exe` | 47,724,032 | 2026-10-03 11:35:19 | `8B8ED5BC2C4027D539A906567962549DD10716812BA42139F3A314F1F5781AC1` |
| `target/release/pomelo.exe` | 35,432,960 | 2026-10-03 11:34:47 | `E06BB660FC0E624705CF201C86A6D5364BEC47B77333F946C2D858D6D13AEF5F` |

Release 私有副本在实际桌面启动后，窗口清单只新增 Pomelo 主窗口，没有新增终端/PowerShell 窗口。实际激活并观察简中深色 1440×920 欢迎页：切片品牌、文件入口、真实最近预览、三组介绍和快捷键均正常渲染。不是仅以进程启动成功替代窗口观察。

首次沙箱启动进程具有窗口句柄，但不在实际桌面的工具清单中；该实例不计作实窗成功证据。上述实窗结果来自随后在实际桌面启动的独立副本。五语深浅最小窗口的排版证据见 [欢迎页验证](welcome-layout-validation.md)，对应修改 GUI 入口前的同版界面，不扩大为本轮重新执行了完整矩阵。

实际桌面实例通过 Ctrl+Q 正常退出，窗口清单确认已移除。首次无人交互、未打开文档的沙箱验证进程在核对精确 ID 与私有可执行文件路径后单独清理。日常 `language.json/theme.json/recent.json/views.json` 四项 SHA-256 再次与基线全部相同。

## 后续产物复核

之后启动错误反馈已接入五语原生对话框和有界本地日志，debug/Release 仍保持 GUI 子系统。下表保留此前产物，不代表最新版本；当前产物及实窗范围见 [启动错误验证](startup-error-validation.md)。

本节保留欢迎页拖入反馈阶段的产物。之后缩略图更新的 debug/Release 仍为 Windows GUI 子系统，当前产物及实际窗口证据见 [缩略图验证](recent-thumbnail-validation.md)。

欢迎页拖入反馈修改后重新构建：debug 通过（11.45 秒），Release 通过（59.04 秒）；两个当前产物的 PE 子系统仍均为 2 / Windows GUI。格式、全工作区全目标全特性 Clippy 和差分检查再次通过，日常四份偏好哈希仍与基线相同。上表保留首次启动修正时的产物证据；当前产物如下。

| 产物 | 字节数 | 生成时间 | SHA-256 |
| --- | --- | --- | --- |
| `target/debug/pomelo.exe` | 47,726,592 | 2026-10-03 11:50:35 | `3AC0BB8A45B917354BDF5096CD7855EB1249A501AC30C633925C82426B61E07E` |
| `target/release/pomelo.exe` | 35,433,984 | 2026-10-03 11:43:21 | `111F1520D28059F0A5A79BB4EF7081F162B2795E30AA2330FA1207DCC864BCBF` |
