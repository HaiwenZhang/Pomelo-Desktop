# BRD 文字、尺寸绘图与场景组装验证

> 本文保留历史验证记录，其中引用的辅助脚本已移除；当前 `scripts/` 仅保留 Windows 打包工具。

日期：2026-10-02。当前开发平台为 Windows。

结论：已实现完整板文字与存储尺寸图形组装，`SceneBuilder` 汇合物理图层、网络、走线、过孔、封装/引脚、铜皮、板框、文字和尺寸绘图。`AllegroImporter::import()` 已从文件生成共享 `Arc<BoardScene>`，不再返回未实现错误。**桌面打开流程仍停留在文件头展示；真实 BRD GPU 视口、完整字段回归和性能验收仍待完成。**

## 实现边界

- `semantics/text.rs`：遍历 header 和已放置封装的文字链，保留首次出现顺序及最后有效 owner。读取第一组字体定义；支持零尺寸字体、对齐、字体编号、字符/行间距、笔画宽度、旋转和镜像。源坐标已是板级坐标，不叠加封装变换；不显示未放置的库文字及 DRC 文字。
- `semantics/drawing.rs`：仅处理源文件存储的 `0xf901` 尺寸图形。通过 header/封装所属链确认成员，以板级 graphic 或封装 owner 分组；不重建箭头、数值或二次变换。闭合路径允许回到第一条边，其他循环为致命错误。绘图通过 `text_ids` 引用场景文字，避免重复字符串。
- 绘图层保存源名字、类别和子层身份。保留源自定义名字；预定义子层使用五语标准标签。默认可见性、颜色和排序沿用 Web 语义，名称在展示时翻译。
- `semantics/scene.rs`：生成完整不可变场景及组件身份；保留特殊图层、钻孔和铜皮外环边界。板边界与冻结 Web 保持一致：文字/尺寸、graphic 类型板框不扩展板边界，shape 类型板框计入端点。
- 场景输出默认上限为 4 GiB、16,000,000 个计数对象。走线/放置、铜皮在每次查询前接收剩余输出预算；文字/尺寸构建使用剩余额度。输出预算涵盖对象、容器增长及诊断；源文件、索引、网络归属、定义缓存及几何临时分配仍有独立限额。**这不是单一进程 RSS 上限，也不是实测内存结果。**
- 取消、资源限制或致命引用错误不会发布部分场景；空几何返回 `BRD_NO_GEOMETRY`。新增字体、文字链、尺寸链和空几何诊断从第一条实现开始采用稳定 code、MessageKey、参数及五语资源。驱动和源文字不翻译。

本轮新增 41 个消息键、205 条五语译文。引入组件/特殊图层等场景类型未修改 GPUI 补丁或 PCB shader；Windows GPU 仍为此前的小场景实验。

## 两类真实对照证据

两端独立读取冻结 BRD 原件，使用相同的显式 `windows-1252` 编码及源码/案例哈希清单。该设置不证明原始编码就是 Windows-1252；严格 UTF-8 与源文字复核限制仍按历史记录保留。

### 完整文字与尺寸字段

`check-annotation-parity.mts` 调用冻结 Web 的完整 TextBuilder/ DrawingBuilder，不抽样。比较所有文字属性、绘图层名称/颜色/可见性、绘图分组、所属对象、graphic/text ID、尺寸路径和诊断。整数、身份和字符串精确；几何浮点相对容差为 `1e-12 × max(1, |Web|, |Rust|)`。本组真实案例没有产生文字/尺寸 warning；异常诊断的五语及位置由合成测试覆盖。

| 案例 | 布局 | 文字 | 绘图层 | 尺寸组 | 尺寸路径段 | 分组文字 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 15061-1b | 165 | 69,797 | 54 | 82 | 2,823 | 279 |
| AGILEX_I_SERIES | 174 | 46,480 | 90 | 38,774 | 40,083 | 17 |
| ML623_BRD_revD_rdf0074 | 157 | 3,542 | 47 | 5 | 23 | 4 |
| S5000C-64_DDR5_BGA_V0.61 | 174 | 36,831 | 59 | 15 | 98 | 2 |
| SS8633A_AMPB_FPC_DOE_V2_HVT_A_0423_1716 | 172 | 9 | 4 | 0 | 0 | 0 |
| ntpcb_320mb | 174 | 67,239 | 65 | 133 | 240 | 45 |
| 合计 | — | **223,898** | **319** | **39,009** | **43,267** | **347** |

