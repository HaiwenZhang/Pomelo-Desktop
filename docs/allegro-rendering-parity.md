# Allegro 案例解析与原生画布对齐记录

> 本文保留历史验证记录，其中引用的辅助脚本及 pomelo-core/import/render 的 example 探针已移除；旧探针命令不再可执行。当前验证使用各 crate 的测试代码。

记录日期：2026-10-04。状态：进行中，尚未完成四板视觉验收。

目标是先完成 `E:/brd_cases` 当前案例的解析，再将下列四板的走线、过孔、焊盘、铺铜、板文字、网络标注、透明度以及跨层/同层叠加与 Allegro 的实际画面对齐。发现的差异必须修复并复验，不能以整理文档或导入成功代替画面验收。本记录同时给后续 Web Pomelo 实现提供可追溯的行为依据。

当前只开发 Windows D3D11/HLSL，业务渲染留在 `pomelo-render`，不引入 wgpu。共享显示与几何契约保持平台无关；新增用户文案同步英、简中、繁中、日、韩。UI 界面字体和 Web 源码保持原状。

**2026-10-04 用户确认**：保留当前 PCB MSDF 字体，不再继续推进字体替换；ANSI 实验及已经修改的代码保留，不撤回。显式源字体入口保持实验性质，正常启动不启用。后续对齐主线继续处理几何、透明度、同层/跨层叠加与标签显示；ANSI 完整字体一致性不作为当前必须完成的修复项。

**当前窗口控制约束**：用户正在手动操作校准窗口，要求暂不控制窗口。本阶段不进行自动点击、键盘输入、关闭或重启，也不为补取证切换当前画面；继续源码检查与无窗口的 CPU/D3D11 离屏验证。用户手动设置不作为受控对照样本，不擅自恢复测试参数。

