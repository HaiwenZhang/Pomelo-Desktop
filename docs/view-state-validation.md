# 查看状态实现与验证

日期：2026-10-02，当前仅 Windows。

2026-10-03 补验并修复：正常退出时创建的前台保存任务不能在 GPUI 阻塞退出等待期间执行，导致旧相机与选择未被替换。查看状态写入已改为直接启动后台串行任务，共享结果用于运行时错误显示和退出等待。Ctrl+Q、标题栏关闭、两个真实文档独立相机/选择/着色/拾取过滤恢复已有实窗及落盘证据，详见 [Windows 退出与恢复验证](view-exit-validation.md)。此记录更新下文早期退出实现描述及相应待验收项；超时、强制结束等仍待验证。

铜皮不透明度已由 BoardDisplay 保存/恢复，旧文件缺失字段默认 0.35，状态校验限制有限值且在 0–1；图层侧栏提供百分比和每次 5% 的降低/提高按钮，端点禁用，五语文案齐全。GPU 帧使用文档值。6 项状态测试、9 项 i18n 门禁、Clippy 及 Windows debug 构建通过；实际调节、GPU 画面与关闭再恢复流程仍需实窗验收。

最新应用层回归：`test -p pomelo --locked --offline` 38 项通过、0 失败、0 ignored；新增真实配置文件中的非法过滤位读取，验证五语错误及原字节不变，并在连续文档合并测试中验证两个文档各自的过滤状态保持独立。随后 Windows debug `build -p pomelo --locked --offline` 和工作区全目标全特性 Clippy 均 exit 0，包含新增过滤字段的接线。这些结果更新下文较早的构建状态；真实窗口及跨启动验收仍未完成。

拾取类别过滤现已纳入快照和恢复，使用受限四类别位集；缺失字段的旧 schema 1 文件默认允许全部类别，未知位拒绝解析并由配置读取层给出既有五语诊断。全部 16 种过滤组合往返、旧文件默认值和非法位回归包含在 `test -p pomelo-core view_state --locked --offline` 的 5 项通过结果中；工作区 Clippy 通过。此新增接线尚未重建应用及完成实窗跨启动验收。

已接入的流程：后台导入对实际解析字节计算 SHA-256，并结合格式和解码选项生成身份；读取标准应用目录 `views.json`，仅路径和身份匹配时恢复相机、图层显隐、选择模式与选择目标。选择经后台源对象校验和检查器准备，恢复不重新定位相机。关闭标签前捕获快照，正常退出捕获其余文档，串行任务合并并原子保存，退出回调等待最终任务。

存储上限为 20 条、4 MiB，非法配置生成五语诊断；恢复保存前备份损坏原文。内容/编码变化拒绝旧状态，非法更新不覆盖原文件，缺失源路径可以保留。尚在后台的有效恢复选择进入快照，被取消的旧代目标不会重新出现。

恢复相机还须能表示为 GPU 使用的 f32：中心与缩放不能转为无穷，缩放不能归零。状态校验与导航恢复共用此边界；无效恢复保持旧相机。新增极大中心、极大/极小缩放回归后，`test -p pomelo-core --lib --locked --offline` 35 项通过，工作区全目标全特性 Clippy 通过；该修复尚未重新构建应用二进制。

后续补验：包含相机数值边界的 Windows debug 应用执行 `build -p pomelo --locked --offline` 成功，exit 0。新增 `view_save_failure_preserves_existing_directory_and_reports_five_languages`，使用真实临时目录让 views.json 路径被目录占用；保存失败后原目录与标记文件字节保持不变，无遗留备份/临时文件，诊断可在五语格式化。`test -p pomelo view_save_failure --locked --offline` 1 项通过。这只覆盖存储失败路径，不证明实际退出时用户能看见错误或退出等待超时行为。

本轮实际验证：

```powershell
python -X utf8 scripts/cargo.py +stable test --workspace --all-targets --all-features --locked --offline
python -X utf8 scripts/cargo.py +stable build -p pomelo --locked --offline
```

工作区 335 项通过、0 失败、7 ignored，32 个测试目标成功结束。原始输出见 [view-state-workspace-tests.log](gpu-validation/view-state-workspace-tests.log)。ignored 是既有外部案例/资源及硬件项，本轮没有把它们计作通过。Windows debug 构建 exit 0；此前查看状态最终接线的工作区 Clippy 已通过。

针对性证据涵盖：状态序列化往返、内容/编码变化、相机首布局保持、类型化选择存在检查、取消优先、配置原子替换/损坏备份、连续文档更新合并、待恢复目标快照与旧代隔离。这些测试未创建真实 GPUI 关闭/退出窗口，不证明任务在实际关窗时序下最终落盘。

仍待验收：真实板调整相机/图层/选择后关闭并重新打开、退出进程后重启、源文件或编码变化、快速关闭/退出、保存失败及退出等待超时。正式发布还需重新构建 release（当前已有 release 二进制早于此功能），并进行干净环境验证。

首次布局前关闭的保存边界已补齐：未初始化导航不生成快照，不以默认相机覆盖磁盘状态；已恢复或手动设置的有效相机可以在首次布局前生成快照。针对性命令 `python -X utf8 scripts/cargo.py +stable test -p pomelo-core interaction::tests --locked --offline` 实际结束，8 项通过、0 失败；工作区全目标全特性 Clippy 通过。修复后执行 `python -X utf8 scripts/cargo.py +stable build -p pomelo --locked --offline`，Windows debug 构建 exit 0。这些测试和构建不替代真实窗口关闭时序验收。
