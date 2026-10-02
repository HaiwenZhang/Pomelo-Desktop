# BRD 走线、过孔与键合对象验证

日期：2026-10-02。Windows 原生语义查询已实现，156 个冻结案例的独立对照全部匹配。**这是走线/过孔查询验证，不是原生整板场景或 GPU 画面验收。** `AllegroImporter::import` 仍返回场景未实现诊断；后续完成文字、绘图、铜皮网格、场景预算和 SceneBuilder 后再接入桌面打开流程。

## 实现与职责

- `allegro/semantics/placement/routing.rs`：`PlacementDecoder::track`，保留源段 ID、走线 ID、物理层、网络、宽度及解析圆弧；识别独立的 TOP 键合线层。
- `placement/via.rs`：`PlacementDecoder::via`，实现普通/反转/翻面层序、区域引用、背钻与键合指；普通及背钻焊盘配方采用不可变 `Arc<[Pad]>` 共享，自定义轮廓继续共享。
- `semantics/bond.rs`：从源键合线端点建立键合指→引脚关系；多引脚歧义永久保留为未关联，不能用过孔的网络 `Next` 猜测。键合线验证裸片、前面封装、TOP 指、网络一致、单段标志、端点和属性链，保留源 profile、material、元件与引脚名。
- `pcb_inspect decode-routing`：通过源 SHA-256 绑定的请求查询走线/过孔，输出身份、物理层/单位、结构化诊断和当前语言文案。

GPUI 补丁、D3D11 与 HLSL 没有本轮业务改动。解析不取得窗口/GPU 句柄，符合四 crate 依赖方向。新增过孔定义缺失、键合指未支持、键合线未支持、走线层未定义四个消息从实现时同步英文、简中、繁中、日文、韩文。

## 冻结输入与对照方法

沿用 `.cache/web-inputs.json`、`.cache/cases-manifest.jsonl`、`.cache/indexes-windows1252.jsonl` 的源码、案例与逐记录索引证据：156 个源文件，7,745,124,096 字节。真实布局族为 152、157、162、164、165、166、172、174、175、181；未据此扩展其他布局族的真实文件验证范围。

两端显式使用 Windows-1252，便于保持字符串和索引契约一致；**不证明全部原始文件实际采用此编码。** 解析默认编码选择、源文字与界面编码切换仍需后续验证。

每个文件的 `0x05` 走线和 `0x33` 过孔各选至多 32 个均匀源序号，另加入源记录层值 `0xfd06` 的全部键合线及 `0xc012` 的全部键合指，去重后按文件偏移排列。共 10,027 条请求，包含 56 条特殊层记录。背钻、区域和普通过孔仍是抽样；未宣称全量过孔或全量走线对照。

`scripts/check-routing-parity.mts` 独立读取冻结 Web 源文件并运行完整 `AllegroParser`、`AllegroSceneBuilder`，再按请求身份抽取预期对象；没有复写原生的放置公式，也没有替换 Web 铜皮或文字构建器。核对源码/案例哈希、案例集合、源类型计数、抽样序号、特殊记录全集、索引签名与记录边界、物理层/单位、查询对象及相关诊断。

ID、网络、图层、类型、源引用、文字与诊断要求精确；几何浮点使用 `1e-12 × max(1, |Web|, |Rust|)` 容差，数组顺序不放宽。JSON 对象成员顺序不参与比较。源 warning 规范化为稳定 code、消息键、参数、对象、偏移和 severity；译文的五语契约另外由工作区测试验证。

## 结果与边界

| 指标 | 本轮实际匹配 |
| --- | ---: |
| 案例 | 156 / 156 |
| 走线查询 | 5,015 |
| 过孔查询及保留对象 | 5,012 |
| 段/圆弧 | 23,293 |
| 其中解析圆弧 | 1,142 |
| 放置后焊盘值 | 40,266 |
| 叶字段 | 752,885 |
| 键合线 / 键合指 | 28 / 28，覆盖当前案例的全部相应特殊层记录 |
| 反转 / 翻面过孔 | 32 / 34 |
| 背钻 / 区域引用 | 3 / 10 |
| 自定义焊盘值 | 5 |

完整 Web 场景作为 oracle，并不意味着 Native 已构建完整场景。结果不能替代全量场景对象、板边界、铜皮网格、文本、拾取、真实窗口或大板性能验收。

