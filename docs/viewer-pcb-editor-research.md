# PCB Editor 文档中的只读 Viewer 能力补充调研

调研日期：2026-10-04。资料版本：Cadence SPB 25.1，September 2025。

**安装后复核：用户随后安装了完整资料，本机现已出现 Getting Started、逻辑数据指南、按字母分类的命令正文与报告目录。以下第 1–9 节保留首次调研的历史证据；其中“核心正文未安装”“仅有索引”的描述已被第 10 节的新检查结果取代，不再代表当前安装状态。**

本文件补充 [能力计划](viewer-review-capabilities-plan.md) 和 [开发设计](viewer-review-development-design.md)。目标是常用 PCB 查看器，而不是复制 Free Viewer 的功能上限或 PCB Editor 的旧界面。此前仅保留“日常审阅闭环”和“工程信息基础”的决定继续有效。本文件提出补充建议，不把候选项自动变成已经批准的开发范围。

## 1. 本地文档覆盖和证据边界

检查了 `C:\Cadence\SPB_25.1\doc`，重点检查 `allegro`、`algcmdref`、`pdv`、`modelinteg`、`algPN`、`minDoc`。

- `allegro/allegro.tgf` 是文档入口映射，设计指南入口转到最小文档 FAQ。
- `algcmdref/algcmdref.tgf` 保留命令/表单索引，但当前有效入口同样转到 FAQ；没有完整命令正文。
- `pdv`、`modelinteg` 的上述检查也只找到 `.tgf`。
- `algPN` 有可阅读的 25.1 新增功能正文，不能把发布说明当成完整功能清单。
- 官方 FAQ 说明默认安装仅包含最小文档集；完整资料可通过 Cadence Doc Assistant 的 My Downloads 下载，需要登录、联网及安装 doc 目录写权限。本次没有下载或修改 Cadence 安装目录。

因此，以下证据分两级：**正文证实**表示可以直接阅读官方行为描述；**索引线索**仅表示本地登记了相关命令，不能据此确认具体交互、版本授权或适用产品。尤其不把 APD 专用能力等同于 PCB 能力。

## 2. 正文证实的能力及 Viewer 取舍

| 官方能力 | 证据与边界 | Viewer 应保留的部分 | 对现有设计的影响 |
| --- | --- | --- | --- |
| Search Panel | `algPN/Docked_Informational_Panels.html`：查询数据库对象，面板与画布交叉定位，按需加载，列过滤和列重排 | 对象表格、字段过滤、按需加载、双向定位 | 加强已有属性查询 B2；不能只做名称搜索和一条详情 |
| Reports Panel | 同一正文：报告可直接用于审阅和画布导航；列举的是铜岛、无网络铜形、缺泪滴/渐变、悬空走线与过孔等报告 | 借鉴“列表条目可定位对象”的交互模式 | 所列诊断报告属于此前排除范围，不加入本期；基础清单可以使用此交互模式 |
| 属性解释与 Smart Search | `algPN/Smart_Search_for_Properties_and_User_Preferences.html`：属性说明、关键词/问题预测搜索；原功能用于 Edit Property 和设置 | 属性描述、单位、适用对象、名称/别名关键词检索 | 补充 B1/B2 的可理解性；不提供属性写入，不要求 AI 问答或联网预测 |
| OpenType 字体 | `algPN/OpenType_Fonts_Support.html`：设计文本支持字体、样式、高度、旋转、镜像；与旧 Text Block 并存 | 正确显示源文件已有文本与字体信息，保留旋转/镜像，明确字体替代 | 新增显示兼容性验收；UI 字体或 MSDF 字形图集不等于支持 PCB 源字体 |
| Positive Mask | `algPN/Positive_Mask_Support.html`：传统可见图形代表阻焊开口；新增正向阻焊覆盖形状，涉及专门 class 和动态生成 | 区分“阻焊开口”和“阻焊覆盖”，读取已有数据；可选只读合成预览 | 新增显示语义建议。不要在 Viewer 执行 Cross-section 设置或生成并写入动态形状 |
| 大板查看性能 | `algPN/Performance_Improvements.html` 明确列出 Report、Graphical Display、Show Element、Display 等性能改进，未给数值 | 检索、属性与显示不阻塞，结果虚拟化，取消/过期响应保护 | 延续已有开发设计，不能引用官方未提供的性能数值作为目标 |

