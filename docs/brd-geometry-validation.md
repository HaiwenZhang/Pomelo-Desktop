# BRD 图层、路径与铜皮轮廓验证

日期：2026-10-02。当前开发平台为 Windows。本轮实现源单位、物理图层、路径、圆弧及铜皮轮廓语义，并在真实案例中与冻结的 Web 算法对照。**这不是完整 BoardScene、铜皮三角化或桌面整板查看验收。**

## 已有实现

| 模块 | 行为 |
| --- | --- |
| `pomelo-import/src/allegro/semantics/units.rs` | 五种源单位按 divisor 转为 mm；未知单位与零 divisor 明确诊断 |
| `semantics/layers.rs` | 解析 header 的物理层表；保留源名称、Properties flags、八色循环；层功能由 `0xc100` mask 判定 |
| `semantics/geometry.rs` | 按源引用遍历四种边记录；处理首边闭合、owner/sentinel、孔洞链、环和缺失引用；保留解析后的圆弧 |
| `pomelo-core/src/geometry.rs` | 有符号圆弧 sweep、完整圆、铜皮路径离散化；保留文件端点，去除相邻重复点和闭合重复点 |
| `database.rs` | 增加精确索引边界的 offset 查询，允许零 Key 记录；内部偏移不能伪装成记录起点 |
| `pcb_inspect decode-geometry` | SHA-256 绑定请求；输出图层/比例元数据、普通/hatch 边、track/graphic 路径、shape 轮廓及五语诊断 |

圆弧半径由端点与圆心计算，不能直接使用源 Radius 字段。hatch 圆心按 ties-to-even 舍入后缩放，半径取两端到圆心距离的平均值。非 hatch 使用起点半径。曲线仍保留在路径中；铜皮轮廓使用与 Web 相同的 `0.00025 mm` chord tolerance，后续 GPU 走线和板框继续使用解析曲线。

路径回到首边表示闭合，进入其他已访问边则报环；owner、header sentinel、零链接及已知非边记录结束路径。孔洞回到所属 shape 结束，孔洞重复则报环。与 Web 在缺失路径链接处静默结束的行为不同，原生实现返回定位到引用方的结构化缺失诊断，避免发布截断几何。非边/非孔洞终点不解码无关的变长载荷。

路径边尚未绑定业务 owner，暂用 `track_id = 0`、`net = 0`、`LayerId::UNASSIGNED`；后续 scene builder 负责赋予图层、网络和归属。本轮不能证明网络连接、bond wire 或焊盘语义正确。

## 预算与 i18n

每次路径/形状查询默认最多 1,000,000 条边、65,536 个路径/孔洞入口、8,000,000 个轮廓点，保守分配记账上限 512 MiB。检查发生在分配/集合插入前；记账包括容器最小容量与增长余量，但不是整板峰值 RSS。后续 scene builder 还需跨对象的总预算，不能让每个对象的预算累加成无限场景。

路径每条边、孔洞每次迭代检查取消；离散化在边界及每 1024 个插值点检查取消。失败产生的临时路径和轮廓不提交为成功场景。

新增 `BRD_UNSUPPORTED_UNITS`、`BRD_INVALID_GEOMETRY`、`IMPORT_GEOMETRY_LIMIT` 均提供英/简中/繁中/日/韩资源，继续复用引用和非法记录诊断。缺失图层名在模型中保持空值，通过 `Layer::display_name(locale)` 生成五语默认名；源名称不翻译，解析结果不固化当前语言。

## 实际验证

| 检查 | 结果与范围 |
| --- | --- |
| 原生几何抽样 | 156 个案例，4,368 条请求，0 失败；每文件/类型均匀选取最多 4 条，包含首尾 |
| Web 几何差分 | 156/156 匹配；1,732 个图层、36,662 条路径、35,414 个轮廓、3,641,281 个轮廓点、166,791 条路径边 |
| 逐字段比较 | 9,318,155 个叶字段；ID、归属占位、flags、比例、文字与颜色精确比较；几何浮点容差 `1e-12 × max(1, abs(Web), abs(Rust))` |
| 负向对照 | 圆弧 sweep 反号、孔洞点 X +0.01 mm、重复抽样偏移、删除类型覆盖均被拒绝，并以非零状态退出 |
| 无窗口验证 | 单位、层 flags/名称、圆弧/舍入/完整圆、四种边、闭合/owner、方孔、离散化容差、错误定位、预算与取消；CLI 五语/源身份/非法请求 |
| 工程检查 | 工作区 112 项测试通过、0 失败；Clippy all-targets `-D warnings`、格式检查和 Windows debug 应用构建通过；不代表 release 或实窗整板验收 |

两端显式使用 Windows-1252 进行本轮布局与几何对照，不能据此确认原始文字编码。真实案例仍只有此前记录的 10 个布局族；本轮几何合成 fixture 使用 V174，其他布局的字段读取沿用已有 decoder 测试与实际差分证据。

差分先校验冻结 Web 的 444 个文件、156 案例集合、源 SHA-256 与大小，以及完整索引的类型分母。Web 再从原始 BRD 独立构建数据库，未使用 Rust 路径或轮廓作为期望值；每个抽样 span 核对索引、源布局的 Key/type/边界，拒绝重复偏移，核对每种类型的数量。以上仅覆盖抽样查询及其全部后继边/孔洞，不是整板所有对象的场景差分。

## 复现

使用已冻结并验证的索引报告及案例清单：

```powershell
python -X utf8 scripts/cargo.py +stable build -p pomelo-import --bin pcb_inspect --locked --offline
python -X utf8 scripts/probe-records.py .cache/indexes-windows1252.jsonl .cache/geometry-probes target/debug/pcb_inspect.exe --geometry --samples 4
node C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/tsx/dist/cli.mjs --tsconfig C:/Users/Zen/Desktop/gitrepo/pomelo/tsconfig.json scripts/check-geometry-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo .cache/geometry-probes/manifest.jsonl .cache/web-inputs.json .cache/cases-manifest.jsonl .cache/indexes-windows1252.jsonl .cache/geometry-parity.jsonl
python -X utf8 scripts/cargo.py +stable test --workspace --locked --offline
python -X utf8 scripts/cargo.py +stable clippy --workspace --all-targets --locked --offline -- -D warnings
```

报告位于 `.cache/geometry-probes/`、`.cache/geometry-parity.jsonl` 及对应 summary；负向材料为 `.cache/geometry-negative/` 中的副本。工具不修改外部 BRD 与 Web 源码，正式应用不携带 Node/TypeScript。`decode-geometry` 请求沿用记录 probe 的 schema version 1、2 MiB/4096 span 限制；报告首行是 metadata，后续每行一个请求，所有行保留 `scene_validated: false`。

下一步移植 padstack/焊盘/过孔及网络归属、文字/绘图和铜皮网格，完善 BoardScene 后再做完整场景差分并接入 Windows 视口。
