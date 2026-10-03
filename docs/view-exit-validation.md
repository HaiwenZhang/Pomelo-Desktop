# Windows 退出与查看状态恢复验证

日期：2026-10-03。范围：Windows debug 原生应用、简体中文浅色、1440×920 默认窗口。使用 `E:\brd_cases\LPDDR4_PGB.brd` 和 `USBC_FPC.brd`，未修改源文件。

## 问题与修复

先前成员列表验证中，退出后重开仍恢复较早的 GND 2035 和 177% 相机。此次复现确认：旧实现正常退出后，`views.json` 仍包含旧网络选择，未保存界面最后的空选择和新相机。

本项目固定的 GPUI 在 `App::shutdown` 中调用退出观察者、释放窗口，再以 200 ms 超时阻塞等待返回的 future。生产 `PlatformScheduler::block` 只轮询该 future，不运行通过平台消息队列分发的前台任务。旧 `save_views` 把启动后台写盘的逻辑放在 `cx.spawn` 中；退出回调等待这个前台任务，主线程却无法继续执行它。因此问题并非退出订阅未 `detach`，也不是选择序列化错误。

`workbench/mod.rs` 现在在前台同步捕获小型查看快照，直接创建后台串行保存任务。后续写入等待上一次后台写入完成，然后沿用 `ViewStore::save_updates` 合并、校验及原子替换。通过 `futures::future::Shared` 同时保留后台任务、正常运行时的错误观察以及退出等待，避免 UI 实体释放取消写入。新增直接依赖复用锁文件中已有的 futures 0.3.34，没有升级依赖版本。

关闭标签、重新加载仍在窗口打开时通过前台观察结果，把结构化错误交给现有 UI 诊断。退出路径只等待后台结果；失败使用当前语言格式化已有 i18n 消息写入 stderr，不依赖已释放的工作区实体。没有更改 GPUI 补丁、业务 Shader、查看状态 schema 或源身份匹配规则。

## 实窗证据

每次退出后先确认 Pomelo 窗口列表为空，再重新启动本轮 debug 二进制；查看状态落盘读取与随后真实窗口恢复分别核对。

| 流程 | 退出前状态 | 落盘及重开结果 |
| --- | --- | --- |
| LPDDR4，Ctrl+Q | 清除 GND；177% 放大至 213%；翻板 | `selection=null`，`pixels_per_mm=21.274173859379612`，`flipped=true`；实窗恢复空检查器、213% 与翻板 |
| LPDDR4，标题栏关闭 | 正面；从 GND 成员定位走线 12152；3130% | 中心 `(24.786082, -1.130808)`、`pixels_per_mm=313.0399914550781`、选择 `object.segment=12152`；实窗恢复 3130%、走线属性及 1/1 成员 |
| LPDDR4 与 USBC 同窗，Ctrl+Q | LPDDR4：F2 后 187%、走线 12152、网络着色、全部拾取；USBC：103%、空选择、图层着色、禁用铜皮拾取 | 两个条目均写入；分别为 18.65811090592867 与 10.31504318024382 pixels/mm；过滤位分别 15 与 7；没有串板覆盖 |
| 多文档退出后分别重开 | 先打开 USBC，关闭标签回欢迎页，再打开 LPDDR4 | USBC 实窗为 103%、图层着色、铜皮拾取未选中；LPDDR4 实窗为 187%、网络着色、走线 12152；恢复选择没有重新把相机定位到 3130% |

首次启动的窗口工具曾报 `foreground window did not report a process id`。重选后仍失败，按工具指引重建 JavaScript 会话后恢复截图和输入；此后的坐标操作使用最新截图 ID。未把失败的操作计入验收。

## 工程检查

本轮最终代码执行：

```powershell
python -X utf8 scripts/cargo.py +stable build -p pomelo --locked --offline
python -X utf8 scripts/cargo.py +stable test -p pomelo --locked --offline
python -X utf8 scripts/cargo.py +stable test -p pomelo-core --test i18n_resources --test i18n_source --locked --offline
python -X utf8 scripts/cargo.py +stable clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
python -X utf8 scripts/cargo.py +stable fmt --all -- --check
git -c core.safecrlf=false diff --check
```

Windows debug 构建成功；应用 51 项、i18n 7+2 项通过，0 失败；工作区全部目标/特性 Clippy、格式及差异检查通过。存储的非法状态、连续合并及失败保留字节沿用现有回归；实际退出时序以上表实窗结果为证，不以存储单元测试代替窗口验证。

## 尚未覆盖

本轮没有注入慢磁盘、权限失败或达到 200 ms 退出超时的保存，没有验证任务管理器强制结束、系统关机、多个进程同时写配置、关闭标签后不间断立即退出、首次布局前退出及大量连续关窗。超时仍由固定 GPUI 管理，此改动不保证超时或强制结束后的最终写入。退出错误写到 stderr 不表示用户能在关闭后的界面看见诊断。

本记录不替代五语、DPI、多显示器、release、完整 BRD 回归和 UI 逐像素验收；语言、外观和最近记录使用各自保存流程，未据此声明它们的快速退出时序已验收。