## 3. 常用 Viewer 功能清单与优先顺序

下表的“建议”是针对 PCB Viewer 工作流的产品判断，不是声明这些全部已由本地 PCB Editor 正文证实。前期代码对照仍有效；本次补看发现桌面名称搜索调用有 20 条结果限制，因此完整清单必须明确区分结果分页与仅返回前若干条。

| 优先级 | 用户任务 | 应提供的只读能力 | 与已有计划的关系 |
| --- | --- | --- | --- |
| P0 | 找器件、引脚、网络、过孔或文本 | 多对象类型检索；字段过滤；可排序/重排列；表格与画布双向定位；完整结果计数、分页或虚拟化 | 强化 B2，不只增加搜索框 |
| P0 | 理解选中对象 | 可读属性、来源、单位、属性说明；从引脚跳所属器件/网络，从对象跳层/焊盘定义 | 强化 B1/B4，属性关系链接不改变源设计 |
| P0 | 看清复杂重叠关系 | 单独查看目标网络/器件及其相关层；淡化上下文；一键恢复原显示；活动层快速切换 | 补充 A4 的临时隔离；区分高亮、隐藏、淡化，复用 A5 显示状态 |
| P0 | 测量和跟线 | 点距、对象边缘间距、路径与网络关系、真实源连接线、差分对成组查看 | 已有 A1/A2/B5/B6/B7；不增加规则校验、阻抗或延迟计算 |
| P0 | 看焊盘和过孔 | 定义与实例分开；逐层铜焊盘、反焊盘、热焊盘、钻孔/槽孔、镀孔、起止层与背钻；局部预览 | 强化 B4，未知字段明确显示，不用几何猜定义 |
| P1 | 看层的实际用途 | 按 class/subclass 分类；铜、阻焊开口、锡膏、丝印、装配、外形、钻孔分别控制；读取真实物理叠层 | 延续 A5/B3；绘图层不冒充物理叠层 |
| P1 | 理解阻焊和丝印 | 明确阻焊图形是开口还是覆盖；保留已有正向形状；文本字体、镜像、旋转和样式兼容 | 本次新增重点，属于显示保真，不涉及编辑 |
| P1 | 查工程清单 | 器件/引脚/网络/过孔/焊盘使用清单；有来源的孔径、孔型、镀孔与起止层汇总；点击汇总下钻到实例；CSV 导出 | 从 B1/B2/B4 派生，区分源事实清单与 DRC/完成率诊断 |
| P1 | 快速到指定位置 | 坐标跳转、缩放到选择、前后视图、命名视图；全板概览可作为候选 | A6 已覆盖部分；坐标跳转只移动相机，不修改数据库原点 |
| P2 | 对比两版板的变化 | 器件/网络/几何差异清单，变化定位；明确对齐变换和匹配置信度 | 新独立候选，工作量较大，需确认范围；不做合并、回写或 DRC |

P0 表示优先完善原两阶段。P1 适合在两阶段中补充细节或作为小项追加。P2 不自动排入现有实施顺序。

## 4. 命令索引中的进一步核实线索

来源统一为 `C:\Cadence\SPB_25.1\doc\algcmdref\algcmdref.tgf`。这些是索引发现，不能作为完整功能规格。

| 索引项 | 待核实的 Viewer 行为 | 建议 |
| --- | --- | --- |
| `find_by_name`、`find_by_query`、`findprop` | 名称、条件与属性检索范围 | 已有 B2，无须复制 Allegro 查询界面 |
| `show_element`、`show_property`、`show_measure` | 对象详情、属性、测量字段 | 已有 B1/A1/A2，等待完整手册补字段语义 |
| `advanced_highlight`、`assign_color`、`layer_priority`、`colorview_*` | 多目标强调、颜色与层优先、保存显示配置 | 复用 A4/A5/A6；不要假设 assign_color 的持久化方式适合只读 Viewer |
| `rats_component`、`rats_layer`、`rats_net`、`rats_end_inview` | 按器件/层/网络筛选连接线及视野相关显示 | B6 可细化筛选，依赖源连接数据，不推导“未布线完成率” |
| `padstack_drillreport` | 孔与焊盘的基础清单 | 候选只读汇总，报告字段和计数口径待核实 |
| `zoom_selection`、`zoom_previous`、`zoom_world` | 缩放到对象、视图历史、全板概览 | A6 与导航候选；不据此推定特定小地图 UI |
| `layer_compare`、`batch_layer_compare`、`layer_diff_walker` | 层比较及差异逐项定位 | 支持进一步研究版本比较；当前无正文，暂不声明具体算法和授权 |

