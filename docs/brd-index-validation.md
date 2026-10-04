# BRD 记录索引验证

> 本文保留历史验证记录，其中引用的辅助脚本已移除；当前 `scripts/` 仅保留 Windows 打包工具。

验证日期：2026-10-02。开发范围为 Windows。本轮完成 M2 的字符串表、记录边界扫描和索引；`AllegroImporter::import` 仍返回场景未实现诊断，桌面打开流程目前仍是头部探测。

## 实现与边界

`pomelo-import/src/allegro/index.rs` 提供 `BrdIndex`，保存一份紧凑的 `RecordSpan` 列表，以及 Key / 类型到列表位置的索引。`RecordKey` 与 `FileOffset` 分开建模；不建立完整记录 AST，不复制源文件。每条记录保存类型、源 Key、起始偏移和字节长度，后续 decoder 按需读取。

`record_scan.rs` 移植冻结 Web 的固定长度和变长布局。覆盖 V15 packed tag、内联文本、特殊 Key 位置、旧 padstack 最后一项缺失 Z2、V251 字体表、V18 零填充间隔、属性二进制附件及对齐。未知非零类型、重复非零记录 Key、重复字符串 Key（包括零）、截断和非法计数返回结构化诊断；零 Key 的记录保留在记录列表中，不进入身份查找表。

`source::read_path` 按 1 MiB 分块读取，读取前和文件增长时检查预算；不按未经读取的 metadata 一次分配整个文件。取消检查贯穿读取、每条记录、字符串表和长零区；索引进度约 10 Hz，最终回调后再次检查取消。合成测试验证取消结果，尚未完成真实大板的 500 ms 响应时间验收。

当前默认输入预算为 1 GiB，索引保守记账预算为 2 GiB，单段文本的源字节上限为 16 MiB。记账包含容器增长余量，每条记录 96 B、每条字符串 128 B 加三倍 UTF-8 长度和基础开销；**`accounted_bytes` 不是进程 RSS**。初始 512 MiB 索引预算拒绝了两个压力案例，因此调整为明确、有界的 2 GiB；这不代表完整场景、GPU 或多标签的总内存预算已经验证。后续按场景生命周期释放源缓冲并测量 RSS。

新错误及 CLI 参数错误均有英、简中、繁中、日、韩资源；注册表、占位符和结构化字段测试通过。文本编码显式支持 UTF-8、GBK、Shift-JIS、Big5、Windows-1252，与 UI 语言独立。

UTF-8 BOM 按 Web `TextDecoder` 默认规则处理，其他码页保持原字节含义；fixture 验证对齐和原始错误偏移。

## 实际验证结果

| 检查 | 结果 |
| --- | --- |
| 工作区自动测试 | 57 项通过，0 失败；包含 14 项索引测试 |
| 工作区 Clippy | `--all-targets -- -D warnings` 通过 |
| 外部 BRD 索引扫描 | 156 / 156 通过，输入共 7,745,124,096 B；显式选择 Windows-1252 |
| 冻结 Web 输入 | 差分前验证 444 项源文件 SHA-256，全部保持一致 |
| 逐记录差分 | 118,101,604 条记录的类型、Key、起始偏移及字节长度全部精确匹配 |
| 字符串表差分 | 1,452,646 个字符串的 Key 和解码结果全部精确匹配 |
| 汇总差分 | 类型计数、总计数、非零 Key 数、对象起点和流结束偏移一致 |
| 负向对照 | 修改最小案例第 0 条记录偏移 `6240 → 6244`，脚本报告该记录的具体差异并非零退出 |
| 默认严格 UTF-8 扫描 | 107 通过，49 因文本解码错误返回可定位诊断；无自动换编码或替换字符 |

真实样本涵盖 152、157、162、164、165、166、172、174、175、181 十个布局族；160、180、251 的边界分支由合成 fixture 覆盖。不能把这些测试当成三种布局已通过真实板场景验证。

