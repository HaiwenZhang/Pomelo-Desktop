# 元件 reference 分组验证

> 本文保留历史验证记录，其中引用的辅助脚本及 pomelo-core/import/render 的 example 探针已移除；旧探针命令不再可执行。当前验证使用各 crate 的测试代码。

后续补验：正式定位现已使用索引几何 bounds，确认最新 Web 包含隐藏条目，显示状态只影响搜索锚点；J1 的 typed 分组、30 成员及相机完成真实落盘和重开恢复。此前本轮未验证项保留作为历史边界，最新证据和其余未验范围见[定位边界验证](selection-navigation-bounds-validation.md)。

记录日期：2026-10-03。Windows 本轮完成搜索、画布元件选择、成员查询、悬停和 D3D11 高亮的 reference 分组对齐。**桌面与 Web 的 UI 界面字体保持不变**；Web 项目只作只读参考。本记录不代表完整 PCB Viewer 或整板视觉验收通过。

## 实现契约

最新 Web 的 `BoardSearchIndex` 和 `BoardIndex.select(..., "component")` 按非空原始 reference 聚合：先遍历 pin，再遍历 via 的 finger。比较区分大小写，不 trim；空 reference 回退到对象选择，空白字符串仍是有效源值。普通 via、bond wire 和仅有 placement 而没有成员的元件不加入分组。

`pomelo-core/interaction/component_reference.rs` 保存这项平台无关语义。新增 `ComponentAnchor::Pin/Finger` 及 `SelectionTarget::ComponentGroup`：使用第一个 pin，或无 pin 时第一个 finger，作为带类型的源锚点。pin 和 via 可以具有相同数字 ID，不发生身份冲突。具有相同 reference 的多个源 placement 保留独立 ID、位置和归属；交互分组不合并源模型，也不为孤立 finger 伪造 placement。

搜索结果、画布四模式中的 component 模式、后台选择准备、检查器成员及 GPU 高亮使用同一分组。成员按源顺序为 pin→finger；finger 无需 source-pin 链接。成员卡片每页最多 256 项，计数、选择 pin 集合和悬停对象集合使用完整成员，避免大元件高亮被分页截断。检查器显示锚点的实际源 ID，不把聚合组展示为具有唯一位置/角度的 placement。

旧的 `Component(placement_id)` 在应用后台选择准备时按其 reference 转换为规范组。源 placement 查询仍保留原 API。已测试规范转换和序列化往返；本轮尚未验证新组选择的真实配置落盘、退出和重开闭环。

同时修正网络列表：保留源名称中的空字符串和空白，仅在名称映射缺失时使用网络 ID。空白网络行的提示与可访问命令名称使用已有五语 `SourceNetName` 消息，原始数据不被翻译或改写。未更改字号、字重或字体策略。

## 自动化结果

使用原生 importer 和真实 Web parser/SceneBuilder 生成场景，比较实际 `BoardSearchIndex` 条目及 `BoardIndex` 的 component 分组和选择结果。原生探针读取正式搜索/选择 API，并核对全部分页成员、完整 pin 集合、带类型悬停集合和汇总计数。仅规范化 JSON 对象属性顺序，数组顺序与值精确比较。

| 案例 | 编码 | 分组 | 成员 | 源 placement | finger | 差异 |
|---|---|---:|---:|---:|---:|---:|
| USBC_FPC.brd | utf-8 | 11 | 94 | 15 | 0 | 0 |
| camera_test_board.brd | utf-8 | 17 | 118 | 30 | 28 | 0 |
| AGILEX_I_SERIES.brd | windows-1252 | 3,814 | 16,966 | 3,837 | 0 | 0 |
| component-reference-groups.json | synthetic | 4 | 6 | 5 | 2 | 0 |

三块真实板合计 3,842 个分组、17,178 个成员。报告位于 `.cache/canvas-parity/components-{fpc,camera,agilex,synthetic}/report.json`，包含案例和 Web 源码 SHA-256。真实案例均未覆盖重复 placement reference 和仅有 finger 的组；自有 fixture 专门覆盖这两项、同数字 pin/via ID、大小写、空/空白值及多语 reference。合成 fixture 经显式模式转换为 Web pin/finger，未提供 pad 几何，因此它只证明分组语义，不证明 GPU 几何或 BRD 解析。

