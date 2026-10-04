# PCB 画布与最新 Web 的对照验证

> 本文保留历史验证记录，其中引用的辅助脚本及 pomelo-core/import/render 的 example 探针已移除；旧探针命令不再可执行。当前验证使用各 crate 的测试代码。

die pad GPU 显示分类已与 Web etch 对齐：120 组正式 D3D11 硬件场景通过，真实 camera_test_board 全部 29 个 die pad 的 3,480 次拾取首选和四模式对照零差异，显示切换不重传几何。工作区 401 项测试通过、14 项默认忽略，debug/Release 构建通过，UI 字体保持原状。此项尚未完成实窗截图与交互验收，详见[die pad 专项](die-pad-rendering-validation.md)。

日期：2026-10-03。参考目录：`C:/Users/Zen/Desktop/gitrepo/pomelo`；外部案例只读，报告保存在桌面项目中。

## 实现范围

Windows 使用 GPUI 同设备 D3D11/HLSL，业务几何、字体和 shader 属于 `pomelo-render`。已删除工作区直接 `wgpu = "=30.0.1"` 依赖及旧独立三角形实验入口；D3D11 后端和测试不使用 wgpu。GPUI 为其他平台引入的传递依赖不代表 Windows 业务后端使用它。

参考最新 Web 的显示排序和覆盖规则：精确覆盖优先、覆盖对象按最终绘制次序选择、未覆盖候选按距离排序；钻孔归所属 pin/via，铜皮孔洞不参与填充拾取。普通点击选首个候选，显式候选切换命令负责循环。走线、圆弧、解析焊盘、Zone 边界和孔洞保留源空间计算；绘图和板文字使用所属绘图对象。五逻辑像素的拾取容差随缩放变化。走线、绘图、字形及 pin/via/Zone 的空间索引在后台构建。

板文字和自动标签采用最新 Web 的 **Source Han Sans SC MSDF** JSON/PNG、字形度量、布局及采样公式。`text/msdf.rs` 准备共享字形和 f64 拾取边界，`text/msdf/labels.rs` 负责标签可见性、缩放阈值和布局；Windows 资源与编码绘制位于 `backend/d3d11/`，实际 shader 为 `shaders/label.hlsl`。源文字与标签分离，自动标签不作为独立可拾取对象。Zone 标签使用所属 Zone 的 stencil，排除孔洞并保留下方图元。

**UI 界面的字体保持不变。** Web 源码仅作为只读参考，未修改其界面字体或其他源文件。

背钻图元、独立显示开关、切除层范围与 base 恢复已接通。128 组真实 D3D11 像素、AGILEX 650 逻辑像素/mm 下的过孔拾取/键盘切换/重开恢复通过，该阶段工作区 401 项测试通过、13 项默认忽略。552 个背钻的 52,992 次全量拾取首选及四模式对照全部匹配，0 差异；二进制与验证边界见[背钻专项](backdrill-rendering-validation.md)。下表保留此前阶段已完成的对照记录。

## 自动对照结果

| 验证 | 案例/范围 | 结果 |
| --- | --- | --- |
| 拾取首选对象及 object/track/net/component 四模式 | USBC_FPC，1,398 点 × 3 缩放 × 8 显示状态 | 33,552 / 33,552 一致 |
| 同上 | LPDDR4_case，1,414 点 × 3 缩放 × 8 显示状态 | 33,936 / 33,936 一致 |
| 板文字和自动标签布局 | USBC_FPC、48 个合成文字场景、12 个视图 | 636 字形，零差异 |
| 同上 | LPDDR4_case、48 个合成文字场景、12 个视图 | 6,594 字形，零差异 |
| 原生 MSDF 硬件像素 | 英文 G、中文“铜”；旋转、镜像、翻转、DPI 1/2 | 32 组通过 |
| 原生图元硬件回归 | 走线端帽、圆弧、裁剪、缓存；铜皮重叠孔洞及 Zone 标签 stencil | 通过 |
| 自定义焊盘边界几何 | USBC_FPC 12 条、AGILEX 694 条（189 条圆弧），直接对照最新 Web | 零差异 |
| 自定义焊盘轮廓硬件与窗口 | 内外环、圆弧、typed owner、选择、缓存；填充切换与重开恢复 | 通过，见[专项记录](custom-pad-outlines-validation.md) |
| 长直线深度缩放硬件与窗口 | 960 组 f64 像素参考；FPC 50,000 逻辑像素/mm 边界拾取/翻板/平移 | 通过，见[精度记录](long-line-precision-validation.md) |
| 工作区测试 | `--workspace --all-features --locked --offline` | 396 通过，12 个硬件/外部案例测试默认忽略，零失败 |
| 静态检查及构建 | fmt、全目标全特性 Clippy `-D warnings`、Windows 应用构建 | 通过 |

