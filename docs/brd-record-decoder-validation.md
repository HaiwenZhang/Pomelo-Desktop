# BRD 按需记录解码与验证

> 本文保留历史验证记录，其中引用的辅助脚本已移除；当前 `scripts/` 仅保留 Windows 打包工具。

日期：2026-10-02。当前开发平台为 Windows。本轮完成固定及变长记录的原生字段解码、数据库按 Key 查询和受预算约束的链表遍历；**尚未完成 BoardScene、整板语义对照或桌面真实 PCB 查看**。

## 实现范围

| 模块 | 已有行为 |
| --- | --- |
| `pomelo-import/src/allegro/decoder/fixed.rs` | 42 个类型标签、40 个类型化结构；保留源字段、版本差异、带符号坐标、特殊 f64 word 字序、V15 十位 flags 和旧版内联名称 |
| `decoder/variable.rs` | 13 种变长记录：属性字段、成对网络、padstack、约束集、SI 模型、padstack 尺寸、blob、约束区、图层表、文字、定义/字体表、命名属性、Key 列表 |
| `decoder/mod.rs` | 以索引 span 限制每次读取，核对类型、Key 和最终偏移；保留绝对偏移与字符串对齐；可取消，无完整记录 AST 缓存 |
| `database.rs` | 拥有一份源缓冲与索引；按 Key 查询、按类型惰性遍历；必需引用的缺失/类型检查；链表 sentinel、环检测、预算和取消 |
| `pcb_inspect` | `decode-fixed` / `decode-records`；请求绑定源 SHA-256 与大小；输出类型化字段和五语结构化错误 |

固定记录中的旧版可选字段保持缺席，不能用默认零补出不存在的字段。真实 V15 差分发现并修复了元件引用名、功能实例、封装实例与文字包装记录的这类问题，已有专门回归测试。

变长记录保留旧版 padstack 最后一个组件省略 Z2、V15 受限层数组压缩、现代钻孔元数据、V174/V175/V251 字体间距和笔画宽度位置等差异。属性 755 的 80 字节尺寸设置、嵌入 STEP/ACIS 模型保持原始二进制，包含 NUL，不作为文本解码；2D 查看器不据此宣称支持 3D 模型。

定义表的非字体项、blob 与约束区目前沿用冻结 Web 的已知字段/跳过语义；源字节仍由数据库持有。未验证的成对网络布局明确报错：`0x1a` 只按参考实现接受 152、157、172、174、251，不能外推其他版本。

## 预算、引用与 i18n

变长记录默认拥有数据分配的保守记账上限为每条 64 MiB、源文本 16 MiB、集合 1,000,000 项；padstack 同时限制最多 256 层。计数、源边界和预算在分配前检查，长数组每 1024 项检查取消。固定记录的数组和文本长度由常量布局限定。记账不是进程 RSS 或所有阶段的总内存指标，完整场景仍需独立预算。

源 extent 与分配预算分别检查：真实案例含最高约 214 MB 的不透明约束区，按 header 给出的结束偏移直接跳过，不复制、不遍历区内数据，也不因 extent 大就声称需要同样大小的解码分配。仍须落在源文件和该记录 span 之内；边界回归测试覆盖这条约束。

链表默认最多 1,000,000 条记录，访问集合按每个 Key 64 字节保守记账，默认上限 64 MiB；遇到零链接或调用方指定 sentinel 终止。只逐条解码和访问，不收集整条记录 AST。访问器产生的临时结果在遍历失败时不能提交为成功场景。源 offset 与记录 Key 为独立类型；缺失引用/类型错误的偏移指向实际引用方，循环错误保留涉及的 Key。

新增 `IMPORT_DECODE_LIMIT`、`BRD_MISSING_REFERENCE`、`BRD_REFERENCE_TYPE`、`BRD_REFERENCE_CYCLE` 及 CLI 请求诊断同步进入 rust-i18n 五语资源。诊断保存稳定 code、参数和源偏移；语言改变不修改记录字段或文件编码。CLI 请求上限 2 MiB / 4096 条 span，源身份不一致时不创建或覆盖报告。

