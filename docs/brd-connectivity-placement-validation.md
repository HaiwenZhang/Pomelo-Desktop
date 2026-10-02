# BRD 网络归属与封装/引脚放置验证

日期：2026-10-02。当前只在 Windows 开发与验证。本阶段将源记录、网络连接与已验证的 padstack/焊盘几何组合成封装和引脚查询结果，尚未完成整板 `BoardScene` 或真实 PCB GPU 视口。

## 实现范围

- `decoder::{FixedRecord, VariableRecord, DecodedRecord}::next_key()`：穷举支持的记录类型，仅返回源 `Next`；不存在该字段时返回 `None`，不误用 `NextInFp`、`NextInCompInst` 或其他指针。
- `semantics/connectivity.rs`：完整读取网络名，按源顺序遍历每条 `ConnItem → Next` 链，到零、无 Next 或当前 assignment sentinel 停止。后出现的归属覆盖先前归属；明确分配到网络 0 与未分配保持区分。网络名保留源文字，缺失名字为原始空值。
- 网络构建约束：名称/归属映射默认 512 MiB、800 万项；单链默认 100 万条、64 MiB 访问集；全局最多 6,400 万次链接访问。覆盖已有归属不重复计映射内存，但仍计工作量。记录类型统计的集合开销也计入预算。
- `semantics/placement.rs`：按 `NextInFp` 遍历封装内的引脚，核对 `ParentFp`。转换前按源坐标网格舍入，半整数向远离零的方向舍入；放置位置、局部焊盘偏移、封装/焊盘旋转、背面镜像和物理图层映射分别处理。
- 引脚先读取直接 `NetPtr` assignment，包含网络 0；该引用没有可用 assignment 时使用连接链归属，再回落到网络 0。单层嵌入、区域与裸片顶层语义沿用验证过的 resolver；背面裸片明确诊断并跳过，保留 Web 已知限制。
- `pomelo-core::ComponentPlacement` 保留封装稳定 ID、源元件引用、名称、变换和引脚 ID；`Pin::owner_id` 关联封装，名称重复也不会合并对象。自定义焊盘轮廓仍通过不可变 Arc 共享。
- 放置输出默认限制 2 GiB、800 万项，定义/焊盘缓存和链遍历各有独立预算。检查贯穿缓存、源记录、每层焊盘和单条大链。失败或取消不发布部分封装/网络结果；重试重新创建构建器。
- CLI 新增 `decode-connectivity FILE --report JSON` 与 `decode-placement FILE --records JSON --report JSONL`。报告携带源 SHA-256、版本、编码及 `scene_validated: false`；放置请求校验源身份和每条索引边界。网络失败报告不输出部分归属，BRD 扩展名不能作为报告目标。

网络缺失、类型错误、环、取消和预算沿用共享五语结构化诊断。新增 `BRD_PIN_DEFINITION_UNSUPPORTED` 与 `BRD_DIE_BACK_UNSUPPORTED` 的英文、简中、繁中、日文、韩文消息，保留引脚 ID、padstack 引用与源偏移；CLI 和错误切换语言不会改变这些字段。

## 合成与工程验证

工作区 **159 项测试通过，0 失败**；格式检查、Clippy `--workspace --all-targets -D warnings` 和 Windows debug 工作区构建通过。测试日志：`.cache/connectivity-placement-workspace-tests.log`。

新增 24 项测试覆盖：网络链 sentinel/零终止、源名字、重复归属、网络 0、错误位置与五语显示、名称/归属/链/全局工作预算、链内取消、Next 字段隔离；正背面旋转、局部偏移、源网格舍入、网络引用优先级、嵌入/区域/裸片、错误父引用、引脚链环/缺失、放置预算、大封装链内取消/完整重试和 CLI 源文件保护。真实文件不包含重复归属，覆盖该行为的证据是合成测试。

## 156 案例完整网络差分

`.cache/connectivity-parity.jsonl.summary.json`：156 个案例全部匹配，完整核对 **324,038 个网络名、4,608,358 个对象归属、423,410 条 assignment 和 4,933,673 个叶字段**。这些不是网络对象抽样。

| 归属对象类型 | 完整匹配数量 |
| --- | ---: |
| `0x05` Track | 1,312,007 |
| `0x0e` FootprintRectangle | 892 |
| `0x28` Shape | 176,601 |
| `0x2e` Connection | 20,008 |
| `0x32` PlacedPad | 1,217,992 |
| `0x33` Via | 1,880,858 |

