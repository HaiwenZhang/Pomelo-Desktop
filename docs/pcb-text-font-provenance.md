# PCB 字体来源与历史核对

> 2026-10-04：已移除 KiCad stroke 字形资源、JSON/编码加载器、专用测试和打包通知。当前默认 PCB 字体使用 Source Han Sans MSDF；ANSI 笔画字体由调用方显式提供。下文仅为旧实现的历史核对记录，所述接入要求不再适用于当前产品。

核对日期：2026-10-02。此记录用于 Windows 文字渲染的资源接入决策，不代表文字已完成 GPU 绘制或字体已纳入发布包。

## 当前证据

- Web 的 `src/lib/text/stroke-font-data.ts` 和 `stroke-font-blocks.ts` 标注源自 KiCad `newstroke_font.cpp`，SPDX 为 GPL-2.0-or-later。
- `public/fonts/stroke/LICENSE-KiCad-stroke.txt` 实际存在；它同时记录 KiCad GPL-2.0-or-later、Lingdong Huang 的 MIT 声明、Source Han Sans 的 SIL OFL 1.1 声明。不能只读取 TypeScript 首部就认定整个资源只有一种许可证。
- [KiCad 官方源码镜像中的字体源文件](https://raw.githubusercontent.com/KiCad/kicad-source-mirror/master/common/newstroke_font.cpp) 首部也包含上述三项声明。此次查看的是浮动 master，只用于来源核对；它不是可复现的发布资源基线。
- Web 文件提到的 `scripts/generate-stroke-font.py` 在当前 Web 工作区不存在。因此当前还不能从该脚本复现生成数据，也未证明 Web 资源与某个固定 KiCad 修订逐字形一致。
- 桌面工作区 Cargo 包声明为 MIT。当前没有把上述字体数据复制、嵌入或改标 MIT；字体资源的许可与项目自有代码许可须分别记录。

## 后续接入要求

本地证据快照：核心字形 TypeScript 的 SHA-256 为 `DE17E76E17CADA8E3DCF91E8F7BFAAD19EB779E546219EFAA07ADD0A4AF7A781`；许可文本为 `EB632CE979B47FE8225D3F15A7EDF09ABBA266603834B05F841AE48D8884607E`。扩展资源包含 99 个 JSON 文件、合计 2,405,173 字节，最大文件 `9d.json` 为 36,940 字节。这些统计不证明覆盖全部 Unicode，也不代替逐字形校验。

1. 固定字体来源修订和完整数据哈希，恢复或编写可复现的转换工具；保留全部适用的原始版权及许可证文本。
2. 按字体来源分别核对分发条款和资源组合关系，再决定随包资源。不能把 KiCad 普通 footprint/symbol 库的许可证直接套用于字体，也不能仅凭字体属于数据就宣称 GPL 不适用。
3. 不将 Web 字形或上游渲染代码直接当成 MIT 资源。若另选来源，必须重新检查覆盖范围，并记录与 Web 字形的外观差异；不暗中替换为合成测试字体。
4. 正式资源由 `StrokeGlyphs` 提供，按所需字符加载、验证和缓存；未知字符使用五语结构化缺字诊断。离线运行不依赖字体下载。
5. 完成资源核对后继续整板数量/内存预算、后台取消、独立文字 GPU 缓存、图层合成和实际 BRD 验收。当前通用布局与变换实现继续保持不绑定字体资源。

目前没有认定本项目需要变更整体许可证；现有证据仅证明引入资源前仍需完成来源固定与具体分发条款核对。

## 扩展字形实际校验

通过 `python -X utf8 scripts/audit-stroke-font.py <Web>/public/fonts/stroke --output docs/gpu-validation/stroke-font-audit.json` 执行只读资源检查，结果见 [原始报告](gpu-validation/stroke-font-audit.json)。99 个 JSON 页包含 23,625 个字形、710,859 个编码点，13 个字形无点；单字形最多 204 个点（U+25D9）。所有页通过重复键、字符归属页、成对 ASCII 编码校验，每页哈希列入报告。

抽查“中文繁體日本語”均存在；“한글가나다”均不在这些扩展页中。该检查未解析核心 TypeScript 字形集，不能声称已完成全部字体差分。现有核心注释及扩展页列表也不能作为五种语言字形全覆盖承诺。五语界面由 UI 字体承担，源 PCB 文字保持原字符；韩文等缺字需要正式字体方案补足或按结构化诊断报告，不能用翻译原文字掩盖缺口。

检查证明资源满足当前解码格式约束，不证明资源许可已完成批准、与上游某个修订完全一致，或 GPU/实窗文字已显示。

2026-10-02 补充 Rust 实际加载证据：显式设置 `POMELO_STROKE_FONT_DIR` 后执行 `test -p pomelo-render --lib external_stroke_json_resources --locked --offline -- --ignored --nocapture`，99 页、23,625 字形均经实际 JSON visitor 与 decode_glyph 成功加载，共生成 400,827 条规范化笔画。逐页以 64 KiB 输入、256 字形、单字形 1,024 点、65,536 笔画预算验证，并确认字符页归属及上述 CJK 探针有非空笔画。测试只读外部数据，缺少明确路径时失败，不把资源缺失静默当通过；普通测试默认忽略此项。结果没有覆盖核心 TypeScript、上游差分、GPU 字形像素或产品字体发布。

核心数据补充：审核脚本新增 `--core <Web>/src/lib/text/stroke-font-data.ts`，以受限字面量语法读取数据，不执行 TypeScript。实际 303 个核心字形通过成对 ASCII 编码及重复键校验；与扩展页无重叠，合计 23,928 个字符。“Aa09Ω”抽查均存在，韩文探针仍缺失。哈希与此前快照一致，见 [核心及扩展审核报告](gpu-validation/stroke-font-core-audit.json)。这补齐了核心编码审核，仍不等于上游修订差分或 Rust 核心资源加载验证。

## 开发资源快照接入

为推进离线实现，当前选择固定已审核的 Web 工作区资源快照作为开发输入：`assets/fonts/stroke/` 保存核心 JSON 转换、99 个原样扩展页及完整原始通知，manifest 记录来源及逐文件哈希。新增转换工具验证核心及通知哈希并重新审核扩展页；资源不改标 MIT。`StrokeFont::bundled_core()` 已在无外部目录测试中加载 303 个核心字形并验证拉丁/数字/希腊与空格。此前“尚未复制资源”记录为历史时点，现已被本项更新。

这是本地开发资源接入，不是发布许可完成结论；固定 KiCad 修订对应关系、完整发布许可证材料及打包审查仍保留为发布前工作。扩展页尚未编译嵌入或接入产品后台，真实板文字仍未显示。
