# 搜索定位锚点：共享核心验证

2026-10-04 更新：应用检查器、实际画布命中元数据及锚点保存/恢复已接通，125,136 次最新 Web 拾取与实际锚点对照零差异，FPC 实窗完成搜索、实际拾取、模式转换、清除和重开恢复。详见[应用接入验证](search-anchor-app-validation.md)。下文保留 2026-10-03 核心阶段的范围与当时未完成项。

日期：2026-10-03。本记录覆盖核心定位锚点及无窗口差分；**尚未接入应用检查器和画布命中元数据，不能视为搜索交互全部完成。** 左侧 TOP 列表顺序和 UI 字体不变。

## 实现与契约

`pomelo-core/interaction/picking_index/navigation.rs` 提供 `SegmentIndex::selection_anchor`，返回有类型的对象、图层和显示类别。它与持久化的网络/元件组身份分离：选择全组不意味着使用第一个成员作为命中对象。根据同一次请求的 `BoardDisplay` 快照，优先选择可见条目中 rank 最高的条目，rank 相同则按 Web 提交顺序选择最后一项；所有条目隐藏时回退至全部条目中最高的一项，无索引几何时返回 `None`。

比较键保留线/圆弧顺序、文字源顺序、孔 scope 的首次出现及组内顺序、背钻组、base pad、解析焊盘、自定义焊盘顺序。孔分组包含未选中的源对象，不能仅从当前网络重新分组。die pad 采用 etch 类别，bond wire 保留单独显示类别。钻孔和背钻分别检查开关及 via scope；pin 钻孔独立于铜层显隐。核心仅使用源模型、显示契约和已准备的文字 quad，不依赖 GPUI、GPU 或浏览器。

查找在工作线程执行，检查取消并返回已有 `PathError`，不发布部分结果。当前仍扫描源场景，scope 字典按每次请求建立；未新增永久的逐 pad 索引。`selection_bounds` 保持独立契约，包含隐藏条目的几何；可见锚点不能改变定位范围。

## 实际 Web 差分

`scripts/check-selection-anchor-parity.mts` 调用最新本地 Web 的 `BoardSearchIndex`、`BoardIndex.locateSteps` 与 `BoardDisplay`，native probe 使用正式 importer 和共享 MSDF 字形准备。Web 工作区只读。比较对象类型/ID、图层与类别，钻孔层 `-1` 映射到核心 `LayerId::UNASSIGNED`。

| 输入 | 默认状态搜索条目 | 10 种显示状态合计 | 差异 |
| --- | ---: | ---: | ---: |
| USBC_FPC | 29 | 290 | 0 |
| camera_test_board | 49 | 490 | 0 |
| AGILEX_I_SERIES | 7,642 | 8,218 | 0 |
| 自有合成场景 | 16 | 160 | 0 |
| 合计 | 7,736 | 9,158 | 0 |

状态包括默认、关闭钻孔、全部隐藏、激活 BOTTOM、隐藏 via、隐藏 pin、手动提升 BOTTOM、关闭背钻、显示文字、隐藏 etch。真实板默认状态覆盖全部搜索条目；其他状态最多均匀抽样 64 条，FPC/camera 条目不足 64，仍全量覆盖。没有将抽样结果扩展为全部显示组合的保证。

合成场景含无几何成员、未知 pad、跨层及自定义 pad、独立 pin 钻孔、交错出现的 via 孔 scope、背钻/base pad、die pad 和 bond wire。fixture 从已有定位边界场景扩展，包含刻意越出外环的减法轮廓；它用于独立算法检查，不代表合法 PCB、BRD 导入或所有实际特殊板已验收。

第一次合成差分发现 2 项差异，原因是适配器给 Web pin 同时保留了原生 `pads` 和转换后的 `shapes`。Web 按是否存在 `pads` 区分 via 钻孔 scope，导致适配器把独立 pin 钻孔当作 via。已移除原生 pin 字段，再用实际 Web 算法复验为零差异；保留旧报告 `anchor-synthetic-adapter-difference.json`。没有为通过差分改变独立 pin 钻孔语义。

17 项核心回归覆盖隐藏回退、手动提升、独立钻孔、完整 600 成员、自定义 pad 提交顺序、孔组顺序、背钻开关、实际索引 glyph 可见性、die/bond 类别及取消。当前工作区全目标全特性 456 项测试通过、17 默认忽略，Clippy、格式、diff 检查和 Windows debug 构建通过。GPU 显式测试未在本轮运行，没有修改 HLSL 或呈现逻辑。

报告位于 `.cache/canvas-parity/anchor-{fpc,camera,agilex,synthetic}/`，每份包含输入 SHA-256、Web 源码指纹、请求及两端结果。运行使用 Node 24.18.1，native probe 为优化的 debug 构建；批量用时包含导入、字体准备和查找，不能作为 release 单次定位或 UI 延迟的性能验收。复现示例：

```powershell
python scripts/cargo.py build -p pomelo-render --example selection_bounds_probe --offline --locked
node --import file:///C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/tsx/dist/loader.mjs scripts/check-selection-anchor-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo target/debug/examples/selection_bounds_probe.exe E:/brd_cases/USBC_FPC.brd .cache/canvas-parity/anchor-fpc utf-8
```

## 尚需接入和验收

应用需在搜索定位时捕获 display 快照，保存 runtime anchor；检查器展示源对象及命中层，同时保留全组统计、完整高亮与持久化身份。画布拾取需保留实际 hit 的层/类别，不能用整个选择的最高条目替代鼠标命中点。模式切换、异步过期结果、清除、重新定位和重启恢复需要一致处理，并同步五语新增文案与真实窗口验证。

本次没有更改应用 UI，也未宣告 macOS/Linux、156 案例整板、GPU 故障、长期内存或发布验收完成。冻结清单位于 `.cache/canvas-parity/anchor-provenance.json`。

Release 更新尝试在最终覆盖 `target/release/pomelo.exe` 时失败，系统报告拒绝访问；只读检查确认用户正在运行该路径的程序（PID 50444）。保留该实例，不终止用户程序，也不把此次命令列为 Release 构建通过。当前运行的 Release 仍是此前 TOP 列表修正版本；新核心尚未接入 UI，本轮不需要替换用户运行实例。