## 5. 推荐的新增验收细节

1. 查询 20 条以上结果时，能看到真实总数或明确的范围状态，继续浏览剩余结果；不把截断列表当成完整清单。表格列过滤和画布反向定位使用稳定对象 ID。
2. 临时隔离前保存显示状态；恢复后层可见性、颜色和透明度一致。无目标或目标位于隐藏层时给明确反馈，不能静默永久修改用户预设。
3. 属性说明来自可追溯的词典或文档；没有解释时保留源名称与原值，不编造业务含义。检索属性名称、别名和说明不依赖云端服务。
4. 阻焊开口和覆盖采用明确模式标签。合成覆盖需要有效板外形和完整开口；边界缺失时降级为开口查看并提示覆盖率。导入已有正向形状和自行合成预览要区分来源。
5. 字体缺失时显示替代说明；验证特殊符号、中文字形、旋转、底面镜像。PCB 字体数据、UI 字体和字体图集分别处理。
6. 工程清单汇总可以下钻定位原对象；过滤范围、单位、孔型和镀孔区分写入导出表头。缺少镀孔标志时显示未知，不能凭孔形判断。
7. 版本比较若后续纳入，必须先定义对齐、底面变换、稳定身份和容差；区分对象属性变化与纯几何差异，不用不同渲染截图替代设计比较。

## 6. 不纳入本期的 Editor 功能

编辑、放置、布线、修改属性/原点/叠层、铜皮重填、生产文件生成与源板回写不纳入。此前排除的 DRC/约束/完成率与铜岛诊断、3D、标注协作、脚本宏、分屏也继续排除。属性浏览、叠层浏览、连接线浏览可以保留；不因为其数据来自编辑器而引入编辑命令。

Search Panel 是本次直接证实且最值得补充的交互能力；阻焊语义与源字体是本次直接证实的显示保真缺口。孔/焊盘清单、隔离查看、坐标跳转属于常用 Viewer 产品建议；版本比较属于独立待确认候选。完整 PCB Editor 手册缺失，使本次无法给出覆盖全部 Editor 只读能力的穷尽结论。本次未完成软件实测，不把索引或设计建议记成实测结果。

## 7. 线上官方资料补查

用户要求本地正文缺失时继续线上查找。本节已补查 Cadence 官方网站与官方技术博客。旧版文章证明相关能力曾存在，不代表已经核实 25.1 的所有授权和菜单；APD 文章明确单列，不能直接作为 PCB Editor 的功能规格。