拾取状态覆盖默认、铜皮透明度为零、非填充焊盘、隐藏顶层、底层提升、当前层提升、关闭钻孔、打开板文字。采样点包含网格、线端/中点、焊盘中心、绘图线端及字形中心。

布局对照直接调用最新 Web 的 `MsdfFont`、`BoardTextGlyphBuilder`、`BoardLabelLayout`；读取同一份 BRD，不重写 Web oracle。源文字包含对齐、镜像、旋转、多行、制表符、CJK 和缺字回退；标签覆盖走线、pin、通孔、盲埋孔及 Zone。布局跨 GPU f32 ABI 使用数值容差，不将其表述为逐 bit 相同。

MSDF 硬件测试使用真实 D3D11 设备、原字体图集和 HLSL，按 Web 的双线性采样/中值距离场公式独立计算参考像素，检查最大 RGB 误差 ≤ 4、平均误差 ≤ 0.05；实际最大 RGB 误差为 2，最高平均误差约 0.02096。它不是 WebGPU 与 D3D11 的整板截图比较。

文字拾取还包含旋转、多字形、绘图所属对象及文字开关的回归：宽阶段按整段文字的字形联合边界查询，窄阶段按各字形计算覆盖与距离，保持 Web 在字形间空白附近的候选行为。

## Windows 实窗验证

已在原生窗口打开 USBC_FPC，启用板文字、放大至约 529%，查看 MSDF 板文字和自动标签，并点击走线选择对应网络。原生 GPU 状态为 `GPUI_D3D11`，适配器为 NVIDIA GeForce RTX 5080，使用硬件渲染，资源 ready、已提交和呈现帧、无 GPU 错误。界面字体保持原状。

截图保存于 `.cache/canvas-parity/window-zoom-selection.jpg`。窗口验证使用独立偏好目录和可执行文件副本；该预览副本对应最终文字拾取边界修正之前的构建，最终构建另经测试及拾取对照通过。窗口遥测文件会随操作持续更新，不能将其最新内容固定解释为 USBC_FPC 状态。

上述文字阶段构建 SHA-256 为 `04dd80b9265f6349877f4e67d760b336dffd11ede250456b9ddc1a5e385d1345`，检查日志位于 `.cache/canvas-parity/workspace-final.log`、`clippy-final.log`、`build-final.log`。上述三项硬件测试单独执行通过，日志为 `msdf-gpu.log`、`trace-test.log`、`copper-test.log`。

后续补齐自定义焊盘轮廓及五语“填充焊盘”控件。该阶段二进制 SHA-256 为 `f63e83b131b9f59bd237b19a3a3a6c68f555d5d574802923fb9d88ea951bf5d0`；同一二进制副本已验证真实自定义焊盘边界拾取、Tab/Space 切换及重开恢复，UI 字体保持不变。实现范围、测试日志和冻结窗口证据见[自定义焊盘轮廓记录](custom-pad-outlines-validation.md)。

长直线精度阶段 `target/debug/pomelo.exe` SHA-256 为 `c51f1b7121357613e1a85b2cb7d6b96909420cc2e2e79df057e40d82c20d45be`，包含长直线深度缩放精度修正；同一副本通过真实 FPC 高倍率线边拾取、翻板和平移。该阶段日志和版本指纹见[长线精度记录](long-line-precision-validation.md)；后续背钻构建版本见[背钻专项](backdrill-rendering-validation.md)。UI 界面字体继续保持现状。

原始报告：