Windows-1252 是本轮两端统一的**记录布局验证参数**。它保证同一解码器设置下字符串可比较，不能证明每个文件原始编码为 Windows-1252，也不能证明 CJK 板文字显示正确。Web 的默认 UTF-8 解码器会记录 issue 并生成替换字符；Rust 当前严格返回错误，要求明确选编码。后续 UI 必须提供编码选择和重试，并用已知源文本复核，不能静默使用此差分参数打开所有板。

两处通过真实差分修正的移植错误均加入 fixture：二进制附件后必须按绝对偏移四字节对齐；V15 `0x08` 内联引脚编号之后有三个 link word，共 12 B，不能跳过 16 B。

| 压力案例 | 索引记录数 | 字符串数 | 索引记账 B |
| --- | ---: | ---: | ---: |
| `15061-1b.brd` | 16,407,627 | 27,117 | 1,579,372,634 |
| `ntpcb_320mb.brd` | 5,670,127 | 61,556 | 554,563,764 |
| `S5000C-64_DDR5_BGA_V0.61.brd` | 5,970,473 | 36,183 | 578,739,452 |

头部对象计数与实际索引记录数含义不同，报告分别保留。以上运行使用 Windows debug 配置，未测量点击到可交互、场景构建、release 性能或 GPU 绘制。

## 复现命令与报告契约

外部 Web 源码和 BRD 案例只读，报告及逐条证据写入本地 `.cache/`，不入库。

```powershell
cargo +stable run -p pomelo-import --bin pcb_inspect --locked --offline -- --locale zh-CN --encoding windows-1252 check --stage index --cases-dir E:\brd_cases --report .cache\indexes-windows1252.jsonl --index-data-dir .cache\index-data-windows1252

node C:\Users\Zen\Desktop\gitrepo\pomelo\node_modules\tsx\dist\cli.mjs --tsconfig C:\Users\Zen\Desktop\gitrepo\pomelo\tsconfig.json scripts\check-index-parity.mts C:\Users\Zen\Desktop\gitrepo\pomelo .cache\indexes-windows1252.jsonl .cache\web-inputs.json .cache\cases-manifest.jsonl .cache\index-parity.jsonl

cargo +stable run -p pomelo-import --bin pcb_inspect --locked --offline -- --locale ja index E:\brd_cases\example.brd
```

`example.brd` 替换为真实文件。单文件 `index` 和批量 `check --stage index` 均可用；`--index-data-dir` 仅用于批量索引证据导出。Node/tsx 只用于研发差分，桌面程序没有 JavaScript 运行依赖。本机 tsx 在沙箱内遇到 `uv_os_get_passwd` 限制，沙箱外执行上述只读对照取得结果。

CLI JSONL schema version 为 3，新增 `encoding`、`index` 和 `index_data_path`，继续保留稳定 `error.code/message.args/offset`、locale 和展示消息。报告阶段明确为 `index`，汇总和差分报告保持 `scene_validated: false` / `sceneValidated: false`。失败案例仍写入报告并使命令非零退出。

可选 `.idx` 证据格式为 `PMIDX001`：8 B 签名、little-endian u32 记录数/字符串数；每条记录四个 u32（offset、length、key、type）；随后字符串按 unsigned Key 排序，每项为 u32 Key、u32 UTF-8 字节长度、原始 UTF-8 字节，无填充。差分脚本逐条读取并检查每个字段，遇到差异输出记录序号/类型/偏移；SHA-256 仅校验输入身份，不能代替字段比较。

本轮证据：`.cache/indexes-windows1252.jsonl`、`.cache/index-parity.jsonl`、`.cache/indexes-utf8.jsonl` 和 `.cache/parity-negative-result.jsonl`。下一阶段实现按版本的语义 decoder、引用检查和 scene builder，随后进行场景差分并接入 Windows 原生 GPU 视口。
