# Windows release 文字准备基线

日期：2026-10-02。使用锁定离线构建的 release `text_prepare.exe`，每个案例串行运行三个独立进程。脚本记录二进制和案例 SHA-256、原始输出、退出码及统计一致性，原始报告为 [USBC](gpu-validation/text-prepare-usbc-release.json) 和 [AGILEX](gpu-validation/text-prepare-agilex-release.json)。

最新代码复测（包含导入内容 SHA-256）：重新构建 release 探针后，USBC 三次为 **0.0728 / 0.0264 / 0.0268 秒**，AGILEX Windows-1252 为 **1.7605 / 1.7502 / 1.7227 秒**。两板对象与实例数仍分别为 482 / 44,841、46,480 / 3,094,215，六次均 exit 0、无跳过且统计一致。新报告保留二进制及输入哈希：[当前 USBC](gpu-validation/text-prepare-usbc-current-release.json)、[当前 AGILEX](gpu-validation/text-prepare-agilex-current-release.json)。下表保留较早基线；未控制缓存和硬件运行状态，不从两批耗时推断哈希步骤的独立开销或性能回退。探针不创建应用视口、不读取查看状态配置，因此这些数字不包括应用状态恢复流程。

| 案例 | 编码 | 三次墙钟耗时（秒） | 文字对象 / 实例 | 跳过对象 |
| --- | --- | --- | --- | ---: |
| USBC_FPC | UTF-8 | 0.0792 / 0.0255 / 0.0253 | 482 / 44,841 | 0 |
| AGILEX_I_SERIES | Windows-1252 | 1.6709 / 1.6575 / 1.6705 | 46,480 / 3,094,215 | 0 |

所有进程退出码为 0，同一案例各次实例统计一致。计时包含进程启动、BRD 导入、字体发现/解码、文字布局、实例准备和进程退出；不是单个算法耗时。没有清空文件系统缓存，不把首轮称为冷缓存，不把后续轮称为受控热缓存。没有测量峰值内存、显存或 GPU 帧率，也没有进行修改前后对比。

复现：

```powershell
python -X utf8 scripts/cargo.py +stable build --release -p pomelo-render --example text_prepare --locked --offline
python -X utf8 scripts/measure-text-prepare.py --binary target/release/examples/text_prepare.exe --board E:/brd_cases/AGILEX_I_SERIES.brd --encoding windows-1252 --output docs/gpu-validation/text-prepare-agilex-release.json
```

脚本要求至少三次（最多二十次）运行，并验证探针统计格式及各轮一致性；非零退出或统计异常保留已取得记录并返回失败。技术报告使用稳定字段，探针的人读警告仍由 rust-i18n 格式化。

这只是文字准备的局部基线，不是研发计划要求的六代表板完整性能报告。CPU/RAM 的 CIM 查询被当前环境拒绝，报告未虚构硬件配置；正式性能验收仍须补齐机器、电源模式、磁盘、DPI、整体导入、完整窗口绘制与内存记录。
# ntpcb_320mb 当前 release 补验

## 六代表板扩展采样

15061 完整笔画计数：新增 count_text_strokes 大小探针，只保留当前对象的规范化字符并累计字体笔画，不保留整板变换几何；与准备入口共用换行、Tab、字符限额和零尺寸处理。`text_prepare <board> windows-1252 --count-strokes` 复测 exit 0：69,797 个文字对象、5,473,611 条笔画、0 缺字对象，见 [计数日志](gpu-validation/text-stroke-count-15061.log)。因此超过当前 400 万限额约 147 万条；尚未提高限额或完成分批实例准备。合成计数与正式准备对照测试和工作区 Clippy 通过。计数不验证每条变换几何、实例精度或 GPU。

