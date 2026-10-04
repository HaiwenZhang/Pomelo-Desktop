# 搜索与实际拾取锚点：应用接入

> 本文保留历史验证记录，其中引用的辅助脚本及 pomelo-core/import/render 的 example 探针已移除；旧探针命令不再可执行。当前验证使用各 crate 的测试代码。

日期：2026-10-04。接续[共享核心验证](search-anchor-validation.md)。当前 Windows 的应用检查器、画布命中元数据和状态恢复已接入；完整搜索/拾取验收仍有未覆盖组合。

## 实现

共享索引新增带实际对象、层、显示类别和距离的 `CanvasHit`。应用保留实际鼠标命中，再展开为对象/走线/网络/元件选择；全组身份、全部几何 bounds 与命中锚点分别保存。搜索根据同次显示快照选择可见最高条目，全隐藏时回退；显示变化会取消或重建检查准备，过期异步结果不发布。

检查器保留完整组名称和统计，同时显示实际锚点的源字段、源 ID 和五语命中层。模式转换保留属于目标的实际命中，清除同时清空选择和锚点。`ViewState` 保存可选锚点，旧配置缺少该字段仍可加载；孤立锚点或不属于所选目标的恢复值被拒绝或重新计算。CPU 契约无 GPUI/GPU 依赖；D3D11/HLSL 边界及 UI 字体不变。

本次补齐 bond wire 的独立类别、die pin 的 etch 类别，以及 ZoneOutline 命中的保留。没有把组中第一个成员当作实际鼠标命中。

## 最新 Web 差分

`check-canvas-picking-parity.mts` 使用正式 `pomelo-render` 的 MSDF 探针，比较首选 hit、四种选择模式及归一化后的实际对象/层/类别锚点。钻孔层 `-1` 对应 `LayerId::UNASSIGNED`，serde 的 etch 类别不改写为另一名称。

| 范围 | 查询 | 匹配 | 差异 |
| --- | ---: | ---: | ---: |
| USBC_FPC 图元/文字 | 33,552 | 33,552 | 0 |
| camera_test_board 图元/文字 | 35,112 | 35,112 | 0 |
| camera_test_board 全部 29 个 die pad | 3,480 | 3,480 | 0 |
| AGILEX 全部 552 个背钻，8 种显示状态 | 52,992 | 52,992 | 0 |
| 合计 | 125,136 | 125,136 | 0 |

这些是明确位置及显示状态的查询对照，不是六代表板整板图像或 156 案例所有组合验收。报告在 `.cache/canvas-parity/anchor-app-{fpc,camera,die,backdrills}/report.json`，包含案例和 Web 源码指纹；本轮 Web picking.ts SHA-256 为 `1db1628ccbf57df24b0b04bdd212da097dea5a3e313a64dc095ec04db9df4e77`。

初次锚点对照的适配器把 etch 改名且按 JSON 字段顺序误判，修正后复验通过；旧 adapter-difference 报告保留，不算产品错误或通过证据。另发现两个 crate 的同名 example 争用输出，已将 importer 源几何探针改名为 `source_canvas_pick_probe`；两次用错输出的背钻运行终止，不计入通过。最终背钻任务使用独立复制的正式 MSDF 探针，SHA-256 为 `8959bd6a5e04cc2b580fefe6e9505f2f10bbbaa5d48368283a17e4eb3f23ca4b`，退出码 0，52,992 项全部匹配。

## 应用窗口与回归

FPC 实窗搜索 J1 得到 `ComponentGroup(Pin(15255))`、全部 30 个成员，检查器实际锚点为 TOP 上 Pin 15220，展示 MTB5、GND 和源 ID `0x3B74`。正常退出重开保留该锚点、相机和组身份。

点击另一焊盘得到相同 J1 组，但实际锚点为 Pin 15225，检查器展示引脚 9、源 ID `0x3B79`。转换对象模式得到 Object(Pin(15225))，转换网络模式得到 Net(657)，两者继续保留 Pin 15225/TOP；清除后目标和锚点均为 null。另实际点击 Zone 31254 和 L2-F Segment 25132，检查器层与遥测一致。多标签切回 FPC 保留其选择，camera 文档不继承 FPC 的锚点。

新增核心及应用回归覆盖 preferred hit 校验、其他组/无效层回退、ZoneOutline、die/bond 分类、锚点持久化/旧配置/孤立锚点、完整组字段与实际源字段组合、钻孔翻译及取消。本轮含面板功能的工作区 473 项测试、Clippy、格式、debug/Release 构建均通过；GPU 显式硬件测试没有在本轮重跑。

窗口证据位于 `.cache/ui-validation/anchor-app-*.json` 及 `panel-collapse-final-j1.json`、`panel-collapse-final-restart-both.json`、`panel-collapse-final-fpc-tab-switch.json`；冻结清单为 `.cache/canvas-parity/anchor-app-provenance.json`。多语排序、finger-only、多文档锚点跨启动恢复、所有隐藏/显示切换的实际窗口流程及完整性能仍待补验。

2026-10-03 记录的 Release 文件被用户实例锁定情况是历史边界。本轮用户实例已退出，最终 `target/release/pomelo.exe` 构建成功；验证使用私有副本，不锁住 Cargo 输出。