- `.cache/canvas-parity/latest-usbc/report.json`
- `.cache/canvas-parity/latest-lpddr4/report.json`
- `.cache/canvas-parity/msdf-usbc/report.json`
- `.cache/canvas-parity/msdf-lpddr4/report.json`
- `.cache/canvas-parity/gpu/d3d11-msdf-sampling.json`

报告记录案例 SHA-256 和参考 Web 文件的 SHA-256，字体资源完整指纹另见 `assets/fonts/source-han-sans/provenance.json`。

## 历史复验入口（旧探针已移除）

```powershell
cargo +stable build -p pomelo-render --example canvas_pick_probe --example msdf_layout_probe --locked --offline
& C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/.bin/tsx.cmd scripts/check-canvas-picking-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo target/debug/examples/canvas_pick_probe.exe E:/brd_cases/USBC_FPC.brd .cache/canvas-parity/latest-usbc
& C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/.bin/tsx.cmd scripts/check-msdf-layout-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo target/debug/examples/msdf_layout_probe.exe E:/brd_cases/USBC_FPC.brd .cache/canvas-parity/msdf-usbc
$env:POMELO_CANVAS_GPU_REPORT_DIR = "$PWD/.cache/canvas-parity/gpu"
cargo +stable test -p pomelo-render --all-features --lib --locked --offline hardware_msdf_atlas -- --ignored
```

## 尚未代表通过的门槛

2026-10-03 定位使用正式索引几何边界，8,840 个真实/合成查询与 Web 一致；按用户确认修正 TOP 列表首行，保持画布绘制和拾取规则。J1 分组的真实退出/重开恢复通过，完整语言、finger-only 和多文档恢复仍待补齐。439 项工作区测试、Clippy、格式、debug/Release 构建通过，UI 字体保持不变。Web 实际定位 bounds 包含隐藏条目，显示状态只影响搜索锚点；范围修正及尚未验证的锚点/排序见[定位记录](selection-navigation-bounds-validation.md)和[图层列表记录](top-layer-order-validation.md)。

2026-10-03 元件 reference 分组的搜索、完整成员、画布选择和悬停集合已与最新 Web 对齐：三块真实 PCB 的 3,842 个组/17,178 个成员零差异，专用 fixture 补验重复 reference、孤立 finger 及类型 ID 冲突；实窗 U2 的搜索、finger 拾取和整组悬停共 57 成员通过。432 项工作区测试、Clippy、格式和 debug/Release 构建通过，UI 字体保持不变。显示相关定位范围、多语查询排序、新组恢复闭环仍待补齐，版本边界见[元件分组验证](component-reference-validation.md)。

2026-10-03 相机与完整板框范围已按最新 Web 对齐：924 个状态及三块原生导入边界零差异，FPC 最终 Release 的 100%→120%、翻板与适应按钮实窗通过。首次静态上传提示不再改变画布高度；UI 界面字体保持原状。相机证据及尚未验证的滚轮映射、多 DPI 等范围见[相机导航验证](camera-navigation-validation.md)。

2026-10-03 曲边铜皮的可见圆弧细分、视口缓存与 D3D11 孔洞遮罩已通过 7,181 次 Web 几何和 216 组硬件场景对照；标签孔洞及每帧共享上传预算另行通过。修正了动态准备状态改变画布高度的重建循环，新版静止呈现稳定，实窗截图与交互仍待补验。UI 界面字体保持不变，详见[曲边铜皮验证](curve-fill-rendering-validation.md)。

以上结果验证指定案例、显示状态和字体场景，尚不能宣布整板渲染与 Web 完全一致。仍需验证：Web 与原生窗口的整板视觉对照及完整交互、全部 BRD 的背钻/特殊层重叠、曲线铜皮的边缘抗锯齿及完整实窗交互、大板动态标签和字体图集上传的帧耗时，以及多 DPI/设备恢复。自定义焊盘的非填充边界及指定直线管线的深度缩放精度已通过专项验证，其余 BRD 的背钻与特殊对象组合仍属于剩余范围。当前字体资源随可执行文件嵌入，PNG 约 55 MiB；按需解码有 128 MiB 上限，图集首轮上传尚未计入每帧几何上传预算。macOS/Linux 未开发、未验证。