15061 预算定位补验：应用和探针均使用 TraceLimits 默认 4,000,000 笔画/实例上限。新增五语累计预算消息，保留 RENDER_TEXT_STROKE_LIMIT 稳定码及失败对象。当前 release 复测在 54,246 个对象、3,999,935 条已准备笔画后失败，剩余仅 65 条；因此是整板累计预算耗尽，现有证据不能断定下一对象异常。原始输出见 [定位日志](gpu-validation/text-prepare-15061-stroke-budget.log)。限额未提高、对象未丢弃，案例仍未通过。25 项文字测试通过（1 外部字体项忽略）、9 项 i18n 门禁及工作区 Clippy 通过。

2026-10-02，同一 release 二进制、Windows-1252、每板三个独立串行进程。原始文件位于 gpu-validation 的 `text-prepare-<板名>-memory-release.json`。

| 板 | 三轮耗时（秒） | 文字 / 实例 | 结果 |
| --- | --- | --- | --- |
| SS8633A FPC | 0.0135 / 0.0105 / 0.0104 | 9 / 373 | 三轮成功；每轮仅一次内存采样，不能作为峰值验收 |
| ML623 | 2.3156 / 0.4769 / 0.4850 | 3,542 / 177,443 | 三轮成功，0 skipped |
| AGILEX | 1.7671 / 1.7418 / 1.7383 | 46,480 / 3,094,215 | 三轮成功，0 skipped |
| S5000C | 5.4974 / 3.1784 / 3.1440 | 36,831 / 2,809,393 | 三轮成功，0 skipped |
| ntpcb | 5.2132 / 5.2434 / 5.1528 | 67,239 / 2,367,209 | 三轮成功，0 skipped；此前同二进制报告 |
| 15061-1b | 11.6463；后两轮未执行 | 未完成 | exit 1，RENDER_TEXT_STROKE_LIMIT，ObjectId 2237209352 |

15061-1b 的失败报告保留于 [原始报告](gpu-validation/text-prepare-15061-1b-memory-release.json)。首次失败后停止重复运行，不能宣称六板文字准备全部通过；后续须检查实际笔画规模及预算，不能通过丢弃对象或仅提高限额掩盖内存问题。缓存状态未控制，首轮较慢不称作受控冷缓存。全部结果仍不含 GPU 或完整 Viewer。

内存采样补验：测量脚本新增可选 `--measure-memory`，仅 Windows CPython；在子进程存活期间读取 GetProcessMemoryInfo 的 PeakWorkingSetSize 和 PeakPagefileUsage，分别记录工作集及私有提交峰值，定义见 [Microsoft 文档](https://learn.microsoft.com/zh-cn/windows/win32/api/psapi/ns-psapi-process_memory_counters_ex)。以 10 ms communicate 超时请求采样，实际间隔受系统调度影响；不声明覆盖最后一次采样后到退出之间的峰值。

同一 release 二进制的三轮结果：5.213225/5.243392/5.152818 秒，exit 均为 0、对象/实例统计一致。采样到的工作集峰值分别 2,864,877,568 / 2,859,065,344 / 2,860,707,840 字节；私有提交峰值 3,224,154,112 / 3,218,759,680 / 3,218,833,408 字节，每轮 327–332 次成功采样。原始报告见 [内存与时间](gpu-validation/text-prepare-ntpcb-memory-release.json)，whole_lifetime_peak_verified 为 false。包括完整导入与文字准备，不包含其他视口批次、GPU 显存或应用窗口资源，因此不能代替完整 Viewer 内存验收。

2026-10-02，当前代码重新构建 `text_prepare` release 后，显式 Windows-1252 导入 ntpcb_320mb。三个独立进程串行测量为 5.234835、5.137784、5.178980 秒，exit 均为 0，统计一致：67,239 个源文字对象全部准备，0 skipped，2,367,209 个实例、99 层。结果及二进制/案例哈希见 [原始报告](gpu-validation/text-prepare-ntpcb-current-release.json)，首次单次运行输出见 [日志](gpu-validation/text-prepare-ntpcb-current-release.log)。

计时包括进程启动、BRD 导入、字体加载、文字几何准备和退出；磁盘缓存未控制。它不测 GPU、峰值内存或真实应用点击到可交互，不能与 Web 历史索引计时直接比较，也不代表整板画面或六代表板验收完成。

