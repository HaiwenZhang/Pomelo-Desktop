# BRD Padstack、焊盘与钻孔验证

日期：2026-10-02。当前开发与验证平台为 Windows。本轮实现 padstack 引用、焊盘定义、钻孔和背钻的语义，并与冻结 Web 源码进行独立差分。**这不是完整 BoardScene、放置关系、整板 GPU 画面或 BRD MVP 验收。**

## 实现范围

| 模块 | 已有行为 |
| --- | --- |
| `pomelo-import/src/allegro/semantics/padstack.rs` | 按需读取并共享 padstack；区分直接引用、单层嵌入焊盘、die pad、区域焊盘/过孔、双端背钻和 bond finger 旋转 |
| `semantics/pad.rs` | 焊盘类型/尺寸、圆环内径、自定义轮廓、槽孔方向、镀层、背钻显示与基础圆；缓存和结构化 warning |
| `pomelo-core/src/model.rs`、`pad.rs` | 保留源形状家族、共享自定义几何及解析边界、钻孔/背钻/区域身份；焊盘局部变换、包围盒和钻孔显示形状 |
| `pcb_inspect decode-padstack` | 同一 SHA-256 绑定请求；定义预览、引脚/过孔引用解析、源单位及 mm 背钻参数、五语诊断 |
| `scripts/padstack-oracle.mts` | 调用冻结 Web 的 padstack/pad/drill/bond-finger/backdrill 实现；只规范化字段表示，不使用 Rust 结果生成期望值 |

定义预览使用 padstack 自身的层序和偏移。嵌入层、区域码和 die 标志分别输出；它们尚未变成完整放置后的 Pin/Via 场景。元件父子关系、引脚放置舍入、旋转/镜像组合、网络归属和背面层映射留给 scene builder，不能把该预览直接当作整板结果。

## 保留的语义区别

- 类型 2、5、25 的高度使用源宽度。零尺寸且非负的矩形表示没有铜，保留源定义而不生成最小尺寸假焊盘。
- 类型 25 的 `Z1` 是圆环内径，必须在零与外径之间；不以 `DrillSize` 替代透明开口。
- 类型 22 保留填充轮廓和原始解析边界。缺失尺寸由有效轮廓的范围补足；放置偏移不修改共享几何。缺失自定义几何或未知家族产生 warning，不能用矩形冒充支持。
- 槽孔宽高由 `SlotX/SlotY` 定义，并根据首个普通层焊盘的长轴调整方向；`SlotY = 0` 时使用圆形 `DrillSize`。镀层沿用版本化 decoder 的结果。
- 单层嵌入焊盘允许 opaque word 3 非零；该值不是几何指针。die pad 的特殊层编码 `0xfc00` 不变成物理铜层 252。区域码独立于物理层，区域过孔仍保持经过验证的全层跨度。
- 背钻元数据按版本取不同 word，并将 unsigned word 按 i32 解释后取绝对值；`i32::MIN` 不溢出。START 普通焊盘只取组件槽 5，不能用 solder-mask 槽 14/15 替代。
- 背钻停止层与保护层分开；双端切除不能重叠。进入层的基础圆使用 START 尺寸，内部基础圆不超过普通焊盘；增强显示圆与标签包络直径独立，保护层保持原有铜。
- 自定义轮廓按 Web 契约镜像局部 Y，再旋转；焊盘 offset 已在板空间，不能再次随局部轮廓旋转。元件/引脚的源坐标镜像属于不同的放置步骤。

## 缓存、预算与 i18n

padstack 定义使用 `Arc` 共享；自定义轮廓按源 shape key 共享；钻孔按 padstack key 缓存。缓存随一次构建存活，不跨文件共享，不允许放置对象修改定义。

padstack resolver 与 pad decoder 各自默认限制 256 MiB、100,000 个缓存/诊断入口。记账包含容器增长和诊断参数；在解码/插入前检查。自定义轮廓的几何查询还受剩余缓存预算约束，不能先分配完整大轮廓再检查缓存。取消检查贯穿引用、缓存命中、几何和背钻应用。失败的插入不消耗成功缓存预算。

这些是分项预算，**不是整板峰值内存保证**。源缓冲、索引、临时查询和后续场景缓存仍需 scene builder 的总预算与真实 RSS/GPU 内存验收。