## 实际验证结果

| 检查 | 结果 | 验证边界 |
| --- | --- | --- |
| 156 原始 BRD 的原生记录抽样 | 204,384 条解码，0 失败 | 每文件、每种出现的类型均匀选取最多 32 条，包含首尾；不足 32 条则全取 |
| 冻结 Web 逐字段差分 | 156/156 匹配；204,384 条记录；78,962,958 个叶字段 | 覆盖实际出现的 53 种类型；包含数组元素与二进制字节；不是全部 118,101,604 条记录的字段解码 |
| 版本覆盖 | 真实文件 10 个布局族；合成 fixture 13 个布局族 | 160、180、251 无本目录真实文件；`0x29` 和 `0x3e` 仅有合成验证 |
| 差分负向对照 | 字段 Width +1、重复偏移、删除一种类型覆盖均被拒绝 | 字段错误定位到 `0x1024b4.Width`；三次均非零退出，不修改原案例 |
| 无窗口工作区测试 | 93 项通过，0 失败 | 固定/变长 decoder、数据库/链表、CLI 五语、资源/源码门禁及已有核心、应用、渲染测试 |
| Clippy / 格式 | 工作区 all-targets `-D warnings` 通过；格式检查通过 | 不代表实窗渲染或发布性能 |

两端在本轮差分中**显式使用 Windows-1252**，便于先验证布局与字段；不能据此推断案例的真实文字编码已正确识别。默认仍是严格 UTF-8，GBK / Big5 / Shift-JIS 等编码由调用方明确选择。场景文字的源语义与用户编码选择尚待进一步复核。

差分先核对冻结 Web 的 444 个文件哈希、案例集合和 SHA-256，再核对每个 span 在已验证索引中的 offset/length/Key/type。类型分母来自完整索引报告，拒绝重复抽样偏移，并检查每种类型的抽样数量。逐叶比较整数、ID、字符串、有限 f64 和数组值，不放宽浮点容差。Web 的 `Uint8Array` 仅规范化为字节数组；缺席字段与非有限浮点按 JSON 规则规范化。

本地报告为 `.cache/record-probes/manifest.jsonl`、各文件 `.fields.jsonl`、`.cache/record-parity.jsonl` 和 `.cache/record-parity.jsonl.summary.json`。数据库/引用保护当前由合成测试证明，真实板上的语义引用关系尚未逐项验收。所有报告保持 `scene_validated: false` / `sceneValidated: false`。

## 复现

先按 [索引验证](brd-index-validation.md) 生成 `.cache/indexes-windows1252.jsonl` 及逐记录索引证据，并冻结 Web/案例清单。然后执行：

```powershell
cargo +stable build -p pomelo-import --bin pcb_inspect --locked --offline
python scripts/probe-records.py .cache/indexes-windows1252.jsonl .cache/record-probes target/debug/pcb_inspect.exe --samples 32 --locale en
node C:\Users\Zen\Desktop\gitrepo\pomelo\node_modules\tsx\dist\cli.mjs --tsconfig C:\Users\Zen\Desktop\gitrepo\pomelo\tsconfig.json scripts/check-record-parity.mts C:\Users\Zen\Desktop\gitrepo\pomelo .cache/record-probes/manifest.jsonl .cache/web-inputs.json .cache/cases-manifest.jsonl .cache/indexes-windows1252.jsonl .cache/record-parity.jsonl
cargo +stable test --workspace --locked --offline
cargo +stable clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +stable fmt --all -- --check
```

`probe-records.py --fixed-only` 可单独验证固定布局。原生程序不依赖 Python 或 Node；这些脚本只服务研发差分。本机 Node/tsx 的 `uv_os_get_passwd` 沙箱限制仍需在沙箱外执行只读 Web 对照。

下一步实现图层/网络/元件与几何的语义 decoder、共享 padstack/字体缓存、场景构建与场景差分，然后接入桌面导入和 Windows D3D11 常驻 GPU 批次。完整目标仍在开发。