两端分别从原始字节建立数据库。Web oracle 对照冻结 `scene-builder.ts` 的网络循环，完整遍历 Web 记录，而不消费 Native 网络结果来生成预期值。源文件哈希、冻结 Web 文件哈希、编码、索引记录/字符串数量、完整键集合、网络 ID、名称、源类型数量与遍历统计精确比较。

## 156 案例放置抽样差分

`.cache/placement-parity-final.jsonl.summary.json`：156 个案例全部匹配，每文件最多 4 个封装，按该类型源记录的首尾等距位置取样。每个选中封装的全部引脚链和焊盘都参与比较：**620 个封装、22,268 个引脚、23,556 个焊盘、753,324 个叶字段**。

其中 1,443 个背面引脚、29 个裸片引脚、926 个自定义焊盘、6 个区域引脚均来自真实案例。单层嵌入/特殊引用基础定义的更广覆盖见 [padstack 验证](brd-padstack-validation.md)。背面裸片的不支持诊断在合成测试验证，不宣称真实背面裸片已支持。

预期引脚直接来自 **完整、未改动的 Web `AllegroSceneBuilder.build()`**，没有用复制的放置公式或铜皮/文字 stub 替代。Web 完整场景本轮累计包含 1,156,560 个引脚；Native 只核对其中上述选中封装的引脚，不能把这个 Web 总量当作 Native 全引脚通过。

校验源哈希、编码、版本、单位换算、物理图层、索引签名/边界/Key/类型、确定性抽样位置、元件归属、引脚顺序与完整对象字段。ID、枚举、网络、源引用和诊断参数精确比较；几何采用 `1e-12 × max(1, |Web|, |Rust|)`，不以改变容差解释差异。浮点证据不等于像素或 GPU 性能验收。

## 负向对照与最终构建复核

仅修改 `.cache/` 报告副本，原始 BRD、冻结 Web 和正确报告不变。以下五种改坏均以退出码 1 拒绝，并定位到被改坏字段：

| 改坏内容 | 实际拒绝位置 |
| --- | --- |
| 对象网络 ID +1 | `network.owners.<object>` 值不一致 |
| 删除一个网络名 | `network.nets` 键集合不一致 |
| 引脚 X +0.01 mm | 指定封装的 `pins.0.at.0` 不一致 |
| 焊盘图层 +1 | 指定封装的 `pins.0.pads.0.layer` 不一致 |
| 引脚封装 owner ID +1 | 指定封装的 `pins.0.owner` 不一致 |

负向报告位于 `.cache/connectivity-negative/` 和 `.cache/placement-negative/`。最终工作区构建后重新生成 Native 报告并做规范化逐字段复核；具体结果记录在 `.cache/connectivity-placement-final-recheck.json`。JSON 对象成员顺序不作为契约，数组顺序与数值仍要求一致。

## 复现

```powershell
python -X utf8 scripts/cargo.py +stable test --workspace --locked --offline
python -X utf8 scripts/cargo.py +stable build --workspace --locked --offline
python -X utf8 scripts/probe-connectivity.py .cache/indexes-windows1252.jsonl .cache/connectivity-probes target/debug/pcb_inspect.exe
python -X utf8 scripts/probe-records.py .cache/indexes-windows1252.jsonl .cache/placement-probes target/debug/pcb_inspect.exe --placement --samples 4
node --max-old-space-size=24576 --import file:///C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/tsx/dist/loader.mjs scripts/check-connectivity-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo .cache/connectivity-probes/manifest.jsonl .cache/web-inputs.json .cache/cases-manifest.jsonl .cache/indexes-windows1252.jsonl .cache/connectivity-parity.jsonl
node --max-old-space-size=24576 --import file:///C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/tsx/dist/loader.mjs scripts/check-placement-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo .cache/placement-probes/manifest.jsonl .cache/web-inputs.json .cache/cases-manifest.jsonl .cache/indexes-windows1252.jsonl .cache/placement-parity-final.jsonl
```

两端显式使用 Windows-1252，与既有索引对照一致；这不证明每块板实际编码是 Windows-1252。工具链、冻结清单和案例准备见此前验证文档。Node 沙箱中的已确认限制需要外部只读运行；输出限于本项目 `.cache/`。CLI 报告和 oracle 都只用于研发，正式程序不依赖 Web/Node。

## 未完成范围

仍需实现过孔放置、bond finger/bond wire 关系、走线归属、铜皮网格、板框/文字/绘图及完整场景构建；之后接入常驻 D3D11/HLSL 批次、拾取和 UI。当前 importer 保持 `scene_supported: false` / `SceneNotImplemented`，不因为两个语义模块通过便把文件标记为 Ready。macOS/Linux、release、真实板 GPU 画面与性能均不属于本轮通过范围。