新增 `IMPORT_SEMANTIC_CACHE_LIMIT`、`BRD_PAD_DIMENSIONS`、`BRD_PAD_DONUT`、`BRD_PAD_UNSUPPORTED` 使用稳定 code、消息键、类型化参数、源 key/offset，并同步提供英/简中/繁中/日/韩。warning 按源身份和原因去重；切换语言重新格式化已有诊断，不重新解析。驱动、源名称和源路径保持原值。

## 实际证据

| 检查 | 结果与范围 |
| --- | --- |
| 原生抽样 | 156 案例、16,149 请求，0 失败；每文件/类型均匀选择最多 32 条，包含首尾；覆盖 `0x1c/0x2f/0x32/0x33` |
| Web 独立差分 | 156/156 匹配，1,065,946 个叶字段；65,901 个普通焊盘值、642 个自定义轮廓、22,300 个轮廓点、5,380 条解析边 |
| 特殊定义抽样 | 157 次槽孔定义预览、15 次 die、1,192 次嵌入层、87 次区域、163 次背钻、8 次 bond finger 引用/放置角解析 |
| 负向对照 | 普通焊盘宽度 +0.01 mm、自定义轮廓点 X +0.01 mm、背钻保护层 +1 均以非零状态失败，定位到被修改的字段 |
| 合成测试 | 21 项语义测试：特殊引用的接受/拒绝、双端/保护层/元数据槽、signed 最小值、槽孔、圆环、零矩形、自定义孔洞、共享缓存、预算、取消、旋转与偏移；另有 2 项 CLI 五语/源身份测试 |
| 工程检查 | 工作区 135 项测试通过、0 失败；Clippy all-targets `-D warnings`、格式检查、Windows debug 应用构建通过 |
| 最终原生复核 | 用最终工作区构建重新运行全部 156 个请求文件，与已通过 Web 差分的报告逐字段精确一致；数组顺序和数值保留 |
| 既有几何模式回归 | 差分工具增加 padstack 模式后，原有 156 案例、4,368 个几何请求仍全部匹配 |

上述数量是**抽样查询输出**，包含重复引用和定义预览，不是整板唯一对象数量。两端显式使用 Windows-1252；原始源文字编码仍未确认。真实样本覆盖既有 10 个布局族，不能据此宣称其他布局已实机/真实文件验证。

本次抽样没有选到圆环焊盘；圆环内径目前只有合成测试证据。自定义焊盘孔洞也有专门合成测试，但本次 642 个真实预览均只有一个轮廓。后续需要针对这些形状做真实文件专项查询和整板场景对照。

最终复核发现独立 importer 构建与 workspace 构建的 JSON 对象成员顺序不同：GPUI 启用 `serde_json/preserve_order`。复核因此比较完整 JSON 的规范化成员顺序，所有字段、数组顺序及数值保持精确；不是把有差异的几何放宽到更大容差。

## 复现与报告

```powershell
python -X utf8 scripts/cargo.py +stable build -p pomelo-import --bin pcb_inspect --locked --offline
python -X utf8 scripts/probe-records.py .cache/indexes-windows1252.jsonl .cache/padstack-probes target/debug/pcb_inspect.exe --padstack --samples 32
node --max-old-space-size=24576 --import file:///C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/tsx/dist/loader.mjs scripts/check-geometry-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo .cache/padstack-probes/manifest.jsonl .cache/web-inputs.json .cache/cases-manifest.jsonl .cache/indexes-windows1252.jsonl .cache/padstack-parity.jsonl padstack
python -X utf8 scripts/cargo.py +stable test --workspace --locked --offline
python -X utf8 scripts/cargo.py +stable clippy --workspace --all-targets --locked --offline -- -D warnings
```

报告保存在 `.cache/padstack-probes/`、`.cache/padstack-parity.jsonl` 及 summary；细分覆盖统计、负向副本和最终复核分别位于 `.cache/padstack-coverage.json`、`.cache/padstack-negative/`、`.cache/padstack-final-recheck.json`。外部 Web 源码和 BRD 均只读；正式桌面运行不携带 Node/TypeScript。

`decode-padstack` 沿用 schema version 1 的 2 MiB/4096 span 请求限制，读取前验证源 SHA-256/大小，逐条验证精确索引边界和 Key/type/长度。JSONL 首行为 metadata，其后每个请求一行，最后为去重诊断及所选语言文案；全部行保留 `scene_validated: false`。未知引用变体输出未解析值，不能依据 probe 命令成功就认定该变体受支持。

完整场景、网络/元件关系、文字、铜皮网格、场景总预算、拾取和真实板 GPU 视口仍待完成；`AllegroImporter::import` 继续明确返回未实现诊断。