最新复验见 [RDIMM 实际对象与 GPU 准备](#rdimm-实际对象与-gpu-准备2026-10-04)、[NVL 受控选择与修复](#nvl-受控选择与修复2026-10-04)、[实窗高亮与退出保存](#实窗高亮与退出保存复验2026-10-04) 和 [NVL R4D1](#nvl-r4d1-实际对象与配置校准2026-10-04)。此前各阶段的“待实窗/待修复”描述保留其历史条件，以最新证据补充，不将局部通过扩大为四板整体通过。

**同日目标更新**：用户确认几何图形的颜色无需与 Allegro 一致。因此不以 RGB、调色板或来源颜色自动读取作为当前验收门槛；已有颜色覆盖功能和历史测量保留。可临时使用不同类别颜色辨识覆盖关系，但不能据此要求产品默认颜色照搬 Allegro。几何尺寸、孔洞、透明度及同层/跨层叠加仍按实际画面核对。

## Trace 端帽与透明合成探索（2026-10-04）

状态：候选方案记录，尚未选定实现，也未完成 Allegro 受控对照。此次用户授权查看 Allegro，但不代表后续可持续控制校准窗口。

### 观察与官方文档依据

在 Allegro 25.1 的 S5000C 当前视图中，Global transparency 滑块初始显示 62%；几条绿色走线的 45° 衔接处颜色看起来均匀，没有明显的加深圆斑，蛇形走线外侧拐角呈圆滑轮廓。这是局部肉眼观察，未锁定对象、测量像素或完成透明度对照，不能推导出所有同层重叠都不加深。操作中滑块曾显示 50%，控制工具随后报告 `Computer Use helper already has an active request`，未确认最终恢复值；此过程不作为受控验收证据。用户随后确认此前“有时能看到圆形端帽”的判断看错了，不把该说法作为待复现缺陷。

本机官方文档提供以下依据：

- `C:/Cadence/SPB_25.1/doc/algrodesignparam/Display_Parameters.html` 的 **Connect line endcaps**：控制屏幕显示，将线段顶点画圆，使其更接近 artwork。圆滑几何轮廓与透明叠加导致的内部圆斑必须分开判断。
- `C:/Cadence/SPB_25.1/doc/algroenvvar/Display_Settings.html` 的 **disable_gpu**：禁用 GPU Plugin 后默认回退常规 OpenGL，除非同时禁用 OpenGL；该文档还描述 GPU Scene Graph 刷新与 OpenGL 平移缓存。存在多条渲染路径，未确认此次窗口实际使用哪条路径。
- 同一文档的 **draw_etch_outline** 与连接端帽增强显示共同控制宽线轮廓；**display_etch_over_pad** 可改变每层 Trace 与 Pad 的绘制顺序。因此不能预设全层所有对象都采用同一覆盖合并规则。

这些资料确认显示能力与选项，没有公开确认离屏目标、Stencil、深度去重、三角化或 SDF 的具体内部实现。

### 当前 Pomelo 的原因

`crates/pomelo-render/src/shaders/trace.hlsl` 对每个线段独立计算胶囊 SDF，端点投影参数限制在 `[0, 1]`，产生圆形端帽；圆弧也有端点距离处理。输出 alpha 为材质不透明度乘 coverage。`crates/pomelo-render/src/backend/d3d11/trace_d3d11.rs` 使用 `SRC_ALPHA / INV_SRC_ALPHA` 的 source-over 混合。

相邻段共用端点时，覆盖区可能重复混合。内部完全覆盖且单段不透明度为 a 时，两次覆盖的等效不透明度为 `1 - (1 - a)^2`；a=0.5 时得到 0.75。这解释 Pomelo 的连接圆斑，但不能据此反推 Allegro 的内部技术。预乘 alpha 本身仍会累积覆盖；直接删除连接圆帽也可能导致转角缺口或抗锯齿接缝。

### 候选实现

| 候选 | 去除重复加深的方法 | 适用范围与限制 |
| --- | --- | --- |
| 连续 Cline 轮廓与三角化 | 相邻段共享连接几何，内部不重复绘制 | 能解释连续路径的均匀连接；独立对象、分叉、变宽及圆弧连接需要额外处理，不能单独证明任意同层交叠去重 |
| 离屏覆盖或颜色缓冲，随后统一合成 | 同组图元先生成覆盖图，再应用一次材质不透明度 | 可保留解析 SDF；分组须遵守颜色、图层、对象类别及绘制顺序，不能直接扩展成全画面统一淡化 |
| Stencil 或深度控制重复覆盖 | 同组已覆盖的像素或 MSAA 采样点不再次混合 | 技术上可行；简单逐像素首次覆盖规则可能使抗锯齿边缘依赖绘制顺序，需要定义采样与分组策略 |

上述技术可以组合使用。当前画面不足以区分它们，也没有证据证明 Allegro 一定按整层合成。

### Pomelo 建议探索方向

优先在离屏实验中保留现有直线／圆弧 SDF，将覆盖生成与最终透明合成分开：同一允许合并的材质组生成 coverage mask，再以组颜色和不透明度合成一次。`MAX(coverage)` 可作为同色覆盖合并的原型，但它不一定等于精确几何并集的像素覆盖率，尤其在亚像素接缝处；需要与连续几何或逐采样并集对照。

分组边界先按受控证据确定，不能擅自按整层或整网合并。不同颜色、选中／高亮、Trace／Pad／Via／Shape 以及跨层关系必须保持明确的优先级。铜皮填充独立不透明度与现有全局不透明度契约保持有效；本方案不覆盖此前已验证的显示语义。GPU 目标复用、视口尺寸／DPI、资源重建与大板额外绘制成本纳入实验评估。

### 区分方案的对照与验收

1. 固定版本、渲染路径、端帽选项、背景、倍率、图层与不透明度，记录源对象身份；先比较同一连续 Cline 的直线、45°／90° 拐角、圆弧连接、变宽及端点。
2. 比较两个独立同层同色 Trace 的交叉与端点重叠，再比较不同网络／不同颜色对象。若只有连续 Cline 均匀，更支持路径几何处理；若独立对象重叠也均匀，则更支持按组覆盖合并或去重。
3. 比较 Trace 与 Pad／Via／Shape 的同层重叠，并控制绘制顺序；另测不同层交叉，确认跨层正常透明叠色。
4. 在低、中、高不透明度以及不同倍率／DPI 下检查内部一致性和边缘接缝；高亮单段、整网及恢复普通显示另行对照。
5. 对选定 Pomelo 原型做硬件离屏像素验证，覆盖顺序反转、直线／圆弧、分叉、亚像素连接与资源重建；再完成同一局部实窗对照及大板性能测量。

本节只记录研究方向，不声明实现、测试通过或 Allegro 内部机制已确认。

## 基准与案例顺序

- 桌面工作区：`C:/Users/Zen/Desktop/gitrepo/Pomelo-Desktop`，本轮开始时 HEAD 为 `38d2572`；后续修改另列验证版本，不能将初始二进制视作新版验证。
- Web 参考：`C:/Users/Zen/Desktop/gitrepo/pomelo`。本轮不修改 Web；算法和显示策略的移植要求记录在本文。
- 官方画面：本机已安装的 `C:/Cadence/SPB_25.1/tools/bin/allegro.exe`，25.1-2025-P001。实际打开案例并读取参数；未保存或转换原始 BRD。
- 比较顺序按当前文件大小：DemoCase → NVL CAMM → DDR5 RDIMM ARM → S5000C。大小只用于选择工作顺序，不代表几何复杂度。

| 案例 | 字节数 | 布局族 | 层 | 网络 | 元件 | 走线段 | 过孔 | 引脚 | 铜皮 | 板文字 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `DemoCase_LPDDR4.brd` | 6,952,428 | 172 | 8 | 489 | 347 | 7,998 | 1,117 | 1,726 | 72 | 3,504 |
| `874140_NVL_H_LP5X_CAMM_T3_Rev_0p7_23p1.brd` | 230,723,032 | 180 | 18 | 4,707 | 5,727 | 149,767 | 13,419 | 19,623 | 1,647 | 23,439 |
| `DDR5_RDIMM_ARM.brd` | 288,571,884 | 175 | 18 | 5,581 | 4,859 | 277,334 | 24,797 | 26,912 | 13,174 | 18,285 |
| `S5000C-64_DDR5_BGA_V0.61.brd` | 303,867,712 | 174 | 14 | 7,233 | 6,909 | 321,364 | 31,318 | 35,466 | 1,017 | 36,831 |

这些是正式导入器输出计数，尚未通过 Allegro 对象数量逐项核验。

## 当前全案例 CPU 导入证据

当前目录为 **157 个 BRD**，与旧 156 案例清单不同。新增 NVL CAMM、DDR5 RDIMM ARM，旧 `S5000C-64_DDR5.brd` 不在当前清单中；`S5000C-64_DDR5_BGA_V0.61.brd` 原清单已有，不能把它解释成新旧文件的改名。

完整正式 `AllegroImporter.import()` 已对当前冻结清单串行执行，每板一个独立进程：**157 成功、0 失败，场景导入诊断为 0**。此轮统一显式指定 Windows-1252 作为兼容基线；它不代表原始文本的实际编码已经逐板确认。不能因为该编码能解码任意字节，就认定中文、日文等源文字正确。

证据保存于 `.cache/full-import-20261004-v2/`：

- `manifest.jsonl`：每板路径、字节、SHA-256、编码、退出码、结果报告。
- `provenance.json`：实际探针与清单指纹。
- `summary.json`：157/157，明确 `scene_validated=false`、`gpu_validated=false`。
- 每板 `.brd.json`：正式场景计数、边界、源身份与完整诊断。
- 冻结编码清单 `.cache/allegro-parity/current-cases.jsonl` SHA-256：`ec82803f46b5de49d843ed968aea9e9eb2f4a8954c15ca8636491f09161b810d`。
- 探针 `target/release/examples/import_case_probe.exe` SHA-256：`5dc81a0ea1192320bc748cb53ddeb92357e5303e19232713235afc789ce21a51`。

初次脚本错误地将导入器返回的 32 字节哈希数组当成十六进制字符串，导致校验提前停止；这是适配脚本错误，已改为字节数组转 hex。旧不完整输出 `.cache/full-import-20261004/` 保留，最终证据只引用 `-v2`。本轮耗时为单次诊断数据，不作为三轮性能验收。

复验时使用新的输出目录，防止旧成功文件掩盖失败：

```powershell
cargo +stable build -p pomelo-import --example import_case_probe --release --locked --offline
python scripts/check-full-import.py E:/brd_cases target/release/examples/import_case_probe.exe .cache/allegro-parity/current-cases.jsonl .cache/full-import-new
```

## DemoCase 的实际显示基准

已在 Allegro 和 Pomelo 私有比较实例中打开同一原始 DemoCase。Allegro 初次打开包含 FAB/图框视图，不能直接与 Pomelo 默认全铜层画面比较。本轮先全局关闭图层，再启用 TOP 的 Etch/Pin/Via，单层核对。

Allegro Color Dialog 与 Design Parameter Editor 实际读取结果：

| 参数 | Allegro 当前比较设置 | Pomelo 初始比较设置 | 处理 |
| --- | --- | --- | --- |
| 全局不透明度 | 255/255，100% | 图元默认不透明 | 仍需逐类别混合核对 |
| 铜皮不透明度 | 99/255，UI 四舍五入显示 39% | 25% | 统一设置后再比较；39% 不是所有案例的固定默认值 |
| TOP Etch/Pin/Via 色 | 绿色；按原始颜色字的低字节推定关联色号 57，导出 RGB 为 38/255/38 | 固定 `#58b5ed` | 颜色字映射还需受控改色复验，尚需颜色读取/覆盖实现 |
| 背景 | 黑色 | 当前主题的画布背景 | 差异已记录，待修正画布外观契约 |
| 填充焊盘 | 关闭 | 开启 | 原生现有开关可切换到轮廓；比较实例已关闭 |
| 走线端帽 | 开启 | 已有圆端帽 | 待同一倍率局部验证 |
| 板上源文字 | TOP Etch 中可见 `i.MX 8M MINI` | 初始关闭板文字 | 启用现有板文字开关后已出现；字体/笔宽还须单独比较 |
| 嵌入网络名称 | Clines/Shapes/Pins 开启，Vias 关闭 | 对应标签默认开启/关闭 | 排布、尺寸、旋转和 LOD 尚未整体对齐 |
| 通孔标签 | 开启；盲埋孔标签关闭 | 两者默认开启 | 待提供完整标签设置并验证 |
| GND 焊盘网络名 | 水平可读 | 随部分焊盘旋转成竖排 | 见差异 A05 |

原始参数导出：`.cache/allegro-parity/DemoCase-LPDDR4.prm`。通过 Allegro 自身的 Export Allegro Parameters 生成并显示写入完成；不是脚本推测的默认配置。`scripts/extract-allegro-appearance.py` 保存调色板、class/subclass 原始字、显示 flags、文字尺寸及文件指纹；不猜测尚未证实的位标志，不将 artwork 的 `draw_pad` 当作视口填充开关，也不把导出文件当作已包含透明度。

参数文件 SHA-256：`19edf033cc7cc6c7f49fc5af25d9308d29f55e59ad958a205e9083c762f25ca8`。规范化输出 `.cache/allegro-parity/DemoCase-appearance.json` 保留全部 257 条颜色记录，包括重复编号；不以最后一个重复记录覆盖前一个。实际窗口可访问性文本保存于 `.cache/allegro-parity/display-observations.json`。

```powershell
python scripts/extract-allegro-appearance.py .cache/allegro-parity/DemoCase-LPDDR4.prm E:/brd_cases/DemoCase_LPDDR4.brd .cache/allegro-parity/DemoCase-appearance.json
```

## 差异与修复台账

状态分为“已确认差异”“配置已统一”“代码已实现待复验”“已修复并验证”。肉眼看起来接近、CPU 导入成功和专项测试通过都不能直接升级为四板画面通过。

| ID | 对象/行为 | 当前证据与原因 | 桌面处理 | Web 后续要求 | 状态 |
| --- | --- | --- | --- | --- | --- |
| A01 | 图层与类别颜色 | 原生与 Allegro 调色板不同；用户确认不要求匹配颜色 | 已有 Etch/Pin/Via 覆盖功能保留；不继续为颜色一致性开发来源颜色读取 | 如后续移植设置功能，复用类别材质与保存契约；不要求照搬 Allegro RGB | 类别改色及保存已验证；颜色一致性从本轮验收移除 |
| A02 | 焊盘填充 | Allegro 关闭 Filled pads，原生默认开启 | 使用现有填充开关统一；还须局部检查环、内孔及轮廓宽度 | 同一填充状态下比较，避免把配置差异当作数据丢失 | 配置已统一；几何待验 |
| A03 | 铜皮和全局透明度 | Allegro 当前 shape=99/255；受控 global=255/128，铜皮内部不随 global 减半 | 新增共享 global_opacity、0–255 精确输入及普通图元/标签 alpha；铜皮填充独立 | 同步两个保存字段、精确量化、每图元 source-over 和标签常量；不能使用全画面淡化或 global×shape | 独立透明度、硬件及 Release 输入/重开恢复通过；实际同层/跨层完整混合待验 |
| A04 | 源板文字 | 实际报告确认 ANSI、38×50 mil、10 mil 笔宽，导入字段一致；MSDF 使用不同字形 | 显式 ANSI 供应器及工作台实验入口保留；默认 MSDF/UI 字体保持 | 如未来重新启用，源笔画字体与自动网络名独立，保留源间距及笔宽 | CPU/硬件实验及应用接线已实现；按用户要求暂不继续，未声明完整字体对齐 |
| A05 | 焊盘网络名方向 | DemoCase 的 GND：Allegro 水平，原生竖排 | 新增共享 `BoardDisplay.horizontal_pin_names` 与五语显示开关，水平时重新按旋转焊盘尺寸适配字号，标签索引同时纳入该模式的最小显示尺度 | 增加同一字段、标签布局及保存契约；默认关闭保留原 Web 行为，Allegro 比较配置开启 | 方向修复、翻板与保存恢复实窗通过；字号/字形整体仍待验 |
| A06 | 同层叠加 | 当前颜色一致不足以判断 pin/via/trace/shape 谁遮挡谁 | 选取或构造实际重叠区域，暂时给类别不同颜色，在不同透明度下检查；记录源对象及像素覆盖 | 复用相同对象、顺序、遮罩和混合预期 | 待验证，未作完成声明 |
| A07 | 跨层与特殊对象叠加 | 单 TOP 不能证明多层、背钻、孔及标签组合 | 同层通过后逐步启用内层/BOTTOM，验证实际优先级和孔的独立覆盖 | 同步绘制/拾取显示顺序与钻孔 scope | 待验证 |
| A08 | 标签开关与 LOD | 通孔/盲埋孔配置不同，原生原先传入固定默认 LabelOptions | 共享核心接管 LabelOptions；六类开关接入五语面板与保存，视口使用文档选项 | 在同一开关/倍率条件下对齐排布，不以关闭所有标签通过；共享字段见下文 | 代码、484 项测试及通孔/焊盘分类与恢复实窗通过；各类别完整视觉/LOD 仍待验 |
| A09 | 背景与颜色空间 | 当前主题画布背景与 Allegro 黑色不同 | 画布背景作为独立显示参数；核验 D3D11 输出与混合，不改 UI 主题字体 | 明确背景和直通/预乘 alpha 契约 | 独立背景颜色、单项恢复及保存恢复实窗通过；颜色空间与完整组合仍待验 |
| A10 | 走线内部与轮廓 | 实际放大检查 TOP 的 CSI_DP2 / CSI_DN2：两条独立实心走线，各宽 3 mil、中心距 9 mil；原“双边轮廓”判断在此区域不成立 | 两条源段坐标、长度、宽度与原生场景一致，实际鼠标分别命中对应 ID；继续核对其他走线、圆弧、端帽与抗锯齿 | 保留独立实心走线，不因低倍率双线观感新增空心模式，不将 Filled pads 绑定到走线 | 该局部疑似差异已排除；整体走线验收未完成 |
| A11 | 静态铜皮显示图案 | Demo TOP GND zone `2458525349` 默认点阵，实心偏好切换可复现；已确认 16×16 逻辑像素图案 | 共享保守身份、五语实心选项、D3D11 点阵与 DPI 修正已实现，物理孔洞/标签遮罩保持 | 移植身份、显示字段和逻辑像素图案；不将点阵解析为物理 hatch，不改变拾取/连通 | CPU/硬件及新版实窗切换、空白处拾取、重开恢复通过；局部横向周期通过，完整相位及其他 DPI 实窗待验 |
| A12 | 零全局不透明度的高亮 | Demo 实心 Static 为连续边界；NVL 点阵 Static 仅原点变白，Dynamic 为密点及原材质间隙的二次合成；Net 为大菱形 | 已实现按已知 ZoneKind 与选择模式区分填充，统一独立 shape alpha；Unknown 保持保守边界 | 分开 Object/Net、物理/逻辑位图；Dynamic 不能仅画白点；不在 global=0 一律跳过高亮 | 25 项硬件回归及 NVL 两对象实窗通过；精确相位、其他实际 DPI、完整叠加及四板仍待验 |
| A13 | 独立标签透明度 | Allegro global=0、shape=99，通孔跨层标签和铜皮名仍可见；旧原生 HLSL 未解码独立位 | 仅 label.hlsl 按 glyph 元数据区分独立标签和普通网络名，字体/图集/布局保持 | 最新 Web 已含该独立位，后续保持语义，不对混合 Drill batch 统一乘 global | 旧实现硬件失败已复现；修复后 global=0/128/255 硬件通过，Demo Base 零 global 实窗保留 span/铜皮名；选中网络的大铜皮名行为仍待核对 |
| A14 | 大板几何准备限额与单位 | RDIMM 19,702,276 顶点超过旧 16M 限额，错误却显示 B；索引及字节也超过旧默认 | 有界默认改为 32M/96M/1GiB，数量与字节五语诊断分开 | Web 如有准备预算，同样区分计数与字节，不把 CPU import 等同 GPU ready | 四板默认准备、RDIMM 实窗上传与局部拾取通过 |
| A15 | Dynamic Base 边线 | RDIMM 正常填充时原生额外亮边；实际低 shape 仍有极细线 | 可靠 Dynamic 在 raw shape≤25 时保留物理细线，≥26 时取消独立亮边，保持 Static/Unknown/选择策略 | 同样区分可靠 kind、Base pass、25/26 阈值与独立 global；不能全删线 | 策略、局部实窗填充/阈值/拾取及缓存通过；AA 相位/覆盖仍待验 |

## RDIMM 实际对象与 GPU 准备（2026-10-04）

实际 Allegro 打开 `DDR5_RDIMM_ARM.brd`，只在内存升级，不保存源板。在 TOP 的 L83 / C2324 / SB_3V3 局部，由 Show Element 导出六份真实报告；不是用解析器代写的参考输出。源文件 SHA-256 为 `430e959d2d90f61c9d78eef907bf9fa88fc80ff0bb7752edf67c09cea7fc6788`，Windows-1252 仅作为本轮解析编码。

| 实际对象 | 源场景与 Allegro 报告核对 |
| --- | --- |
| L83 | 原点 `(-124.5,283.25) mm`，90°，不镜像 |
| Dynamic zone 2020223 | ETCH/TOP、SB_3V3；12 边、5 弧、0 void；面积 `0.056207430968181 cm²` 符合实际打印 `0.0562` 的精度 |
| Via 636119 / 636135 | 原点 `(-125.75,284.75)` / `(-124.5,284.75) mm`；0.100 mm 镀孔，TOP→BOTTOM，0°，不镜像；每个连接一条 TOP 走线 |
| Segment 578156 / 578179 | 均宽 0.300 mm；分别从 `(-125.75,283.9)` / `(-124.5,283.93)` 到对应过孔，长度 0.85000 / 0.82000 mm；SB_3V3、ETCH/TOP |

独立核验合计 **53 项匹配、0 差异**。弧中心/半径最大误差 `3.3333e-5 mm`，符合报告三位小数精度；走线端点误差小于 `6e-14 mm`。同网几何包含 2 pins / 4 vias 与实际连接摘要相符，但不将空间包含视为独立连接链解析证明。Via 的 Tool Size 0.2 mm 单独记录，不能替代实际 Drill 0.1 mm。实际报告未提供两处 Via 的 pad 宽高，4 项保持未验证；各 pin 的实际 pad 尺寸/旋转和 C2324 独立报告也未验收。原 BOUNDARY 的四边矩形面积仅作报告自洽校验，不冒充源 BOUNDARY 独立解析通过。

原始证据为 `.cache/allegro-parity/rdimm-{l83,zone2020223,via636119,via636135,segment578156,segment578179}-allegro.txt` 及对应报告截图；核验入口为 `geometry-candidates/verify_rdimm_actual.py`、`RDIMM-actual-reports-verification.json` 和 `RDIMM-zone2020223-connected-counts.json`。

### 大板准备预算修复与实窗

首次原生窗口准备失败，旧错误显示“19,702,276 B 超过 16,000,000 B”。实际单位是铜皮顶点数量，不是字节：CPU 导入创建 BoardScene，尚未进入 PreparedCopper，故此前 157/157 导入成功不能覆盖这一失败。RDIMM 同时超过旧 48M 索引和 512 MiB 输出限额，不能只提高第一个限额。

共享有界默认改为 **32M 顶点、96M 索引、250k 铜皮、1 GiB 准备输出**，保留分配前检查、u32 索引界、取消、分配失败处理及每帧 4 MiB 共享上传预算。数量错误独立为 `RENDER_PREPARE_COUNT_LIMIT` / `render.geometry_count_limit`，五语按元素数量说明；字节错误继续显示 B。1 GiB 仅限制准备输出，不是整个 CPU 工作集或 VRAM 上限。

| 板 | 铜皮顶点 | 索引 | 准备输出 B | GPU 上传数据 B |
| --- | ---: | ---: | ---: | ---: |
| DemoCase | 255,746 | 756,354 | 7,125,416 | 7,117,352 |
| NVL CAMM | 9,123,509 | 26,882,799 | 253,691,804 | 253,507,340 |
| RDIMM | 19,702,276 | 57,823,866 | 548,007,368 | 546,531,880 |
| S5000C | 13,383,557 | 39,326,247 | 371,555,804 | 371,441,900 |

四板均通过生产默认的 tracks / drawings / zone outlines / copper / pads / custom pads / drills / search / pick 准备。独立 RDIMM 进程以 100 ms 采样的 CPU 工作集峰值约 1.66 GiB；这个探针不运行字体或分配 GPU。core+render **256 passed、29 ignored**，其中 render 98 passed；pomelo+render 全目标全特性 Clippy `-D warnings`、格式和 Debug 构建通过，未重跑整个工作区。详见 `desktop-copper-budget-verification.json`、四份 `*-default-prepare.json` 与 `desktop-copper-budget-{tests,clippy}.log`。

随后实际启动独立 Debug 副本（SHA-256 `e08337af4ef6854669011d2b54ad3da1d8e75b4135e91e533f5d3b108b7c0c64`，启动信息 `rdimm-budget-interactive-launch.json`），RDIMM 成功显示，RTX 5080 硬件 D3D11 完成全部 546,531,880 B 铜皮上传。真实点击分别命中 Via 636119、Segment 578156、Pin 434209、Zone 2020223；每次对象身份、成员及 GPU 状态独立存 `rdimm-native-*-pick-runtime.json`，相应截图存 `rdimm-native-*-pick.jpg`。稳定几何上传不增加，ANSI 上传为 0，当前 MSDF 保留。取证汇总 `rdimm-native-observations-verification.json` 及可复跑的 `verify-rdimm-native-observations.py` 验证原件/二进制哈希、四次实际命中与两档无选择/无悬停透明度配置；它不替代像素分析。实际成功范围是该原生实例及局部视口，不能扩大为另外三板的新预算 GPU 验收或整板视觉通过。

### 同层透明度与 Dynamic 边线修复

当前产品策略已按用户要求改为：Zone 边框在所有 shapes alpha 值及填充模式下保持显示。以下 alpha25/26 隐藏阈值仅记录此前的 Allegro 对齐取证，不再作为当前显示规则。 Dynamic 铜皮的单对象点击高亮改为走线式白色轮廓及逻辑像素周期 5 的稀疏白点（alpha 0.9），保留原色填充与透明度，点阵间隙不再额外叠加原色。Static 对象点击和网络高亮保留此前规则。

实际 Allegro 在无选择的相同视角，以 shape=99、global=255/128/0 逐档设置并 F5 后取稳定图。原生保持相同开关：普通过孔网络名开启、通孔跨层标签关闭；这是本板实际比较配置，不能因此改变其他板默认标签。字体、图集、布局未改。

铜皮内部绿通道保持约 99；一次普通图元覆盖约 177，走线和过孔重复覆盖约 216，符合逐对象 source-over。global=0 时铜皮仍完整填到这两个真实钻孔中心：这不是 zone void，普通 Drill 随 global 消失，不新增物理钻孔 stencil 剪切。原生三档干净内点得到相同的归一化响应；颜色不同不作差异。该证据覆盖此局部填充、单次/重复混合和零 global 行为，不证明全部类别顺序、抗锯齿、多层或标签尺寸一致。

**差异与规则**：原先 Dynamic zone 2020223 Base 的独立全强度边线造成较宽亮峰。实际 Allegro 受控采样 shape=0/1/8/16/24/25 时保留极细边线；shape=26/32/99 时没有对应额外亮峰。shape=25 的内点为 25、顶边峰约 190.5；shape=26 的内点为 26、顶边峰约 27，确认原始 8 位值 25/26 的切换。global=0、shape=25 并 F5 后边线仍存在，说明该低值边线独立于 global。不能一律删除轮廓，也不能仅以 `alpha == 0` 决定边线。下面的生产修复及局部实窗证据已补齐；Static、Unknown 和 Object/Net/hover 边界保留各自已经验证的行为。当前视口曲线细化未启用（runtime curve=0），该亮边不是曲线细化或 gamma 路径导致。

实窗图为 `rdimm-{allegro,native}-global{255,128,0}-shape99-*.jpg`；附加阈值图为 `rdimm-allegro-global255-shape{0,1,8,16,24,25,26,32}-idle.jpg` 及 `rdimm-allegro-global0-shape25-idle.jpg`，设置值独立保存 `*-settings.json`。像素分析及可复跑脚本为 `rdimm-allegro-alpha-compositor-analysis.json`、`rdimm-native-alpha-boundary-analysis.json`、`rdimm-low-alpha-outline-analysis.json` 和 `measure-rdimm-compositor.py`。实验后实际窗口已恢复 global=255、shape=99；BRD SHA-256 仍与原件一致。后续 Web 应保留独立 shape、逐对象叠加和普通 Drill 的 global 语义；可靠 Dynamic 的 Base 边线按 `shape_alpha <= 25/255` 保留独立细线，更高值不另画亮边。该规则来自本板实际采样，最新 Web 及本机官方文档未提供阈值；一物理像素的原生细线实现仍须实际覆盖复验，不能声明所有缩放/DPI 完全一致。

global 独立性另经同一干净顶边 ROI 定量：shape=25 时 global=0/255 的内点均为 25，绿通道边峰均为 190，逐行中位数曲线最大差为 0。结果 `rdimm-outline-global-verification.json` 与脚本 `verify-rdimm-outline-global.py`（使用含 Pillow 的 bundled Python）只覆盖此边/倍率/DPI，不能扩大为全对象栅格一致。

**边线修复与测试**：可靠 Dynamic 的边界实例保存 kind 位，D3D11 Base 常量按原始 shape 阈值控制细线；低值线宽固定一物理像素，高值不另画全强度边。复用已有实例/常量布局，不修改字体和 ANSI。kind 临时索引纳入字节预算，准备及标记过程继续支持取消。28 项相关硬件回归全部通过，其中新增三项验证阈值/global 独立/填充及真孔保留、四 DPI 物理线宽、Object/Hover/Net 边界保留，切换无铜皮/边界重传。core+render 258 passed、32 ignored（render 100 passed），pomelo+render 全 targets/features/locked Clippy `-D warnings`、格式检查与 Debug 构建通过。日志分别为 `rdimm-dynamic-outline-{hardware,targeted-hardware,cpu,clippy}-final.log` 和 `rdimm-dynamic-outline-debug-build.log`；首次高亮硬件失败另存 `-hardware-first-failure.log`，其采样点落在旧高亮边覆盖内，修正测试 ROI 后通过，未改生产高亮策略。

**新版真实窗口**：二进制 SHA-256 `466c84a076ceb7cc047497f46a9186ad472f0795c18fc610f923bc7a89427749`，独立设置、125 logical px/mm、DPI 1.5。实际切换 shape=25/26/99 后，纯铜皮内点 G 中位数分别为 25/26/99；25 保留细边，26/99 无独立亮峰。每档无选择/无悬停、GPU ready 且无错误，546,531,880 B 铜皮与 35,499,904 B 走线上传保持，不因阈值切换重传。实际点击正确命中 Zone 2020223，选择图仍有连续边界；截图同时有目标悬停，不能仅凭该图分离 Object 与 hover，独立策略由硬件专项证明。随后清除选择/悬停并恢复 99。证据为 `rdimm-native-outline-fixed-shape*.{jpg,json}`、`rdimm-outline-window-verification.json`、`verify-rdimm-outline-window.py` 与 `rdimm-native-outline-fixed-analysis.json`，原 BRD 哈希不变。

**仍未验收的 AA**：归一化截图覆盖积分（原生/Allegro）顶边约 1.013/1.006，峰值 119.5/190.5 不能直接认定为线透明度错误；右边约 1.041/0.125 仍有明显差异。仅将私有 camera Y 从 284.15 改为 284.1526666666667（名义移动半物理像素），相同二进制的顶峰变为 200、底峰变为 193，说明相位对亮峰分布影响很大；该额外相位尚缺 shape=26 配对参考，不能宣称 AA 完全一致。对应 `rdimm-native-phaseY-shape25*.jpg` / runtime 及 `rdimm-phase-interactive-launch.json`。一度怀疑聚焦输入框会清掉细线，后经三图整个画布 656,667 RGB 像素逐值完全一致排除，审查为 `rdimm-focus-outline-audit.json`；没有据此增加生产遥测或修改 GPUI。用户随后确认正在手动操作窗口，自动窗口输入已停止；手动改为 shape=5 不算自动控制的受控采样，当前校准实例未声明恢复 99。

### RDIMM 右侧边线的离线复核（2026-10-04）

在用户要求暂不控制窗口的约束下，本轮只读取已有源几何和保存的 JPEG，未改 Shader、字体或 ANSI。原 RDIMM SHA-256 仍为 `430e959d2d90f61c9d78eef907bf9fa88fc80ff0bb7752edf67c09cea7fc6788`。新增证据缩小了原因范围，右边线差异仍未修复和验收。

- **邻接核查**：TOP 的 2,323 个 Zone 中，目标 bbox 外扩 1 mm 仅筛得 Dynamic 2020223（SB_3V3）、2020204（DHUB3V3_F）与 2538360（GND），没有同网邻接或局部 Static。GND 的真实闭合 void ring 1946 包围目标；六处边见证 × 六偏移共 36 个点位确认目标边及 ±0.05 mm 带不被其他 Zone 覆盖，四边所测净距均为 0.202 mm，外移 0.25 mm 才进入 GND 填充。该源几何不支持“同网内部边界被合并隐藏”的解释，仅核对 Zone，不推断所有类别遮挡。
- **ROI 核查**：旧右 ROI 对应源直边 2020227，`x=-124 mm`、`y=285.705..283.545 mm`。扩展后的两应用各 160 行均通过内侧 shape99≈99、外侧≈0 的平台筛选，覆盖完整亮峰；原生/Allegro 的逐行归一化积分中位数仍约 1.041/0.126。改用均值/中位数及 shape26/99 目的颜色参考后，范围分别约 1.021–1.042 / 0.121–0.129。未发现简单取错边、漏峰或单个文字污染足以解释此差异。
- **global 核查**：Allegro 同一右 ROI、shape25、global=0/255 的 400 个 RGB 像素中 329 个完全相同，最大通道差为 7；绿色边界峰都为 28。这一新 ROI 不能沿用此前顶边等 ROI 的“全部像素相等”结论，但未显示主导的 global 遮挡影响。
- **注册限制**：shape99 半填充边缘拟合的左右平移差约 0.389 个截图像素，上下差约 0.520；JPEG、捕获缩放和未知重采样使物理像素相位仍不唯一。截图积分不等于实际 framebuffer 线宽，不按 DPI 直接换算后修改 AA 参数。

输入指纹与核查结果见 `geometry-candidates/RDIMM-zone2020223-neighbor-geometry.{json,md}`、`rdimm-right-roi-registration-audit.json`、`rdimm-right-roi-audit-crops.png`，复跑脚本为 `geometry-candidates/rdimm_zone2020223_neighbors.py` 与 `audit-rdimm-right-roi.py`；根代理再次核对原板、输入哈希及点位/图像范围的七项审查全部通过，见 `rdimm-right-edge-combined-audit.json`。当前案例目录仍为 157 个，路径/大小与冻结清单一致，元数据审查见 `current-inventory-metadata-audit.json`；它没有重新哈希或导入全部案例。

**下一项必要取证**：恢复窗口控制后先重新观察用户当前状态，确认捕获的物理尺寸与坐标映射，采集受控 X 相位的 shape25/26 配对；此前 Y 相位仅有 shape25，不能补足右边的 X 相位证据。是否修改 AA 须据新取证判断。已有 Windows 25/26 阈值修复保留；Web 移植暂保持相同策略与几何，不从这些 JPEG 数据推导方向相关的任意强度系数。

### 下一板 S5000C 的真实孔洞目标（仅候选）

U94 区域的 TOP Dynamic zone 5598600 有一个围绕 U94.41（Pin 901969）的闭合圆角孔洞；Static zone 933674 不覆盖该孔中心。这里还包含 GND Via 1332220 / 1656947、Pin 834394（U94.40）、TOP Segment 4242052 / 4241766，可区分真实 zone void、普通钻孔及 static/dynamic 重叠。源板坐标以 mil 记录；原生详情相机建议 `(16.98, 74.82) mm`，280 logical px/mm，概览 `(16.6, 74.2) mm`，180 logical px/mm。实际打开后须确认当前设计单位，不能将 RDIMM 的 mm 默认沿用为 mil 源坐标。

源几何摘要为 `geometry-candidates/S5000C-U94-visual-target.{md,json}`。U94 附近没有真实走线圆弧；zone 的 6 条外环弧和 4 条孔洞弧仅作为铜皮曲边验证，走线圆弧必须另选准确源对象。该节只确定下一轮取证对象，不声明 S5000C 实际画面、孔洞或图元叠加已通过。

下一轮精确字段、报告清单和 CLI 命令集中在 `geometry-candidates/S5000C-next-actual-target.{md,json}`；29 条指定源记录独立解码全部成功。另一个真实走线圆弧目标为 ART12 / Segment 4356524，180° CCW，中心 `(57.0217554, 84.5710526) mm`、半径 `0.223536535366 mm`、宽 `0.11176 mm`，建议独立相机 `(57.15, 84.75) mm` / 480 logical px/mm。它与 U94 相隔约 41.5 mm，不能写成同一个局部已经覆盖所有图元。

### S5000C U94 的离屏 GPU 与拾取验证（2026-10-04）

用户手动使用当前窗口期间，本轮只执行无窗口验证。硬件测试重新通过正式 importer 读取原 BRD，再保留 U94 附近 2 Zone、2 Segment、3 Pin、2 Via 共 9 个真实对象；释放整板 CPU 场景后使用生产几何准备与 D3D11 `BoardRenderer` 绘制，不创建 GPUI 窗口。原板 303,867,712 B，SHA-256 `09c3e6f170ead979f4f8537ab27bc50f04021855b2c8cde117e5b4935aa393c7`，本轮前后相同。原始 Zone 索引为 Static 933674 / 10、Dynamic 5598600 / 809，子集保留此相对顺序；其他对象原索引也写入报告。

条件为 128×128 离屏纹理、DPI 1、单 TOP、填充焊盘、shape=99/255，global=0/128/255；类别颜色仅用于辨识覆盖关系。概览相机 `(16.6,74.2) mm` / 31 px/mm，孔洞详情 `(16.9801794,75.1774214) mm` / 100 px/mm。关闭板文字、自动标签和绘图，本轮没有字体或 ANSI 开发。断言避开 AA 边缘，不作为精确曲边、其他 DPI 或 Allegro 像素一致证明。

| 检查 | 局部结果 |
| --- | --- |
| Dynamic 真圆角孔洞与 Pin 41 | 三档 global 各 1,132 个净空像素保持黑色，836 个 Pin 901969 内点按自身 alpha 绘制；另以非黑 `[51,77,179]` 预填目标，净空保留该色、Pin 内点仍为蓝色，排除黑色填充伪装挖孔 |
| Static / Dynamic 重叠 | 点阵及实心模式各核对 600 个内点：只含 Dynamic 时 G=99，两次 shape source-over 时约 G=160（`99 + 99 × 156/255`）；不随 global 变化 |
| 焊盘、走线、过孔与普通钻孔 | 两种 Static 模式 × global=128/255，每组合 63 / 21 / 27 / 45 个内部样本符合逐对象 source-over；普通钻孔与铜皮 void 分别验证 |
| 几何缓存 | 铜皮 / 走线 / 焊盘 / 钻孔 / 铜皮边线累计上传分别为 3,036 / 256 / 3,968 / 256 / 4,480 B，改 alpha、填充模式、相机及预填颜色后均未增加 |
| 生产画布拾取 | 既有解析路径子集经 `SegmentIndex::query_visible_hits` 验证 23/23 点位：孔中心仅 Pin 901969，孔内净空不命中，局部重叠 Dynamic→Static；指定 Pin/Via/Segment ID 和 layer/category anchor、隐藏及筛选通过 |

拾取输入是此前 importer 输出的 32,731 B 紧凑几何，SHA-256 `8b56bdffff22240a8517e536c212d1e89f882f939f10ae877e35f17325da7fe9`；该测试本身没有重新读取 BRD，也不恢复 GPU mesh。采用 280 px/mm、5 px 容差、4 MiB 索引预算。全局 alpha=0 仍可拾取普通对象，shape=0 不拾取填充内部、仍可拾取边线，这是当前 Pomelo 产品语义，尚未与 Allegro 实际操作核对。

新增两项测试均为显式 opt-in / 默认 ignored，不使 CI 依赖外部 BRD 或 `.cache`。硬件单项与 CPU 单项各 1 passed；render 全目标/全特性及 import 新测试 Clippy `-D warnings`、格式检查通过。本轮没有重跑整个工作区，也没有修改生产渲染、拾取、字体或 ANSI 逻辑；10 个冻结文件逐个哈希未变。初次硬件测试有坐标类型编译错误及相对输出路径错误，日志保留，失败尝试不计通过。

证据位于 `.cache/allegro-parity/`：`s5000c-u94-offscreen-report.json`、同名黑底 PNG / `-sentinel.png`、`s5000c-u94-verification.json`、`s5000c-u94-sentinel-{hardware,clippy}-final.log`、`s5000c-font-and-source-freeze-verification.json`；精确双铜皮公式复验日志为 `s5000c-u94-exact-alpha-hardware-final.log`。根代理独立核对结果、文件指纹、零重传和冻结文件的 9 项检查见 `s5000c-offscreen-evidence-audit.json`，全部通过。CPU 结果及复跑命令为 `geometry-candidates/S5000C-U94-production-picking.{json,md}`。硬件复跑使用绝对输出路径：

```powershell
$env:POMELO_S5000C_U94_BOARD_PATH = 'E:/brd_cases/S5000C-64_DDR5_BGA_V0.61.brd'
$env:POMELO_S5000C_U94_GPU_REPORT = Join-Path $PWD.Path '.cache/allegro-parity/s5000c-u94-offscreen-report.json'
cargo +stable test -p pomelo-render --features native-gpu --locked --offline hardware_s5000c_u94_real_void_and_same_layer_composition_without_reupload -- --ignored --nocapture
```

**尚未完成**：S5000C 实际 Allegro 报告与画面、整板 GPU 上传/呈现、跨层覆盖、标签、曲边 AA 和独立 ART12 走线圆弧。硬件范围仅为 9 对象子集，不将此前整板 CPU 准备通过升级为整板 GPU 验收。后续 Web 可复用同一对象 ID、解析轮廓、圆角孔洞独立判定与非黑底检查，并保持真实 void 不覆盖目标已有内容；最终显示优先级仍以实际 Allegro 对照为准。

## 四板几何差分与组合器专项（2026-10-04）

独立只读探针执行最新 Web 的 `AllegroParser` / `AllegroSceneBuilder`，与桌面 `pcb_inspect decode-scene` 按对象 ID 比较。统一显式 Windows-1252，坐标/尺寸容差 `1e-8 mm`，将轮廓内部的无图层标记 `u32::MAX` / `-1` 规范化。四板的走线端点/宽度/圆弧、pin/via 位置/方向/镜像/钻孔、pad 尺寸/偏移/自定义轮廓/背钻属性和 zone 解析环均零差异。

| 板 | 铜皮数 | 解析环数 | 实体几何差异 | 解析轮廓差异 | CPU 结构问题 |
| --- | ---: | ---: | ---: | ---: | ---: |
| DemoCase | 72 | 1,814 | 0 | 0 | 0 |
| NVL CAMM | 1,647 | 81,288 | 0 | 0 | 0 |
| DDR5 RDIMM ARM | 13,174 | 213,823 | 0 | 0 | 0 |
| S5000C | 1,017 | 137,404 | 0 | 0 | 0 |

证据与复验入口在 `.cache/allegro-parity/geometry-candidates/README.md`、`*-Web-comparison.json`、`*-structure.json`、`provenance.json`。结构检查覆盖有限值、非负尺寸及三角索引合法性；差分没有证明三角化、显示策略、GPU 或 Allegro 正确性，共用算法错误仍可能同时存在于两端。

新增三个真实 D3D11 硬件专项，3 passed / 0 failed；专项 Clippy `-D warnings` 和格式检查通过。独立合成输入证实当前同类别 TOP 覆盖底层、全局 Pin/Via/Drill 顺序、active/promoted Etch 可覆盖钻孔、每对象 source-over、铜皮独立 alpha、上层真孔露下层和 via drill 的图层 scope。示例两层铜皮 alpha=99 时中心 `[99,0,61]`，上层孔露下层 `[0,0,99]`；这些是当前策略的证明，不是 Allegro 验收。没有改生产管线或字体。

```powershell
cargo test -p pomelo-render --features native-gpu hardware_compositor_ --locked --offline -- --ignored --nocapture
```

日志和版本证据：`.cache/allegro-parity/compositor-policy-{hardware,clippy}.log`、`compositor-policy-verification.json`。零 global 时仍显示 selected/hover 的原生/Web 差异见 A12；active/promoted Etch 与真实钻孔的覆盖仍待 Allegro 校准。

## Demo GND 重叠区域与静态填充显示（2026-10-04）

实际放大 TOP GND 区域：via `2458385550` 位于 `(1870.24,365.25) mil`，源铜盘/孔直径 `16/8 mil`；10 mil 宽直段 `2458384105` 连接到 pin `2457968117`，中心 `(1933.34,365.25) mil`，源 pad 为 `42×87 mil`、owner 旋转 π/2、无孔。它们均在 TOP zone `2458525349` 内。实体尺寸目前来自 CPU/Web 对照；via/pin/trace 的实际报告、像素尺寸和完整混合尚待逐项验收。

该 zone 的实际 Allegro Show Element 报告已导出：ETCH/TOP、GND、solid filled、3 connected pins / 11 connected vias、22 条外边界、0 voids、面积 `0.0177 sq in`。逐边与桌面场景比较，坐标及宽度最大误差 `2.2737367544323206e-13 mil`；CPU 面积 `0.0176766514 sq in` 按报告精度四舍五入一致，区域中同网 pin/via 数量一致。报告及校验在 `.cache/allegro-parity/demo-gnd-zone2458525349-show-element.txt` 和 `geometry-candidates/DemoCase-zone-Allegro-comparison.json`。

实际 Allegro 中这块铜皮为细点阵，邻近大 GND 铜皮实心；F5 后点阵仍在。安装的官方 `doc/algroenvvar/Display_Settings.html` 和 `share/pcb/configure/prfedit/opengl.prf` 明确说明 `static_shapes_fill_solid` 控制静态 Shape 的填充图案。实际 User Preferences 中此项原为未勾选；受控勾选并 Apply 后目标区域变为实心，取消勾选再 Apply 后恢复点阵。原值已经恢复，未保存原始 BRD。这确认显示模式原因，不能把 solid filled 的物理铜皮改为 hatch，也不能根据未证实的 Unknown2 位定义修改解析器。

最初原生和 Web 均实心绘制该 zone。原生现已接入下面记录的身份和显示策略；Web 尚未移植。受控图像证据在 `.cache/allegro-parity/demo-static-fill-{preference-before,solid,restored}.png`；旧原生实心区域为 `demo-native-gnd-overlap.png`。旧截图曾关闭通孔标签检查孔洞，随后已恢复，不能作为标签或完整视觉通过。

### 静态身份与点阵实现（2026-10-04）

共享模型新增 `Zone.kind = Unknown / Static / Dynamic`，旧场景缺失字段时默认为 Unknown。Dynamic 必须由合法的 BOUNDARY、可选包装、owner table 和完整数组链关联证明；Static 仅限完整身份图中未关联的源 raw1。未知布局、自动生成 fillet/teardrop 和矩形保持 Unknown，继续实心显示。不能按非零 TablePtr 或单个位猜身份；详细独立来源、指针关系、版本限制见 `.cache/allegro-parity/geometry-candidates/shape-kind-contract.md` 和 `shape-kind-evidence.json`。

| 案例 | Static | Dynamic | Unknown | 导入诊断 |
| --- | ---: | ---: | ---: | ---: |
| DemoCase | 31 | 41 | 0 | 0 |
| NVL CAMM | 67 | 1,580 | 0 | 0 |
| DDR5 RDIMM ARM | 30 | 995 | 12,149 | 0 |
| S5000C | 107 | 889 | 21 | 0 |

RDIMM 的 Unknown 为 11,971 个 raw4097 和 178 个 raw12289；S5000C 的 Unknown 为 21 个矩形。新增分类 15 项和 serde 兼容 2 项通过。最终生产导入探针在新目录 `.cache/full-import-20261004-zone-kinds-final/` 复跑 **157/157、0 失败**；与旧 v2 逐报告的对象统计、诊断、边界、源身份及来源字段均无变化。CPU 分类和导入通过不代表四板画面通过。

共享 `BoardDisplay.static_shapes_fill_solid` 默认 false，保存时有旧配置默认值；显示面板提供五语“静态铜皮实心显示”。Windows copper HLSL 仅在 Base pass、已确认 Static 且该选项为 false 时应用点阵，Dynamic、Unknown 和自定义焊盘保持实心。点阵空白只使颜色 alpha 为零，不丢弃几何遮罩，因此物理孔洞并集、标签裁剪、轮廓和拾取保持原契约，切换不要求重新上传几何。

Allegro 在两种缩放倍率下测得相同的 16×16 画布逻辑像素图案：第 3/14 行在 x=3/8，第 6/11 行在 x=0/11，其余为空。初版按物理像素绘制，在原生 150% DPI 实窗出现过密点阵；已定位到 DPI 策略，HLSL 改用 `(position.xy - canvas.xy) / viewport.z`。新硬件 oracle 在旧 Shader 上先复现失败，修复后覆盖 DPI 1/1.25/1.5/2、分数画布起点、缩放/翻板、连续轮廓、重叠孔洞和下层实心铜皮，2 项通过。实际原生截图的最终周期及相位仍须复验，不能仅凭硬件测试宣称与 Allegro 一致。证据见 `static-pattern-measurement.json`、`static-logical-dpi-verification.json` 和 `static-logical-dpi-{before,after}.log`（均在 `.cache/allegro-parity/`）。

初版 Demo 隔离实窗已完成 false → true → false 鼠标切换：目标 Static 点阵/实心切换，邻近 Dynamic 不变；对应 `demo-native-static-{stipple,solid,restored}.jpg`。该截图使用旧物理像素策略，不能用于证明最终 DPI 修复。最终 Debug 副本 SHA-256 为 `dc4b33a97f0da0d5d6cdbe0ba4dccaac2cac7a9633811517f380b0bd25ae6d00`；隔离配置、PID 和路径在 `static-final-app-launch.json`，遥测 DPI=1.5、static=false、ready=true，提交/呈现 9/9、无 GPU 错误。窗口工具未发现最终新实例，因此周期/相位、键盘切换、点阵空白中的拾取和正常退出重开恢复仍待实窗补验。没有操作用户正在使用的 Release 窗口；本阶段 Release 构建因该 exe 被运行进程占用而失败，未强制关闭用户实例。

最终工作区全 targets / all features 回归 **520 passed、0 failed、25 ignored**，48 个测试目标；显式 D3D11 硬件回归 17 项通过。Clippy `-D warnings`、格式和 diff 检查通过，Debug 构建通过。日志为 `static-final-{workspace-tests,workspace-clippy,debug-build}.log`。ignored 项不能计为通过，ANSI 字体开发仍按用户要求暂停，未继续开展源字体实窗验收。

### 零 global 与标签独立透明度（2026-10-04）

在实际 Allegro 将 global 设为 0、shape 保持 99，并 F5 刷新：普通走线/焊盘/钻孔隐藏，通孔 `1:8` 跨层标签和铜皮 GND 名仍在。刚 Apply 时残留的灰色钻孔在 F5 后消失，不能据此推断钻孔不受 global 影响。临时 Show Element 选择目标铜皮保留原绿色小点填充，另加黄色连续边界；持久 GND 网络高亮则改为黄色大菱形点阵。已恢复 global=255、shape=99 和无网络高亮，未保存源 BRD；Find 面板筛选未完全恢复原值，不声明该项恢复。图像为 `demo-global-zero-{refresh,temporary-shape,persistent-net}.jpg`。

原生 glyph 已在 `rotation.w` 低位编码独立透明度，上位保存 atlas page；原 HLSL 忽略低位，导致独立 span 被 global=0 清空。现在仅解码现有元数据：普通 via 网络名乘 global，通孔 span 保留自身 alpha，ZoneName 保留 `0.62 × shape`，同一个 Drill batch 也分别处理。生产修复仅在 `label.hlsl`，没有改字体、图集、字号、旋转、镜像或布局。旧 Shader 先复现 span 像素由 182 变黑，修复后 global=0/128/255 的 55 个 span、75 个普通 via 名和 142 个独立 ZoneName 像素通过；切换 opacity 不重传标签。证据见 `label-opacity-gpu-verification.json` 和 `label-opacity-{before,after}.log`。实际 Allegro 观察和合成硬件证明分开记录，最终原生实窗及 A12 两种高亮图案仍未验收。

### 实窗高亮与退出保存复验（2026-10-04）

本节补充此前最终副本未被窗口工具发现的验证缺口。使用可枚举的独立测试窗口，未操作用户日常 Release 实例。ANSI 实验与代码按最新指示继续保留，不回滚；本轮没有更改字体、图集、字形布局或 Web。

高亮验证副本 SHA-256 为 `fcc109a6259e8604474a7cb8937b64f319401efd539e00aad41cde4b801e31db`，启动记录 `highlight-interactive-launch.json`。Demo 单 TOP，global=255、shape=99，DPI=1.5，相机中心 `(50.2,7.9) mm`、77.7071 逻辑 px/mm。实窗证据均在 `.cache/allegro-parity/`：

- `demo-native-zone-object-outline.jpg`：在静态铜皮点阵的空白位置实际点击，命中 `2458525349`、GND 网络 `2457728874`，连续白色边界保留原小点阵，旧选中填充不再覆盖空白。点击时遥测独立归档为 `zone-object-interactive-runtime.json`，21 次提交/呈现、无 GPU 错误。不能用后来清除选择的遥测证明这次对象身份。
- `demo-native-zone-net-diamonds.jpg`：将选中对象切到网络模式，Static `2458525349` 和已确认 Dynamic `2458822263` 都显示菱形，原 Base 填充被替换，菱形空白可见背景；GND 成员为 941 segments、429 pins、451 vias、23 zones。
- `demo-native-zone-net-zero.jpg`：global=0、shape=99 时两片菱形及通孔 `1:8` 跨层标签仍在，普通几何消失。选中网络的大铜皮名在此状态下消失，尚未与相同 Allegro 选择状态核对，不能宣称所有选中标签的 alpha 已通过。Base 无网络选择的零 global 实窗见 `demo-native-independent-zero.jpg`，span 和独立铜皮名保留。
- Base 点阵在 `demo-native-logical-stipple.jpg` / `demo-native-logical-restored.jpg` 的局部横截面，16 逻辑像素间隔的 8/8 峰位置一致；完整相位和其他 DPI 的实际窗口仍待验。JPEG 点阵经采样会变灰，按 RGB 最大值辨识，不按“绿色远高于其他通道”错误丢点。

网络菱形来自实际 Allegro 截图逐位恢复的 **16×16 物理像素**位图，与 Base 的 16×16 **逻辑像素**小点阵不同。16 行 mask 为 `0000/01c0/03e0/07f0/0ff8/1ffc/3ffe/3ffe/3ffe/1ffc/0ff8/07f0/03e0/01c0/0000/0000`，109/256 墨点。DPI=1.5 截图坐标的实际采样拟合覆盖全部 256 个位、0 矛盾，三个独立 ROI 的 1024/3072/6912 个采样全部吻合。证据及可复跑脚本为 `zone-highlight-verification.json` / `measure-zone-highlight.py`。Windows 实现只在已确认 Static/Dynamic 的 Net 高亮替换 Base 颜色，仍保留完整物理遮罩；Unknown、悬停、其他分组及自定义焊盘维持原策略。新增 5 项硬件专项覆盖替换、零 global、孔洞/标签遮罩等，连同已有专项为 22 项通过；实际 Net 图案的绝对相位及其他 DPI 仍待核对。

实窗还暴露正常退出保存缺陷：旧版本切换静态实心后 Ctrl+Q，进程退出，但保存值仍是 false，留下 153 字节的面板临时文件。相同旧版本先关文档标签再重新打开，设置可正确保存恢复。GPUI 的退出回调受 200 ms 预算限制；现有串行面板/视图原子写入及前置任务未必在此预算内完成，不能将空 stderr 解释成保存成功，也不能推断为 Windows 权限拒绝。

`workbench/saving.rs` 现在使 Ctrl+Q、文件菜单退出和窗口 X 先等待完整串行保存，期间保持事件循环运行；每次写后比较完整快照与保存 generation，有新变化则继续写，稳定后才退出。重复退出请求合并，写入错误保留窗口并显示既有五语诊断，可重试。原子写入继续记录 directory/temporary_create/serialize/flush/sync_all/persist 等失败阶段；系统 END_SESSION 仍使用 GPUI 限时兜底，不承诺强制结束或断电保存。没有修改 GPUI 的超时或新增退出强杀。

实际复验使用最新 Debug 副本 `ad0655befed0be6466f0a70d02992e043a34647e660dc30ebcb291c719b9e8df`，隔离配置和 PID 见 `quit-flush-launch.json`：

1. 初始实心 true → 实际点击切为 false → Ctrl+Q。进程结束，`views.json` 保存 false，无临时文件，stderr 为空，见 `quit-flush-ctrlq-result.json` / `quit-flush-ctrlq-views.json`。
2. 重开同一副本，画布恢复点阵及原相机，见 `quit-flush-reopened-stipple.jpg`；global=255、shape=99 保持。
3. 切回 true → 点击实际窗口右上角 X。进程结束，保存值 true、相机保留，无临时文件，stderr 为空，见 `quit-flush-window-close-result.json` / `quit-flush-window-close-views.json`。窗口系统 UIA 的 Close 项没有 bounds，首次未触发关闭；重新观察截图后用实际按钮完成，未把首次失败计作通过。

新增两个真实文件系统失败回归覆盖“父路径为文件”和“目标为非空目录”，保留原文件/目录内容并核对失败阶段、路径、诊断及临时文件清理。最终全工作区 all-features / all-targets 为 **522 passed、0 failed、30 ignored，48 targets**；Clippy `-D warnings`、格式/diff 检查及 Debug 构建通过。日志为 `final-20261004-{workspace,clippy}.log`，汇总 `final-20261004-test-summary.json`，构建 `quit-flush-debug-build.log`。22 项硬件是此前本轮显式运行结果，未将普通回归中的 ignored 再计为通过。Release 链接受用户运行中的 exe 占用，未关闭该实例，本次交付验证版本为 Debug。

### NVL R4D1 实际对象与配置校准（2026-10-04）

已实际打开 `874140_NVL_H_LP5X_CAMM_T3_Rev_0p7_23p1.brd`，源 SHA-256 `290bf390a0ed1b7361617158017debed9865656bd1df9da90882ca402ec5d3f0`。Allegro 由 23.1 板格式加载到 25.1 的升级仅在内存进行，原 BRD 未保存。通过 Find By Name 定位 R4D1，报告原点 `(104.219,-83.822) mm`、180°、不镜像；源文件及截图为 `nvl-r4d1-allegro.txt` / `nvl-r4d1-allegro-report.jpg`。

比较配置仅显示 TOP 的 Etch/Pin/Via，背景黑，实际 Color Dialog 精确将来源 global=198、shape=187 校准到 **255/99**，见 `nvl-allegro-alpha-calibrated.jpg`。RGB 仍按用户要求排除一致性门槛。原生相机 `(104.775,-83.947) mm`、180 逻辑 px/mm、DPI=1.5，填充焊盘和静态实心开启，版本同上述 `fcc109…`；启动记录 `nvl-interactive-launch.json`，区域对象清单 `geometry-candidates/NVL-R4D1-visual-target.{json,md}`。

独立报告核对已通过：

| 实际目标 | Allegro 报告与原生解析核对 | 结论边界 |
| --- | --- | --- |
| Dynamic zone `2907362` | ETCH/TOP、+VCCPRIM_VNNAON、Dynamic BOUNDARY；11 条外边、5 条弧、0 void；端点/宽度最大误差 `1.42e-14 mm`，弧心/半径最大误差 `2.44e-5 mm`，小于报告 4 位小数的 `5e-5 mm` 舍入容差 | CPU 面积 `0.0128706321 cm²` 与报告 `0.01287 cm²` 匹配；开放半圆凹口半径 `0.3812 mm`，不是闭合孔；尚非 GPU 像素验收 |
| Via `1207888` | `(105.676,-83.838) mm`、镀孔 `0.2 mm`、同网、同 padstack、TOP→BOTTOM，与解析的 layer 0→17 一致 | 报告没有验证各层 pad 直径；Allegro rotation=180°、当前场景 angle=0，圆盘旋转不改变此区域画面，但不能据此宣布角度字段一致 |

原始报告为 `nvl-zone2907362-allegro.txt` / `nvl-via1207888-allegro.txt`；数值、报告哈希及复验脚本为 `nvl-actual-reports-verification.json` / `verify-nvl-reports.py`。

原生 Object 模式实际点击过孔钻孔中心，命中 `Via(1207888)`、anchor category=drill，检查器显示上述坐标/0.2 mm 孔径及同 padstack；267 次提交/呈现、无 GPU 错误，证据 `nvl-native-via-hole-pick.jpg` / `nvl-via-hole-pick-runtime.json`。再点动态铜皮内露出的区域，命中 `Zone(2907362)`、TOP，连续白色边界沿半圆凹口绘制；271 次提交/呈现、无 GPU 错误，证据 `nvl-native-dynamic-object-pick.jpg` / `nvl-dynamic-object-pick-runtime.json`。这些证明真实目标身份及局部拾取，尚未证明钻孔/铜皮抗锯齿、所有选择状态或重叠透明度与 Allegro 相同。

测试配置的首次人工 `layer_order` 写入使铜层获得 promoted 优先级，初始图中钻孔被铜层覆盖。实际点击“恢复源顺序”后该按钮禁用，钻孔和 `1:18` 标签可见。对应图 `nvl-native-promoted-order-before.jpg` / `nvl-native-source-order.jpg`；不是已证明的默认叠加错误，不根据这份人工配置改生产策略。当前 `runtime.json` 的 `display.layer_order` 输出的是展开后的 ordered_layers 完整绘制列表，即使恢复默认仍包含所有层；不能从该输出误判自定义排序未清除。实际 active/promoted 与 Allegro 的精细覆盖仍待受控核对。

新的临时状态差异仍未闭环：Show Element 检查此 Dynamic 后关闭报告，目标出现白边和密点，见 `nvl-dynamic-temporary-highlight.jpg`，不是持久 Net 大菱形。只读审查发现局部横/纵 8 截图像素周期，记录在 `nvl-dynamic-selection-audit.json`。指针移至目标外、Escape 并未结束该命令；右键菜单中的 Done/Cancel 快捷键尝试未退出，最终点击 Cancel 后状态为 Idle、密点消失，F5 后仍是普通填充，见 `nvl-allegro-idle-refreshed.jpg`。因此不能将一张命令预选画面直接映射为所有 Dynamic 对象选择；还需分开悬停、实际点击后临时报告状态、Static 对照及明确 Net 高亮。字体不参与这项核对。

本局部尚缺 Static `1398196` 的独立报告、各类实体尺寸像素、同网铜皮交叠 alpha、分类标签和原生相同选择状态的验证；NVL 整板、多层和另外两板的实际视觉验收仍未完成。

### NVL 受控选择与修复（2026-10-04）

本节补齐上节的 Static 报告与临时选中对照。ANSI 实验及已有代码继续保留，当前 UI/PCB MSDF、字体图集和布局均未改；本次仅修改铜皮 D3D11 选择策略、HLSL 和相应硬件回归。

实际 Show Element 独立导出 Static `1398196` 报告 `nvl-zone1398196-allegro.txt`：ETCH/TOP、+VCCPRIM_VNNAON、4 条直边、0 宽度、0 void，边界 `(104.6142,-84.1604)`～`(105.2879,-83.4796) mm`。CPU 面积 `0.0045865496 cm²` 与打印的 `0.00459 cm²` 差 `-3.4504e-6 cm²`，在打印精度内；端点/宽度最大误差 `1.42e-14 mm`。独立 Green 积分与细分折线面积再次确认 Dynamic 的旧面积公式正确；证据 `geometry-candidates/NVL-actual-independent-audit.json`。报告中的 solid filled 是对象数据描述，不单独证明显示方式或 Static 身份。

Allegro 同一视角、TOP-only、DPI=1.5：以 global=0 隔离焊盘/走线，实际 Show Element 点选对象、关闭报告、指针移出画布并 F5；右键菜单 Cancel 明确回到 Idle。分别采集 shape=99/128/255/0。Static 的普通显示确为原有 16×16 **逻辑**点阵；点选该 Static 后仅原点位变白，50 个测量点缺失/新增均为 0。点选 Dynamic 时，这 50 个邻近 Static 点的 RGB 完全不变，排除把同网整组高亮当成 Object 行为。

| 实际状态 | 原生修复及 Web 移植契约 |
| --- | --- |
| Static 普通点阵 | 使用既有逻辑屏幕位图；原填充保持完整 stencil 覆盖 |
| Static Object 选择，点阵模式 | 在相同点位叠加白色，alpha 使用独立 shape 值；间隙不添加新点 |
| Static Object 选择，实心模式 | 保持原填充，仅增加连续白色边界，沿用 Demo 已验证规则 |
| Dynamic Object 选择 | 在基础填充之上再做一次 source-over：密点位置取白色，间隙取原材质 RGB，共用 shape alpha；不是只叠加孤立白点 |
| shape=0 | 选择填充及密点消失，连续边界仍在，实际 Show Element 仍能命中对象 |
| 持久 Net 选择 | 沿用此前 109/256 物理像素菱形替换填充策略，不混用 Object 位图 |

Dynamic 的 132 个间隙样本在 shape=99 时，Idle/Selected RGB 中位数为 `[97,47,48]` / `[154,75,76]`；shape=128 时为 `[124,60,60]` / `[184,89.5,90]`。第二次相同材质 source-over 的预测残差中位数分别约 `[-2.89,-0.75,-2.36]` / `[-2.26,0.12,0.12]`，与 JPEG 误差相容；shape=255 间隙基本保持原色，shape=0 为全零。联合这四档确认独立 shape alpha 的二次合成规则，不能用固定提亮倍数替代。

密点的完整最小候选周期为 **4 物理像素**。当前 Allegro 窗口相位行 `[0,4,0,1]`，画布原点 `(196,96)` 逻辑 / `(294,144)` 物理后，对应画布相位 `[0,1,0,4]`。这是 DPI=1.5 下测得的最小候选；更大但未观测全的位图仍可能拟合，其他实际 Allegro DPI 与原生画布精确相位尚未验收。不能把硬件测试的多 DPI 通过写成实际 Allegro 多 DPI 一致。

复算脚本 `measure-nvl-controlled-selection.py` 和证据 `nvl-controlled-temporary-verification.json` 保存图像哈希、点阵、四档 alpha、源码哈希及日志。原图为 `nvl-allegro-global0-{idle,static-selected,dynamic-selected,cancel-idle}.jpg`、`nvl-allegro-global0-shape{128,255,0}-{idle,dynamic-selected}.jpg`。实验完成后 Color Dialog 恢复 global=255、shape=99，见 `nvl-allegro-restored-after-alpha-series.jpg`，源 BRD 哈希未变。Show Element/Cancel 后部分钻孔显示曾残留命令状态差异，因此不以该组 global=0 截图宣称钻孔覆盖一致。

**真实原生窗口**：Debug SHA-256 `9a21d30a987e26d7e82ca8a132fc0ca912c3675ae897a4e2b1226d35da04526a`；独立配置 `nvl-selection-aa17f80959694001badaf906bdcc5f18`。显式 `--encoding windows-1252`、相机 `(104.775,-83.947) mm` / 180 逻辑 px/mm、global=0、shape=99、Static 实心关闭、源图层顺序、仅铜皮拾取。实际点击 Dynamic 与 Static 分别命中 `Zone(2907362)` / `Zone(1398196)`；指针移出画布后遥测 `hovered_object=null`，显示密点/稀点差别和邻近未选对象恢复。108/108、116/116 次提交/呈现，GPU 无错误，铜皮累计上传 `253507340` 字节保持，MSDF 114821 实例、源 ANSI 笔画 0。

窗口及遥测证据 `nvl-new-native-{dynamic,static}-runtime.json`、`nvl-new-native-{dynamic,static}-selected.jpg`，汇总 `nvl-new-native-selection-verification.json`。第一次启动遗漏编码，来源身份保护正确跳过旧 windows-1252 视角，该整板画面排除验收；重开匹配编码后才记录上述证据。计划采集 Cancel 前用户已清除选择并缩放，单独保存 `nvl-new-native-user-cleared-and-zoomed.jpg` / 对应 runtime，不将其冒充相同视角的受控 Cancel 像素比较。

**回归**：新增 3 项 Object 硬件测试，其中静态点阵与动态间隙测试先复现旧实现差异，修复后含旧 Net/孔洞/custom pads/stencil/标签 alpha 的 **25 项 D3D11 硬件测试通过**。检查 4 个渲染 DPI、0/99/128/255 alpha、间隙/孔洞/邻居不受影响、Cancel 精确恢复和零几何重上传；其中 alpha 检查 7788 像素。`pomelo-render` **97 passed、0 failed、29 ignored**，全目标/全特性 Clippy `-D warnings`、fmt、Debug 应用构建通过。硬件命令按 `pixel_tests::` 过滤；首次过宽 `--ignored` 误包含三个缺少显式输入的外部案例测试，原失败日志保留，不隐藏为成功。本轮未重跑整个工作区，此前 522/30 为上一阶段证据。

**剩余范围**：此处关闭 Static 报告缺口及两种 Object 填充缺失问题；没有宣布像素抗锯齿、精确相位、标签配置、NVL 整板、多层或四板整体通过。普通 Via `1207888` 原始 `0x33.Unknown5=180000` 与实际报告 180°一致，而原生及最新 Web 普通过孔场景 angle=0；当前同心圆几何不受影响，暂记录，未来非对称/偏移焊盘需修复并核对镜像及 bond finger，不能称角度已一致。下一板真实窗口目标已整理为 `geometry-candidates/RDIMM-L83-visual-target.{md,json}` 与 `S5000C-U94-visual-target.{md,json}`，实际核对仍待执行。

## 类别颜色与背景修复（2026-10-04）

共享核心新增 `BoardAppearance`，颜色用不含 GPU 类型的 `RgbColor([u8; 3])` 保存为不透明 sRGB 字节。每层保留独立 Etch/Pin/Via 可选覆盖，另存画布背景和钻孔色；空覆盖回退原来源层色/原主题背景，不修改源 BRD。新字段有默认值，既有查看状态可继续恢复，外观层数量超过 4096 时按既有五语配置错误拒绝。颜色与铜皮透明度分开存储；选择器支持 alpha，但提交后按 RGB8 量化、恢复不透明色，避免其色块和实际材质不一致。

Windows 组合器在每个绘制命令选择类别材质，通过 `TraceFrame.material_override` 传给实际 HLSL 管线。Etch 覆盖走线、铜皮、铜皮边界及物理铜层源板文字；解析和自定义焊盘按 Pin/Via 分类，BOND TOP die pad 继续按 Etch。自动网络名使用原校准颜色，非铜层源文字和辅助绘图保持原策略。网络着色优先于铜材质 RGB，钻孔始终用独立颜色；选择/悬停策略保持。背景仅应用于 PCB 画布，未改 UI 主题或字体。

实际回归发现并修复了两个问题：

- `label.hlsl` 把源板文字类别写死为 `5u`，共享 `DisplayCategory::Text` 实际为 6，源文字未按图层材质取色。新增硬件测试先复现绿色 Etch 下源字形仍为灰色；改为由共享 Rust 枚举生成 `PCB_SOURCE_TEXT_CATEGORY` 后通过。后续 Web 移植应使用自身统一类别契约，避免复制这个旧编号。
- `BoardRenderer` 原先忽略 `BoardFrame.drill_color` 并每次创建空颜色表、硬编码灰色。现在执行传入钻孔材质或文档覆盖；应用的无覆盖颜色仍保持原来的 `[0.46, 0.49, 0.51, 1]`。原测试曾传入未生效的占位钻孔色，本轮使这些 fixture 明确传入其原有灰色，保留原像素断言；新测试另外验证传入 0.2 灰色和 `[172,172,172]` 覆盖均真正生效。

左侧展开图层提供三类 Kit ColorPicker 和各自恢复按钮；显示面板提供背景、钻孔 ColorPicker 与恢复按钮。六条新增消息通过 rust-i18n 同步五语。显示面板颜色行接入已有焦点滚动；完整键盘、最低窗口五语、浅色自定义背景下坐标/标尺对比度尚未验收，不声明这些组合通过。

### 本轮证据与边界

- 新增硬件测试 `hardware_category_colors_cover_analytic_custom_copper_text_and_drills_without_reupload`：解析/自定义两套场景各检查 Etch、Pin、Via、两处钻孔及 40% 铜皮，共 12 处固定像素；源板文字墨迹、网络着色、钻孔不受网络色影响、恢复默认和六类上传量不增加通过。它验证材质传递，不替代同层/跨层遮挡的 Allegro 对照。
- 相关 11 项硬件回归通过，包括 MSDF、曲边、孔洞、背钻、die、自定义焊盘与长线精度。工作区 489 项通过、18 项默认忽略；Clippy 全目标/全特性 `-D warnings`、格式及 Release 构建通过。日志位于 `.cache/allegro-parity/appearance-{hardware,hardware-regression,workspace-test,clippy,release-build}.log`。
- 实窗使用同一 Release，SHA-256 为 `328f444386debed493c7d871965eb90a24988a6a3245ee3671e27f19c4366d3a`，私有配置 `.cache/allegro-parity/appearance-3348e63cd9624e45a7bbb2084160b16f`。继承已验证的 DemoCase TOP-only、39% 铜皮、非填充焊盘、水平标签和六类标签状态，实际提交 TOP 三类绿色 `[38,255,38]`、背景 `[0,0,0]`、钻孔 `[172,172,172]`。背景单项恢复只清除背景覆盖，其余四项保留。
- 改色前后八类几何/源文字 cache、pipeline 和 uploaded bytes 均不增加，实际运行提交=呈现=237、GPU 错误为空。Ctrl+Q 正常退出确认进程消失，落盘保留全部五项颜色；同一二进制重开后 619%/翻板和颜色恢复，随后实际适配整板、取消翻板，黑色画布的正面 `i.MX 8M MINI` 文字已使用绿色，提交=呈现=22、无 GPU 错误。
- 原始观察/状态：`appearance-ui-observations.json`、`appearance-baseline-runtime.json`、`appearance-matched-runtime.json`、`appearance-geometry-comparison.json`、`appearance-background-restored-runtime.json`、`appearance-saved-views.json`、`appearance-reopen-runtime.json`，均在 `.cache/allegro-parity/`。Computer Use 的 `set_value` 返回窗口查找错误，但同一活窗口复读确认 hex 已写入，Enter 后正式显示状态和颜色改变；没有据此重启、重复提交或把错误返回当作成功证据。完整颜色控件键盘流程仍待补验。

**来源颜色边界**：本轮是用户可操作的文档颜色覆盖，没有把 `.prm` 自动导入应用，也没有宣称 BRD 自带类别颜色已读取。`[38,255,38]` 来自已导出调色板的 57 号颜色，TOP 原始颜色字低位与其一致；这是候选映射，未经控制实验证明全部位语义。Allegro 窗口确认 TOP 三类为绿色、黑色背景和灰钻孔，颜色设置本轮仅选择调色板后 Cancel，未改变源 BRD 或参考图层色。准确来源映射、颜色空间、源文字尺寸/字形、标签密度和叠加仍待完成；铜皮 99/255 精确量化的后续修复见下一节。四板整体验收仍未通过。

## 全局与铜皮透明度修复（2026-10-04）

### Allegro 受控显示实验

保持同一原始 DemoCase、单 TOP Etch/Pin/Via、黑背景、绿色 `[38,255,38]` 和灰钻孔 `[172,172,172]`，相机不变。Color Dialog 实际读取 shape=99、global=255；将 global 改为 128 后，再恢复 255。Shadow mode 关闭，Object filter 为 0。每次应用后执行 **F5**：仅 Apply/OK 会暂时残留旧钻孔像素，不能把那个瞬间当成最终混合结果。

稳定截图实际为 JPEG，以下是固定区域 RGB 中位数，允许 JPEG 压缩误差；它们支持透明度行为判断，不是 Allegro 逐像素一致证明。坐标、样本数、图像哈希和源板哈希见 `.cache/allegro-parity/allegro-opacity-measurements.json`；控件原始观察见 `opacity-ui-observations.json`。

| 固定区域 | global=255、shape=99 | global=128、shape=99 | 恢复 global=255 |
| --- | --- | --- | --- |
| 铜皮内部，35 像素 | `[16,98,14]` | `[14,100,13]` | `[16,98,14]` |
| 非填充模式钻孔内部，64 像素 | `[172,172,174]` | `[86,85.5,88]` | `[172,172,174]` |
| 走线区域，285 像素 | `[0,20,0]` | `[0,10,0]` | `[0,20,0]` |
| 源文字基线绿色 >230 的同一 959 个墨迹像素 | `[50,246,51]` | `[28,122,29]` | `[50,246,51]` |

另实际启用 Filled pads，分别以 global=255/128、shape=99 采集 F5 后画面。相同钻孔内部区域从 `[171,173,170]` 变为 `[97,150,97.5]`，铜皮内部绿通道仍约 99；半透明钻孔暴露下方绿色填充，因此不能统一期待灰色 86。该观察与逐对象 source-over 相符，但尚未定位该区域所有 source owner，不单独证明全部类别优先级或跨层顺序。实验后实际恢复 Filled pads 关闭、global=255、shape=99 并 F5；源 BRD SHA-256 保持原值，未保存 BRD。

### 共享契约与 Windows 实现

- `BoardDisplay.global_opacity` 独立保存，默认 1.0；旧配置缺少字段按 1.0 读取，非有限值和超出 `[0,1]` 的保存值拒绝。`copper_opacity` 继续独立保存，既有默认值不因 DemoCase 参考参数而全局改写。
- D3D11 对普通走线、焊盘、钻孔、绘图和源板文字的材质 alpha 应用 global；网络着色只替换 RGB，不覆盖 alpha。铜皮填充使用 shape，不额外乘 global。当前铜皮边界沿用独立策略，其 Allegro 局部边界像素仍待校准。自动标签使用 global；选择/悬停覆盖层保留原策略，避免零透明度后无法辨认操作对象，参考软件的覆盖层强度另待核对。
- 采用每图元的 source-over：`Cout = Csrc × alpha + Cdst × (1 − alpha)`，覆盖率乘入本图元 alpha。真实几何孔洞保留空区域，不能画背景色伪装挖孔；也不能先画不透明整板再统一降低画面 alpha。颜色空间的跨应用完整对照仍未通过。
- 显示面板使用标准 Kit NumberInput，提供 global/shape 的整数 0–255 输入，值按 `byte / 255` 提交；原百分比滑块和按钮继续工作，精确框在非编辑时显示最近字节，不主动改写旧百分比值。按单字节减小/输入可正式量化。无效输入保留已应用材质，显示五语错误，Enter/离开焦点恢复最后有效值。每文档持有自己的输入状态。
- 本轮硬件测试先复现普通图元没有应用 global，再发现 MSDF 的 `view.z` 被每批次网络着色开关覆盖为 0，使自动标签完全透明。修复后 MSDF 保留 opacity，普通 trace 管线继续使用该槽位作为 net flag；源码文字取材质 alpha，自动标签取该常量，各应用一次。

### 当前验证与复验入口

新增共享保存回归覆盖 128/255 与 99/255 独立往返及非法保存值。生产组合器硬件回归包含解析/自定义两组焊盘、走线、钻孔、铜皮、源文字和自动标签：验证全/半/零 global、独立 shape、网络模式 alpha、零 global 下仍可见选择边缘、恢复像素和七类 GPU 上传不增加。解析填充焊盘下的半透明钻孔与自定义真实孔洞采用不同预期背景，不将 source-over 当作全局淡化。完整工作区 **491 passed、0 failed、18 ignored**；显式硬件 **12 passed、0 failed**（额外真实 compact text 测试使用 DemoCase），全目标/全特性 Clippy `-D warnings`、格式和 Release 构建通过。

```powershell
$env:POMELO_TEXT_BOARD_PATH = 'E:\brd_cases\DemoCase_LPDDR4.brd'
$env:POMELO_TEXT_BOARD_ENCODING = 'windows-1252'
cargo test -p pomelo-render --features native-gpu hardware_ -- --ignored --nocapture
```

最终 Release SHA-256：`be46b5c4372ca68886c4fc01bf4841c636c64bfbb164858756d5a98034847df1`。私有配置 `.cache/allegro-parity/opacity-e46ffbc566394c128c8146c2b0941da0` 使用同一 DemoCase、619% 翻板、单 TOP 和已验证颜色；首次默认 UTF-8 在原生界面报告解码失败，实际选择 Windows-1252 并重试后成功。实窗已验证：

- 铜皮 99 → 98 的单字节按钮、精确输入 99，以及 global=128；运行报告实际为 128/255 与 99/255，而非四舍五入的 50% 与 39%。
- global 输入 300 显示就地错误、材质保持 128；Enter 与 Tab 分别恢复 128。global=0 后普通图元和文字消失，铜皮仍保留；随后恢复 255。
- 原百分比提高按钮把铜皮显示为 44%、精确框 112；再输入 99 后左侧滑块恢复实际 38.823528…%。
- Ctrl+Q 正常退出后原进程终止，查看状态保存 128/255、99/255、相机和显示项。同一二进制重开（显式 Windows-1252）恢复正确；保存 JSON 的 f32 最短十进制与 runtime JSON 扩为 f64 的数值按 f32 位比较相等。
- 改值阶段八类几何/源文字的 pipeline、cache、上传量未增加；标签按现有布局缓存策略另计。退出前提交=呈现=315、错误为空；重开记录 39/39、错误为空。当前窗口已恢复 global=255、shape=99。UI 字体文件哈希仍为 `2cb472958b85ea4a32316961c2902b535903697719b9743231743bf1e5c0ed70`。

日志为 `.cache/allegro-parity/opacity-{hardware,hardware-regression,workspace-test,clippy,release-build}.log`；实窗证据为 `opacity-native-ui-observations.json`、`opacity-{baseline,matched,before-exit,reopen,final}-runtime.json`、`opacity-saved-views.json`、`opacity-geometry-comparison.json`、`opacity-restore-comparison.json`。Computer Use `set_value` 在此 GPUI 窗口仍返回窗口查找错误，随即同一窗口复读确认输入与正式材质改变；没有重复提交或重启来掩盖错误。完整五语/最低窗口/键盘/DPI 矩阵仍待验，四板整体仍未通过。

### 后续 Web 移植

共享模型加入默认 1 的 global opacity，保留独立 shape；保存迁移、精确字节输入和五语消息保持相同语义。Web 走线/焊盘/钻孔及自动标签管线分别传 alpha，网络 RGB 不替换 alpha，MSDF source text 与自动标签各只应用一次。用同一解析/自定义孔洞场景验证 128/255、99/255、零 global 和选择覆盖；不要直接复制 Windows 的 `view.z` ABI，因为 Web 各 shader 的常量布局不同。源字形、标签字号/密度以及同层/跨层真实覆盖继续按 A04/A06/A07/A08 修复；A10 的局部走线核对见下一节。

## 相邻走线的实际对象核对（2026-10-04）

此前在低倍率 TOP 扇出区域看到成对绿线，将其暂记为可能的“双边轮廓”。本轮在同一原板实际使用 Allegro Zoom By Points 放大，再用 Show Element 的 Cline segs 过滤分别读取两条线。参考画面显示各自的实心内部，而非同一对象的两条边界。Filled pads 的既有对照不能用于推断走线空心模式。

| 网络 / 原生源段 ID | Allegro TOP 起点（mil） | 终点（mil） | 宽度（mil） | 长度（mil） |
| --- | --- | --- | ---: | ---: |
| CSI_DP2 / 2458256277 | (993.65, 1395.82) | (993.65, 1620.13) | 3.00 | 224.3100 |
| CSI_DN2 / 2458256836 | (984.65, 1406.51) | (984.65, 1616.40) | 3.00 | 209.8900 |

从当前源码重新构建 Release `pcb_inspect` 并输出 `trace-democase-scene.jsonl`，以 1 mil = 0.0254 mm 核对两段：网络、TOP 层、端点、宽度、长度全部匹配，数值误差小于 `1e-9 mil`。中心距为 **9 mil**，边缘净距为 **6 mil**；本轮中途口头读为 7 mil 有误，以此计算记录为准。只核对这两段，不将导出器的 `scene_validated=false` 改成整板通过。

原生实窗使用透明度阶段同一 Release（SHA-256 `be46b5c4372ca68886c4fc01bf4841c636c64bfbb164858756d5a98034847df1`）和私有配置。搜索 CSI_DP2 后定位，切至正面、取消高亮，再实际点击两段：分别命中 Segment 2458256277 / 2458256836，选择 Net 2457724766 / 2457724753，原生报告无 GPU 错误。相机保持同一正面视角、44.48102925647463 逻辑像素/mm；global=255、shape=99、单 TOP、非填充焊盘。原生 JPEG 的无标签横截面确认两个实心绿色内部及其间黑色空隙；仅采用容差阈值，未声称边缘抗锯齿或跨应用 RGB 精确一致。

证据位于 `.cache/allegro-parity/`：`trace-csi-{dp2,dn2}-report.jpg`、`trace-csi-{dp2,dn2}-uia.txt`、`trace-democase-scene.jsonl`、`trace-native-unselected.jpg`、`trace-allegro-zoom-stable.jpg`、`trace-native-{dp2,dn2}-runtime.json`、`trace-pair-verification.json`。Allegro 两份 Show Element 内容均由实际工具截图读取并人工转录，报告截图均已保存。验证脚本 `verify-trace-pair.py` 记录源板、二进制与证据哈希，并核对两个实际鼠标命中和 JPEG 横截面；另在 Allegro 稳定截图中确认两条实心内部和空隙。两应用截图倍率不同，未进行宽度/抗锯齿的逐像素对齐。

源 BRD 哈希保持不变，没有保存参考 BRD。Show Element 的 Save 操作最初因报告按钮在主窗口界外失败，没有生成文本报告，也没有将失败计作通过。一次试图关闭报告的 Alt+F4 实际触发主窗口退出询问，已选择 Cancel 保留原窗口，未保存或退出。随后通过已观察的系统菜单最大化主窗口，报告 Close 按钮回到可操作范围；实际关闭报告、F5 后保存稳定画布，再补读 DP2 并保存报告截图，最终用报告 Close 返回画布。无需重启参考程序。

**后续规则**：原生与 Web 均保留两个独立源对象及实心走线，不为这处疑似差异新增空心 shader。后续从已定位的真实对象继续比较局部笔宽、端帽、圆弧及相同倍率抗锯齿；源文字、标签 LOD 和真实同层/跨层混合仍未通过。该局部核对排除了错误实现方向，不能代替四板整体验收。

## 源文字参数与 ANSI 笔画实验（2026-10-04）

在实际 Allegro 25.1 DemoCase 画布执行 Show Element，点选 TOP 的 `i.MX 8M MINI`，通过报告窗口 Save 导出 `.cache/allegro-parity/source-text-imx-allegro.txt`。这是报告另存，未保存 BRD。正式 importer 导出同一对象 `2458560861`，实验不使用人工尺寸替代源数据。

| 字段 | Allegro 报告 | 原生源场景 |
| --- | --- | --- |
| 来源 | ETCH / TOP、TEXT_BLOCK# 3、ANSI | class=6、subclass=0、layer=0、font_index=3 |
| 原点 | (1085.29, 68.77) mil | (27.566366, 1.746758) mm |
| 方向 | 0°、未镜像、左对齐 | 0 rad、false、left |
| 字宽 / 字高 | 38 / 50 mil | 0.9652 / 1.27 mm |
| 字距 / 行距 | 13 / 63 mil | 0.3302 / 1.6002 mm |
| photoplot width | 10 mil | stroke_width=0.254 mm |

坐标和五项尺寸换算误差小于 1e-9 mil。当前差异是字形绘制方式，不是该对象的尺寸解析错误。最新 Web 的 `BoardTextGlyphBuilder` 明确以 MSDF 轮廓替代来源应用的单线字体，现行原生默认方案与它一致；不能靠改 UI 字体、随意缩放或加粗 MSDF 声称 ANSI 已对齐。

`pomelo_render::text::AnsiStrokeFont` 接收调用方明确提供的字节，通过已有 `StrokeGlyphs` 接入源物理尺寸、间距、旋转、镜像和笔宽布局。读取 Width / Height / Decender 和顺序 ASCII 的 pen program，取消、预算、损坏数据复用现有五语结构化字体诊断。实际 `I` / `1` 的轴向提示有三处与明确端点不符：本轮先因严格轴向限制失败，再修复为保留端点并新增回归。未新增 wgpu、修改 GPUI 补丁或复制/嵌入 Cadence 字体。

实验显式读取安装目录 `C:/Cadence/SPB_25.1/share/pcb/text/ansifont.dat`，127 个字符，SHA-256 `6147c7a1b37dd2963ace1bafb0e31085ee496387bd76472dc700bd1d1c5f4919`。真实源文字进入生产 `PreparedTextInstances` / D3D11 HLSL：64 条笔画、4096 字节上传，宽均为 0.254 mm。64 px/mm 下放大首个 M，3810 个内部与 10153 个空白像素通过源线段/物理半笔宽检查，排除距边缘 2 px 内的 AA；稳定重绘不重传，零 opacity 不留墨迹。

实际 Allegro 放大后的首个 M 墨迹范围为 125×175 px（比例 0.714286），实验为 70×97（0.721649）；由外部字体归一化范围和源笔宽计算的物理范围为 1.090534×1.524 mm（0.715574），两者与物理比例均相差不到 2%。参考图是 Show Element 临时黄色高亮，实验为绿色，因此这里只核对局部几何范围。JPEG、倍率不同、一个字形范围不能证明颜色、完整轮廓、全部字符或半透明交点一致。

本轮渲染库 **95 passed、0 failed、15 ignored**；新硬件实验另显式 **1 passed**；全工作区全目标/全特性 Clippy `-D warnings` 通过。日志 `.cache/allegro-parity/source-text-{render-tests,clippy}.log`。源输入、实际报告、参考放大图、GPU PNG、独立核对脚本和文件指纹分别为同目录的 `source-text-imx-input.json`、`source-text-imx-allegro.txt`、`source-text-imx-allegro-zoom.jpg`、`source-text-ansi-gpu.png`、`verify-ansi-source.py`、`source-text-ansi-verification.json`。脚本确认源 BRD SHA-256 未变。

```powershell
$env:POMELO_ANSI_FONT_PATH = 'C:/Cadence/SPB_25.1/share/pcb/text/ansifont.dat'
$env:POMELO_ANSI_TEXT_PATH = "$PWD/.cache/allegro-parity/source-text-imx-input.json"
$env:POMELO_ANSI_PIXELS_PATH = "$PWD/.cache/allegro-parity/source-text-ansi-gpu.png"
cargo test -p pomelo-render --features native-gpu hardware_ansi_source_text --locked --offline -- --ignored --nocapture
```

以上是初始离线实验的证据。随后已接入显式工作台入口，范围及用户决定见下一节；其他 TEXT_BLOCK/font 类型、非 ASCII、完整字形及真实半透明交点仍未验收，四板整体验收仍未通过。

### 显式工作台接线与保留决定（2026-10-04）

用户要求停止继续处理字体，保留当前字体，同时将 ANSI 实验记录下来、已经修改的代码先保留。用户再次确认：ANSI 实验代码、工作台接线、测试及其他已有改动均先留在工作区，不撤回、不删除，不为恢复字体而覆盖这些改动。本阶段没有执行回滚。默认源板文字与自动网络名继续使用既有 MSDF，UI 字体未变；不自动扫描 Cadence 安装目录，不嵌入或分发它的字体。

已经保留的实现：

- `services/source_font.rs` 在后台读取显式提供的字体，限定文件为 1 MiB，保留取消和五语结构化错误。CLI 必须同时传入字体路径与 0–255 的文字块编号；重复、缺失及非法参数拒绝。`font_index` 是 TEXT_BLOCK 编号，不能推断为字体名称，因此调用方须按具体源板确认编号。
- 仅对应文字块中字体完整支持的对象使用源笔画；缺字时整段保留 MSDF 回退，其他文字块不替换。自动网络标签始终走原 MSDF。保留源宽高、间距、旋转、镜像及 photoplot width。
- `PreparedDocument`、`BoardViewport` 与 `BoardFrame.texts` 已接线。源笔画有独立上传统计和准备完成判断，避免两种文字共用计数而一直请求帧；绘图文字携带 owner，拾取数据使用笔画的保守四边形。实窗鼠标、边界误选及全部叠加尚未验证。

以下命令仅供未来明确恢复实验时复验，日常启动无需这些参数：

```powershell
./target/release/pomelo.exe --encoding windows-1252 `
  --source-font 'C:/Cadence/SPB_25.1/share/pcb/text/ansifont.dat' `
  --source-font-block 3 'E:/brd_cases/DemoCase_LPDDR4.brd'
```

本阶段 **501 passed、0 failed、19 ignored** 的工作区回归、Clippy 全目标/全特性 `-D warnings`、格式检查和 Release 构建通过。原硬件测试扩展为实际 BoardRenderer 同时绘制 ANSI 与 MSDF，显式 1 项通过：两套上传计数独立、M 内部保持、global=0 清空与恢复、稳定绘制不重传。硬件源输入仍是实际报告确认的 `i.MX 8M MINI`。

验证副本 SHA-256：`4ae29ed03e888108e335f1209ff004e5b86ddcf2870eba4e1a0dbc3def670062`，隔离配置 `.cache/allegro-parity/source-font-6d00edce994e4a9ebc80dce09c0a1186`。该实例的 `runtime.json` 报告 `ready=true`、9 次提交/呈现、无 GPU 错误：42 个文字块 3 的对象共 2,289 条笔画，上传 146,496 字节；其余 MSDF 源字形 26,770 个。对应源文字 ID `2458560861` 在笔画对象集合中。此证据证明应用准备与呈现链路运行；当时窗口发现工具未返回该新实例，**不声明实窗字形、鼠标拾取或与 Allegro 完整视觉对照通过**。

日志与版本信息：`.cache/allegro-parity/source-font-app-{workspace,hardware,clippy,release}.log`；副本来源见 `source-font-app-launch.json`。其他字体、非 ASCII 回退实窗、半透明笔画交点、混合文字的源顺序及四板完整字形对齐没有完成，按用户决定暂不继续。本记录保留这些边界，后续不得自动将 ANSI 改成默认字体或重新开启字体替换。

## 四板验收矩阵

水平网络名阶段已通过：3 项共享标签回归、旧查看状态兼容/新字段保存、完整工作区 482 项测试（17 项按原有策略忽略）、全目标全特性 Clippy `-D warnings`、格式检查及两个 Python 脚本语法。日志位于 `.cache/allegro-parity/horizontal-{workspace-test,clippy,release-build}.log`。以下另列新版实窗证据，不将局部方向修复冒充 Allegro 字形/字号或四板画面已经一致。

### 水平标签 Release 实窗（2026-10-04）

实际比较二进制 SHA-256：`829284af98c32675a2ae9900bacbfc8cb7b06cda3311594612897bc46333a4f7`。使用独立 `.cache/allegro-parity/horizontal-cba4b6683c3e4689a99cf881ffa4c8a3` 配置目录，原板、Web 源码和日常偏好不修改。DemoCase 单 TOP、焊盘非填充、板文字开启、铜皮 39%，放大到约 619%。

- 新版关闭水平选项时，旋转焊盘内 GND 仍竖排，兼容原行为；开启后同一区域 GND 改为水平，源焊盘与走线不移位。
- 翻板后 GND、VDD_ARM_0V9 等保持水平可读；源走线上的旋转名称仍由走线标签控制，不受此开关替代。
- Ctrl+Q 正常退出后原进程终止，实际 `views.json` 保存 `horizontal_pin_names=true`、翻板、相机及显示设置。
- 重开同一二进制后恢复约 619% / 翻板、水平 GND、39% 铜皮及未填充焊盘；显示面板勾选状态正确。重开 GPU 遥测为 39 提交/39 呈现、`last_error=null`，标签管线/几何管线没有 GPU 错误；不作为 Allegro 像素一致证明。
- 原始无该字段配置成功读取；UI 字体文件哈希保持 `2cb472958b85ea4a32316961c2902b535903697719b9743231743bf1e5c0ed70`。

可访问性观察：`.cache/allegro-parity/horizontal-ui-observations.json`；退出/恢复证据：`horizontal-saved-views.json`、`horizontal-before-reopen-runtime.json`、`horizontal-reopen-runtime.json`。沙箱内旧尝试完成 GPU 提交但没有工具可操作的窗口，另有隐藏方式启动无主窗口的尝试；这些均不作为实窗通过证据。最终使用可枚举、已观察的正常窗口完成上述检查。

### 标签分类的共享契约

`pomelo-core::display::LabelOptions` 保存六个独立布尔字段：`track_names`、`pin_names`、`via_names`、`zone_names`、`thru_labels`、`bb_labels`。`BoardDisplay.label_options` 默认保留原 Web 设置（过孔网络名关闭，其余开启），旧配置缺少整个对象或部分字段时按相同默认读取。渲染库从核心重导出类型，视口直接传入文档选项；不在 GPU 后端保存 UI 选择。

源板文字 `show_texts`、钻孔几何 `show_drills` 和嵌入标签独立；关闭标签不得移除走线/焊盘/过孔数据或改变拾取身份。修改显示快照使现有标签布局缓存重新计算；切换不要求整板几何重新上传。DemoCase 的 Allegro 比较配置需将 `bb_labels=false`，保留通孔跨层标签开启。

本阶段新回归验证六类状态独立保存/旧配置默认、焊盘标签关闭再开启恢复全部实例字段以及查看状态往返；完整工作区 **484 passed、0 failed、17 ignored**，Clippy、格式和 Release 构建通过。日志：`.cache/allegro-parity/label-options-{workspace-test,clippy,release-build}.log`。未运行忽略硬件测试不声明它们本阶段新通过。

最终 Release SHA-256：`2c3d31d18f4a3ebfdf7af23bed960e42a9db8ea7cacc356e96929d7eb5b3faa0`。独立配置 `.cache/allegro-parity/label-options-e8b0b23364c849fcae5de97563ada00f` 中实际验证：

- 六个标准 Checkbox 的名称、默认勾选均可见；原查看配置没有 `label_options`，成功采用旧默认值。
- 关闭通孔跨层标签，局部 `1:8` 全部移除，而孔、焊盘、走线、GND 及铜皮网络名仍在；开启后标签恢复。
- 关闭焊盘网络名，水平 GND 与大焊盘内名称移除；走线名称、通孔标签与铜皮名称继续显示。重新开启后恢复。
- 关闭盲埋孔标签以匹配当前 Allegro 设置，通孔保持开启；DemoCase 本区域无盲埋孔，因此此项只证明设置和保存，不能证明盲埋孔像素。
- 正常退出时实际进程终止，`views.json` 保存六类字段。重开同一版本、进入显示面板，通孔开启/盲埋孔关闭/过孔网络名关闭/其余名称开启状态正确；619% 翻板及水平名称保持。
- 切换阶段 130 提交/130 呈现，GPU 无错误。比较通孔关闭与最终匹配状态的八类几何/板文字管线构建、缓存构建、累计上传字节，全部未增加；只有标签实例和布局缓存变化。不是性能 P95 或逐像素一致验收。

证据：`.cache/allegro-parity/label-options-ui-observations.json`、`label-options-saved-views.json`、`label-options-through-off-runtime.json`、`label-options-matched-runtime.json`、`label-options-geometry-comparison.json`、`label-options-reopen-runtime.json`。六类开关的完整键盘、五语实窗及盲埋孔真实板效果尚未全部验收，四板标签尺寸/LOD 也继续保留待办。

| 案例 | 完整 CPU 导入 | Allegro 实际打开 | 单层几何 | 标签/文字 | 透明度/同层叠加 | 跨层叠加 | 整体结论 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| DemoCase | 通过 | 已打开并读取显示参数 | 比较中 | A04/A05/A08 | A03/A06 | A07 | 未完成 |
| NVL CAMM | 通过 | 已实际打开、定位 R4D1 | Dynamic 11 边/5 弧/面积/0 void 及 Via 坐标/孔径报告匹配；像素待验 | 待验 | 配置校准与局部观察中 | 待验 | 未完成 |
| DDR5 RDIMM ARM | 通过 | 已实际打开并定位 L83/C2324 | 6 份实际报告 53 项匹配；4 项 Via pad 尺寸及其他 pin 报告仍未验证 | 保留当前 MSDF；局部标签开关统一 | 三档局部内点混合及 Dynamic Base 25/26 策略修复通过；AA 相位/覆盖仍待验 | 待验 | 未完成 |
| S5000C | 通过 | 待本轮验证 | U94 真孔洞局部 D3D11 与 23 点 CPU 拾取通过；实际报告/曲边 AA 待验 | 待验 | 9 对象离屏局部通过；实际对照待验 | 待验 | 未完成 |

每次视觉结论应记录同一源文件哈希、实际二进制哈希、显示配置、视角/倍率、比较区域和代表对象。输出上传/呈现且 `last_error=null` 只能证明 GPU 链路运行；不能替代几何、覆盖顺序和文字像素对照。

## 后续执行与共同契约

1. 先完成 DemoCase 单 TOP 的走线、焊盘、过孔和铺铜孔洞几何及标签显示；继续保留线段、圆弧和完整对象身份。当前字体保持 MSDF，ANSI 实验留存；几何 RGB 和来源调色板一致性不作为门槛。
2. 选局部同层重叠区，记录对象身份和 net/layer；分别测试铜皮 0%、99/255、100%，类别不同颜色，填充开/关，源文字与标签开/关。检查真实孔洞应暴露的下层画面，不能用背景色伪装挖孔。
3. 启用 TOP+一个内层、TOP+BOTTOM，再扩到全层；按 Allegro 实际优先级记录 pin/via/trace/shape 与标签覆盖，不把左侧列表顺序直接当作绘制真相。
4. 同一修复版本按顺序打开其余三板，重复代表区域与整板检查；源码编码另以 Allegro 可见源名称/文字核实。
5. 每项修复分别记录共享 Rust 算法、D3D11/HLSL 材质/遮罩及应用显示设置；Web 对应修改放入模型、标签布局、材质管线和设置恢复。不能把 Windows GPU 资源类型带入共享模型。
6. 修复后执行相关 CPU/硬件回归与真实窗口验证。尚未证明的条目继续列出，不把旧 Web 专项零差异解释成 Allegro 已对齐。

原有 Web 对照证据见 [画布 Web 对照记录](canvas-web-parity-validation.md)，当前进展见 [开发进展](development-progress.md)。