全部六板匹配，4,395,359 个叶字段通过。记录：`.cache/annotation-six-parity.jsonl` 及 `.summary.json`。最终 Windows 工作区构建再次生成六板报告，JSON 值逐字段一致（对象成员顺序不影响比较，数组/数值不放宽），记录于 `.cache/annotation-final-six-probes/verified.json`。

负向对照仅修改缓存副本：文字 X 加 0.01 mm 被定位为 `texts.0.at.0`；graphic ID 加 1 被定位为 `drawings.0.graphicIds.0`。两轮均以退出码 1 返回，并且只有被改坏的案例失败，其他五板匹配。记录：`.cache/annotation-negative/`。

### 完整 SceneBuilder 的统计、边界与对象身份

六板使用 Native SceneBuilder 完整生成场景，JSONL 按对象写出。对照程序流式读取，核验 header/source 身份、所有类别的数量及每个对象的源顺序 ID、板边界和完整结束标记；预期来自完整冻结 Web SceneBuilder。

合计：104 个物理图层、33,998 个网络、31,211 个组件、1,147,379 段走线/圆弧/hatch、144,925 个过孔、163,589 个引脚、25,044 块铜皮、551 段板框，以及前表的全部文字和尺寸。六板均匹配，均无解析 warning。本组六板没有键合特殊图层，不能扩大为特殊图层的整板验收。

记录：`.cache/scene-six-journals/manifest.jsonl`、`.cache/scene-six-summary-parity.jsonl` 及 `.summary.json`。**本项只证明完整组装统计、边界、顺序和 JSONL 覆盖，不证明每个走线/焊盘/铜皮字段都已完成全量差分，更不证明 GPU 像素正确。** 既有分层字段抽样证据分别见 [走线与过孔](brd-routing-validation.md)、[铜皮](brd-copper-validation.md)、[网络与放置](brd-connectivity-placement-validation.md)。

## 构建与复现

205 项工作区测试通过、0 失败；包含本轮新增的 15 项文字/尺寸、场景预算/取消、真实文件 importer 和 JSONL CLI 测试。全部特性/目标 Clippy `-D warnings`、格式检查与 Windows debug 工作区构建通过。日志：`.cache/annotation-scene-workspace-tests.log`、`.cache/annotation-scene-clippy.log`、`.cache/annotation-scene-build.log`。

```powershell
cargo +stable test --workspace --locked --offline
cargo +stable clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
cargo +stable build --workspace --locked --offline

target\debug\pcb_inspect.exe --locale zh-CN --encoding windows-1252 decode-annotations E:\brd_cases\ntpcb_320mb.brd --report .cache\annotations.json
target\debug\pcb_inspect.exe --locale zh-CN --encoding windows-1252 decode-scene E:\brd_cases\ntpcb_320mb.brd --report .cache\scene.jsonl
```

`decode-scene` 的首行为 metadata（含源哈希、编码、记录数和各类场景计数）；随后为 `{kind,index,data}` 对象行，最后为 bounds 和 complete。场景构建失败不写出部分场景对象；写出中断时缺少 complete，校验会拒绝报告。完整原始对象导出可能达到数 GiB，不把 JSONL 总字节数当作 CPU 常驻内存；正式应用直接消费 `Arc<BoardScene>`，不绕行诊断 JSONL。

```powershell
node --max-old-space-size=24576 --import file:///C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/tsx/dist/loader.mjs scripts/check-annotation-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo .cache/annotation-six-probes/manifest.jsonl .cache/web-inputs.json .cache/cases-manifest.jsonl .cache/copper-six-indexes.jsonl .cache/annotation-six-parity.jsonl
node --max-old-space-size=24576 --import file:///C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/tsx/dist/loader.mjs scripts/check-scene-summary-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo .cache/scene-six-journals/manifest.jsonl .cache/web-inputs.json .cache/cases-manifest.jsonl .cache/copper-six-indexes.jsonl .cache/copper-earcut-baseline.json .cache/scene-six-summary-parity.jsonl
```

Native CLI 不运行 Web/Node；这两条 Node 命令仅用于开发对照，并核验冻结源码与案例。所有报告保留 `scene_validated: false`，避免把分层/统计通过扩大为最终产品验收。

## 仍需完成

将 importer 接入桌面后台任务和状态机，并把真实 `BoardScene` 送入 `pomelo-render` 的 Windows D3D11/HLSL 批次；完成拾取、图层与面板、全部字段/截图回归、GPU 资源恢复、release 性能与打包。铜皮单环 earcut 内部仍无取消回调，其延迟门槛尚未通过。当前不新增 macOS/Linux 实现。