`component_groups.rs` 六项回归覆盖上述契约，以及 601 成员的三页查询、完整高亮集合、源锚点失效、取消、组边界和旧 placement 转换。工作区全目标全特性测试 **432 通过、0 失败、17 默认忽略**；忽略的测试不计为本轮硬件验证。Clippy `-D warnings`、格式及 Windows debug/Release 构建通过。

参考 Web 源码指纹：

| 文件 | SHA-256 |
|---|---|
| src/lib/board/search.ts | `880778293ad3c75f43ce42daaf52e1d584cb35bb57b67b21e4a3e1e78dedfa81` |
| src/lib/interaction/picking.ts | `1db1628ccbf57df24b0b04bdd212da097dea5a3e313a64dc095ec04db9df4e77` |
| src/lib/allegro/scene-builder.ts | `63532efd52fba960732859da0105f016878a4f3bbe68a32a8b1738c9040d84b5` |

复验示例，从桌面项目根目录执行：

```powershell
cargo +stable build -p pomelo-import --example component_group_probe --locked --offline
& C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/.bin/tsx.cmd scripts/check-component-group-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo target/debug/examples/component_group_probe.exe E:/brd_cases/camera_test_board.brd .cache/canvas-parity/components-recheck utf-8
& C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/.bin/tsx.cmd scripts/check-component-group-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo target/debug/examples/component_group_probe.exe tests/fixtures/component-reference-groups.json .cache/canvas-parity/components-synthetic-recheck synthetic
```

## 原生窗口结果与版本边界

私有简中深色 Release 副本打开真实 camera_test_board，使用 GPUI 同设备 D3D11、NVIDIA GeForce RTX 5080，资源 ready、呈现正常且无 GPU 错误。搜索 U2 后，检查器和画布得到 **29 个 pin + 28 个 finger，共 57 个成员**，目标为 `ComponentGroup(Pin(ObjectId(18041)))`。切换 component 拾取并点击 finger `Via(27072)`，得到同一组，观察到 pin/finger 选择轮廓；清除选择后悬停该 finger，观察到整组悬停轮廓。

冻结窗口遥测为 `component-window-search-u2.json`、`component-window-finger-u2.json`、`component-window-hover-u2.json`，均位于 `.cache/canvas-parity/`。交互期间静态几何保持 805 实例、103,040 上传字节，管线及缓存构建次数保持 1。本轮未更改 HLSL 或 GPU ABI，未新增硬件像素测试；实窗结果也不替代整板 Web/原生逐像素对照。

实窗副本 SHA-256 为 `b0576a792ff7fd35967eee81b86668b79410949a8bb2617f0f0338e3acaa6cb9`，启动参数和私有配置目录见 `component-launch.json`。之后加入空白网络名称的可访问标签，并重新通过完整测试和构建；最终 Release 为 `0a259037c1aab29ed6d839232fdbd7876b59d1e1e1db171b483956d6bf3d3b2e`，debug 为 `3c4a8b25c08f1909999b419f839803782d5cb0af5eca669611ea78cc2ae19fbf`。最终标签改动尚未单独实窗复验。检测到用户手动操作预览后停止自动输入，保留该私有窗口；本轮未继续执行配置恢复测试。

UI 字体策略文件 `crates/pomelo/src/theme.rs` 的 SHA-256 保持 `2cb472958b85ea4a32316961c2902b535903697719b9743231743bf1e5c0ed70`。各源文件、日志、报告和二进制指纹冻结在 `.cache/canvas-parity/component-provenance.json`；这些缓存是本机证据，复验脚本及自有 fixture 保留于项目中。

## 后续门槛

- 定位当前按组成员的源几何取 bounds，不包含虚构 placement 位置；尚未对齐 Web 按当前显示状态生成的可见条目 bounds。
- 分组和条目构建已对齐；多语查询匹配、`localeCompare` 排序与全部排名仍需单独验证。
- 新组选择的真实退出/重开恢复、全部案例组合、鼠标滚轮映射及多 DPI 仍需完成。
- 整板视觉、完整交互、设备恢复、性能与发布验收继续按研发计划执行。macOS/Linux 本阶段不实施。