| 能力 | 线上证据 | Viewer 补充要求 |
| --- | --- | --- |
| 保存查询、复用查询、导出结果 | [Find by Name / Find by Query，2020-09-08](https://community.cadence.com/cadence_blogs_8/b/pcb/posts/find-by-name-or-find-by-query?pifragment-2199=588)：按字段及文本/数字条件查询，结果定位，保存/载入查询，CSV/XML 导出 | B2 增加查询模板与 CSV 结果导出；模板保存条件、字段及单位，不保存过期对象 ID；加载后重新执行，不沿用旧结果。编辑示例不纳入 |
| 板内 film 显示视图、网络可见性规则 | [Text Labels and Film Views，2019-06-11](https://community.cadence.com/cadence_blogs_8/b/pcb/posts/text-labels-and-film-views-help-intelligent-designers)：已有 film 可驱动画布可见性，另有颜色视图与网络/连接线显示规则 | A5/A6 增加只读读取源 film 配置作为显示预设的候选；源预设与用户自建预设分开。没有源 film 时不伪造“原厂视图” |
| 过孔跨层标签 | 同一官方文章描述过孔 span 和堆叠过孔标签区别 | A5/B4 补充起止层标签；只显示源已知跨度。投影重叠不能证明堆叠过孔，不能凭外观合并 |
| 双单位、跨层几何测量 | [PCB Editor Show Measure，2014-05-13](https://community.cadence.com/cadence_blogs_8/b/pcb/posts/what-s-good-about-allegro-pcb-editor-show-measure-for-dual-units-16-6-has-it?pifragment-3431=664)：主/备用单位，允许不同层对象的测量 | A1/A2 增加 mm/mil 双单位与显式“平面投影距离”模式；不能把跨层投影距离标为同层铜间距或三维空间距离 |
| 背钻事实浏览 | [Enhanced Backdrill，2016-09-26](https://community.cadence.com/cadence_blogs_8/b/pcb/posts/why-move-up-to-allegro-17-2-2016-new-enhanced-backdrill-capability-reason-4-of-10)：对象详情、背钻孔尺寸、禁止切割层、深度与跨层说明 | B4 展示源记录中已有的这些字段；字段缺失保持未知，不能把钻孔直径或板厚当背钻定义；不运行背钻分析 |
| 逻辑与物理比较分开 | [Comparing Design Versions，2020-12-15](https://community.cadence.com/cadence_blogs_8/b/pcb/posts/comparing-design-versions)：APD 中区分网表/器件清单比较、IPC-2581 物理比较与逐层差异定位；文章含产品选件和格式限制 | 为 P2 提供交互参考；仍需单独核实 PCB 25.1 授权和数据覆盖，不复制写比较结果数据库、豁免或联动分屏流程 |
| 原理图与 PCB 双向定位 | [CAD Tool Cross-Probing，2020-04-06](https://resources.pcb.cadence.com/schematic-capture-and-circuit-simulation/2020-cad-tool-cross-probing-in-pcb-design)：对象选择在原理图与布局间对应 | 适合作为后续集成候选，需要原理图数据和身份映射；不是仅靠 BRD 就能完整实现，也不在当前两阶段隐含加入原理图编辑器 |

额外找到本地补充资料 [Allegro_Find_by_Query.pdf](C:/Cadence/SPB_25.1/share/pcb/help/Allegro_Find_by_Query.pdf)，描述 16.6 的原型版查询：视野内过滤、多条件筛选、批量选择及保存/载入。其所在目录的 README 明确说明为尚未纳入正式文档的补充帮助。它能作为行为参考，不能替代 25.1 正式指南；尤其不照搬原型的 AND/OR/NOR 特殊解释到新的查询语言。此前 A2 的“同物理层铜间距”仍有效，跨层测量必须新增独立模式，不能放宽原有语义。

新增测试建议：查询模板跨板加载验证字段支持和单位；导出覆盖全部匹配结果或明确导出选中范围；film 引用缺失层时显示部分覆盖；双单位换算使用同一原始 mm 值；跨层投影测量在画布、面板和导出中都带模式说明。

## 8. 完整文档获取方式与本地路径

线上与本地资料一致：完整离线文档的默认目录就是安装目录下的 `doc`，本机对应 `C:\Cadence\SPB_25.1\doc`。目前不是简单找错目录，而是核心手册正文没有安装。官方 [Doc Assistant 说明](https://www.cadence.com/en_US/home/support/doc-assistant.html) 确认在线从云端读取、离线从安装目录读取；[24.1 文档获取说明](https://community.cadence.com/cadence_blogs_8/b/pcb/posts/accessing-documentation-in-cda) 另说明可使用独立 Product Documentation 安装包。

可用获取路径：

1. 从 Windows 开始菜单的 Doc Assistant 或 PCB Editor 的 Help 进入；开启在线文档，选对应 PCB 产品和 25.1 release，查设计指南、命令参考、属性参考和 Padstack 文档。
2. 如需离线，使用 My Downloads。按本机 [My Downloads 指南](C:/Cadence/SPB_25.1/doc/cdadoc/My_Downloads.html)，需要联网、登录、doc 写权限，且当前在线 release 与默认离线 release 一致；可以在 Libraries 的 Default Doc 上 Make Current，再 Download and Install Update。
3. 如需单独安装包，前往官方 [Downloads](https://downloads.cadence.com)，查 Product Documentation 对应 release；本次没有下载登录后的包，不能保证具体 25.1 安装包名称或文件大小。
4. 如果其他安装盘或版本已经有完整手册，可在 Doc Assistant Libraries 中检查路径。本机 [Loading Document Libraries 指南](C:/Cadence/SPB_25.1/doc/cdadoc/Loading_Document_Libraries.html) 支持 Windows `CDA_DOC` 用分号指定多个 doc 目录；本次没有修改环境变量或 library 设置。

应寻找的标题/索引入口包括 Allegro PCB Editor 设计指南、Allegro PCB and Package Physical Layout Command Reference、属性参考，以及 Padstack Designer。`allegro.tgf` 和 `algcmdref.tgf` 已提供入口线索，完整包安装后应出现实际 HTML/PDF 正文；不要仅凭目录存在就认为手册已经齐全。本次使用可公开访问的官方文章补查，没有取得登录后的完整 25.1 手册，仍保留完整覆盖的限制。

## 9. 本地官方来源

- [默认最小文档集说明](C:/Cadence/SPB_25.1/doc/minDoc/Why_do_I_not_see_all_the_documentation_for_the_installed_products_.html)
- [完整文档下载说明](C:/Cadence/SPB_25.1/doc/minDoc/I_am_only_intermittently_online_but_I_want_access_to_documentation_at_all_times._How_do_I_download_the_documentation_.html)
- [PCB Editor 文档入口](C:/Cadence/SPB_25.1/doc/allegro/allegro.tgf)
- [命令与表单索引](C:/Cadence/SPB_25.1/doc/algcmdref/algcmdref.tgf)
- [Search/Reports Panel](C:/Cadence/SPB_25.1/doc/algPN/Docked_Informational_Panels.html)
- [属性说明及搜索](C:/Cadence/SPB_25.1/doc/algPN/Smart_Search_for_Properties_and_User_Preferences.html)
- [正向阻焊](C:/Cadence/SPB_25.1/doc/algPN/Positive_Mask_Support.html)
- [OpenType 字体](C:/Cadence/SPB_25.1/doc/algPN/OpenType_Fonts_Support.html)
- [性能改进](C:/Cadence/SPB_25.1/doc/algPN/Performance_Improvements.html)

## 10. 新安装完整文档后的复核结论

重新检查同一路径后，确认 `allegroTOC.html`、`algrostart` 用户指南、`fcoms/scoms/rcoms` 命令正文、`algrologic` 逻辑数据指南、`xcoms` 叠层说明及 `lcoms` 层比较正文已安装。阅读的是 25.1 September 2025 的具体章节，不再依赖旧版博客或命令名字推断。

### 10.1 直接确认、适合常用 PCB Viewer 的能力

| 能力 | 完整手册证据 | 当前实现/前期计划 | 建议落实方式 |
| --- | --- | --- | --- |
| 表格式对象查询 | [Search Panel](C:/Cadence/SPB_25.1/doc/algrostart/Search_Panel.html)：表格全局搜索、列筛选、列排序/重排/自动宽度、范围/非连续选择、画布定位 | 现有名称搜索与结果列表不能覆盖该完整工作流；B2 已规划查询基础 | 完整结果表格、类型化过滤、复制单元格、批量选择及定位；不复制旧式窗口 |
| 保存查询和导出 | [Find by Query Dialog Box](C:/Cadence/SPB_25.1/doc/fcoms/Find_by_Query_Dialog_Box.html)：AND/OR、匹配总数、字段配置、延后选择、保存/载入、重跑、CSV/XML | 原两阶段未完整约定模板与导出契约 | 保存查询 AST/字段/单位，不保存对象 ID；结果分页，导出明确范围。批量选择由显式操作触发，避免浏览行就破坏当前选择 |
| 网络级显示规则 | [Using the Nets Grid](C:/Cadence/SPB_25.1/doc/algrostart/Using_the_Nets_Grid.html)：网络下的 pins/vias/clines/shapes/rats 分别着色，XNet 子网络继承和实例覆盖 | 已有网色、层色是基础；A4/B5 未完全细化继承 | 保留用户显示配置；网络组、网络、对象类别、实例按明确优先级解析；不要将颜色写入 BRD |
| 聚焦与淡化 | 同一章：Shadow Mode 淡化非高亮内容，活动层是否淡化可控制 | 已规划高亮，缺乏明确聚焦闭环 | 独立聚焦显示状态，可调上下文亮度并恢复；亮度不能当透明度，也不能影响导电图计算 |
| 每层类别开关、逐层查看 | [Visibility Panel](C:/Cadence/SPB_25.1/doc/algrostart/Visibility_Panel.html)：每层 etch/pin/via、mask、单层与多层选看、多 stackup | 桌面有层开关但完整类别矩阵不足；Web 有部分全局类别开关 | 按层与类别组合显示，单层/若干层聚焦；stackup 有来源时分区显示，缺失时保留全局层模式 |
| 源 film 与自建视图 | [colorview create](C:/Cadence/SPB_25.1/doc/ccoms/colorview_create.html)：显示设置集合、当前设计的 film visibility | A6 命名视图已有设计；源 film 未纳入契约 | 区分源只读 preset 和用户 preset；film 仅作为可见性配置，不生成 Gerber |
| 更完整对象详情 | [show element](C:/Cadence/SPB_25.1/doc/scoms/show_element.html)：坐标/线段顶点/长度、圆弧圆心半径、符号/位号、属性、user schedule、背钻 | 当前 inspector 为基础，B1/B4/B6 需要扩展 | 几何字段与工程属性分组；路径/来源链接可定位；外部 URL 仅明确点击打开 |
| 测量结果与路径引导 | [Measure Dialog Box](C:/Cadence/SPB_25.1/doc/scoms/Measure_Dialog_Box.html)：点距/累计/Manhattan/Dx/Dy/角度、线宽、中心线路径长度/累计、过孔数、Air Gap、公共 subclass | A1/A2/B7 基本覆盖；新增局部宽度和单位字段 | 点距、最小边缘距离、沿路径长度分开输出；中心线图和占用区域不能混用 |
| 经过指定点的路径 | [Measuring Distance](C:/Cadence/SPB_25.1/doc/scoms/Measuring_Distance_between_Two_Points_in_the_Design.html)：多路径时找到其中一条，可通过中间点指示另一条 | 原 B7 已有途经点，需明确不把 Allegro 行为称为最短路径保证 | Pomelo 自己定义最短已知路径及途经点；只有图覆盖完整才声明最优；选中路径高亮，旁路不算入长度 |
| 网表和器件逻辑比较 | [Comparing Netlists](C:/Cadence/SPB_25.1/doc/algrologic/Comparing_Netlists.html)：多来源网表、基线/差异树、对应器件/引脚画布定位 | 本期没有设计比较；此前仅为候选 | 后续可做器件、引脚归属、网络成员、源属性的只读差异清单；不据此承诺几何比较或引入新格式 |
| 板主体 fit | [Viewing a Design](C:/Cadence/SPB_25.1/doc/algrostart/Viewing_a_Design.html)：全图与排除图框/图例的全板查看、区域/选择/历史缩放 | 已有 fit 和选择定位；板主体与绘图全集不应混同 | 保留两种 fit 范围；优先有效板外形，缺失时说明降级到几何范围 |

### 10.2 工程清单应成为一级功能

[List of Available Reports](C:/Cadence/SPB_25.1/doc/rcoms/List_of_Available_Reports.html) 已直接证明下列基础报告。它们适合 Viewer，且无需执行 DRC 或修改设计。

| 清单 | 必要字段与联动 |
| --- | --- |
| 器件/BOM | 位号、器件类型、封装、值/容差、坐标、角度、镜像；支持明细与按有来源的类型汇总，不凭显示名称合并不同物料 |
| 器件引脚/符号引脚 | 位号、引脚号、坐标、焊盘定义、网络；从清单定位引脚、网络和器件 |
| 网表 | 网络与引脚成员关系，明确孤立对象与缺失逻辑数据；不借此运行连通性合规检查 |
| 焊盘定义/使用 | 定义列表以及使用它的符号引脚/实例；可从定义下钻到全部实例 |
| 背钻 | 源起始层、must-cut-layer、manufacturing stub 等；与 must-not-cut 字段分别保存，不能互相替代或猜测 |
| 槽孔 | 中心坐标、定义的 Size X/Y、起止层、镀孔、继承旋转；定义尺寸为零旋转、未镜像尺寸，与变换后包围盒分开 |
| 网络过孔 | 网络、实例数、通孔/盲埋孔类型、定义名，以及按层明细；跨层同一过孔只算一个实例 |
| 走线长度 | 按网络/层/宽度聚合中心线长度；指定引脚对使用路径长度。铜皮不凭面积换算长度；source/net coverage 不全时给部分结果 |
| 网络属性 | 属性键、类型、单位、原值、网络及来源；与 B1 属性浏览使用同一事实模型 |

官方 `Etch Length By Pin Pair` 报告会排除部分电源地或大引脚数网络，不能将其条件未经说明搬进 Pomelo。官方 Film Area 的金属比例涉及板外对象与 route keep-in 特殊口径，不直接复制为“板内铜覆盖率”。源数据清单与完成率/失效诊断分开，后者继续排除。

[reports](C:/Cadence/SPB_25.1/doc/rcoms/reports.html) 确认报告支持文本、CSV、HTML 输出。Pomelo 建议先实现表格/CSV，不需要复刻多个报告浮窗或 Allegro 格式文件。

### 10.3 需要更正或谨慎处理的细节

- [layer compare](C:/Cadence/SPB_25.1/doc/lcoms/layer_compare.html) 与 [layer compare batch](C:/Cadence/SPB_25.1/doc/lcoms/layer_compare_batch.html) **明确仅限 APD**，并把结果写入目标层/参考数据库。它们不作为 PCB Editor 的已证实只读能力。版本几何比较继续作为 Pomelo 独立候选，当前不实施。
- `rep padstack` 是 replace padstack 的编辑命令，不是 report；报告使用正式报告目录里的 Padstack Definition/Usage，不靠命令缩写推断。
- Allegro 的 Measure Dx/Dy 是绝对量；Pomelo 原设计定义有符号 ΔX/ΔY。保留有符号差值以便判断方向，可附加绝对量，但在字段和导出里写清定义。累计点距与累计路径长度分开。
- [Cross-Section Controls](C:/Cadence/SPB_25.1/doc/xcoms/Cross-Section_Editor_Dialog_Box_Controls.html) 中 **Display padless holes** 是显示辅助；**Dynamic unused pads suppression** 会增删源焊盘，属于编辑。Viewer 可看无焊盘孔及源抑制状态，不能运行抑制算法或把无铜孔自动画成有铜焊盘。
- 同一叠层章直接确认物理层类型、层用途、厚度及公差、材料、fill-in material、Layer ID、总厚度/不含 mask 总厚度和 Drill Chart。只读展示源事实；阻抗、SI what-if 和规则关联编辑仍排除。多 stackup 必须保留来源与区域，不拼成一个假叠层。
- [Using Data Browsers](C:/Cadence/SPB_25.1/doc/algrostart/Using_Data_Browsers.html) 说明源 Quickview 可能是简化预览，焊盘只提供文本。当前缩略图不应被当作完整板数据；Pomelo 自己的焊盘局部预览可优于这个限制。

### 10.4 实施结论

以原 A1–A7/B1–B7 为主线，优先把查询表格、属性关系、测量、网络聚焦、层类别控制、焊盘/叠层和工程清单做成完整闭环。查询模板、源 film、无焊盘孔显示、板主体 fit 是同类常用查看能力的细化；具体执行契约见开发文档新增补充节。

本次已核实核心只读章节，不声称逐页读完全部 SPB 的编辑/仿真文档，也不声称做过软件功能实测。初次缺正文的限制已解除；样板数据解析覆盖和当前程序运行结果仍需开发时验证。