真实文件 `211215_LPDDR5_Board_SNI.brd` 的过孔 11065 引用 Padstack 5175：LayerCount=0、PadType=0、DrillSize=0。Web 保留对象但给出带 `-1` 的层范围。Native 保留 ID、坐标、网络和空焊盘，把 `start_layer` / `end_layer` 都表示为 `None`，避免伪造物理层；oracle 只在源定义明确 LayerCount=0 时做此规范化。此差异有真实案例和合成回归证据。

普通走线链允许零/所属走线作为终点，其他环报错。缺失或错误类型的必要段引用在 Native 返回可定位失败；Web 对这两类情况会警告并停止链遍历。此次真实抽样未触发该差异。未知键合端点/profile/段变体与属性链环作为未支持诊断保留，不猜测图形。

## 预算、取消与缓存

`PlacementLimits` 默认对象/返回数据预算 2 GiB、8,000,000 次记账；Padstack 与自定义几何缓存分别有独立限额。键合关联表默认 256 MiB / 100,000 项，受 `links` 单独约束。每条链默认最多 1,000,000 个引用与 64 MiB 访问集合；走线还遵守 `GeometryLimits` 的段数和分配限额。限额在分配/克隆前检查，不能用丢弃对象绕过预算。

每个源记录、链节点、焊盘和属性读取都检查取消；走线段/引脚构建及键合关联扫描每 256 条上报阶段进度，批量过孔阶段进度留给后续 SceneBuilder 统一治理。返回对象按需拷贝小元数据，自定义几何和焊盘配方共享。取消或失败不发布部分查询结果，重试使用新的构建器/预算；缓存命中也检查取消。

合成测试覆盖三种层序、配方共享/隔离、网络归属优先规则、旋转键合指、来源歧义、端点未舍入坐标、属性环/未知变体、背钻物理保护层与独立放置标志、零层对象、预算、处理中途取消和完整重试。CLI 验证五语文案与不变身份/偏移、错误请求，以及源哈希不匹配时保留已有报告。

四个负向对照仅修改缓存副本，均退出 1 并定位预期错误：过孔焊盘 layer +1、走线 net +1、键合线 sourcePin +1、同时从请求/报告删除一条特殊层记录。前两个使用 `0GYJS-0615-1100.brd`，后两个使用 `camera_test_board.brd`；它们证明比较器能识别这些错误，不扩展为其他差分维度已验证。

## 复现与证据

```powershell
python scripts/cargo.py +stable build -p pomelo-import --bin pcb_inspect --locked --offline
python scripts/probe-records.py .cache/indexes-windows1252.jsonl .cache/routing-probes target/debug/pcb_inspect.exe --routing --samples 32
node --max-old-space-size=24576 --import file:///C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/tsx/dist/loader.mjs scripts/check-routing-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo .cache/routing-probes/manifest.jsonl .cache/web-inputs.json .cache/cases-manifest.jsonl .cache/indexes-windows1252.jsonl .cache/routing-parity.jsonl
python scripts/cargo.py +stable test --workspace --locked --offline
python scripts/cargo.py +stable clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
python scripts/cargo.py +stable fmt --all -- --check
```

本机使用 `+stable`，实际版本与仓库锁定的 Rust 1.98.1 相同。Node 为开发对照工具，不进入桌面运行依赖。`.cache/` 报告不入库，需要保留或按上述命令重建；案例与 Web 源码只读。

- `.cache/routing-probes/manifest.jsonl`、各请求与 `.fields.jsonl`：原生探针。
- `.cache/routing-parity.jsonl` 及 `.summary.json`：独立 oracle，156 匹配、0 失败。
- `.cache/routing-negative/verified.json`：四种改坏报告被精确拒绝。
- `.cache/routing-workspace-tests.log`：工作区 173 项测试，27 个结果分组全部通过；含 i18n 门禁，不含新增 GPU 实窗证据。
- `.cache/routing-final-clippy.log`、`.cache/routing-final-build.log`：最终工作区 lint 与 Windows debug 构建证据；release 尚未验收。
- `.cache/routing-final-probes/`、`.cache/routing-final-recheck.json`：最终 Windows 工作区构建重新生成全部 156 案例、10,027 查询；与已通过独立 oracle 的报告逐字段精确一致，数组/数值不放宽，仅忽略 JSON 对象成员顺序。

下一步实现绘图/文字、铜皮网格、场景总预算与 SceneBuilder，完成整板对照后接入 GPUI 视口及真实 PCB D3D11/HLSL 批次。
