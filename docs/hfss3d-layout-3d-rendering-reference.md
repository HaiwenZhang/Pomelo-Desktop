# HFSS 3D Layout PCB 三维显示功能调研

调研日期：2026-10-18。桌面核查版本：Ansys Electronics Desktop 2025 R2（v252），活动编辑器为 HFSS 3D Layout 的 Layout。配套实施方案见 [PCB 三维显示开发设计](pcb-3d-rendering-development-design.md)。

## 1. 范围和结论

这里的“三维渲染模块”指 PCB 结构的显示、视角操作、层与对象过滤、选择反馈、剖切、辅助图形及图像输出。网格和场结果只记录其可视化能力；不包含求解器实现、端口创建、布线、铺铜、布尔编辑、DRC、热分析或设计文件写回。

HFSS 3D Layout 的 Layout 本身就是三维显示环境，默认俯视看起来像二维。它的核心是**按物理层叠组织的工程查看**：导体、介质、孔及元件按高度关联，再结合显示模式、可见性矩阵和选择状态观察内部结构。不能把任意 HFSS 3D Modeler 的菜单都算作当前 Layout 的功能。[Layout Editor Display][s01]

本文以当前界面和可访问的官方资料形成有边界的功能目录。标记“资料确认”的功能没有全部在当前板上执行；未提供数据的网格、场、弯折和外部三维组件无法由当前画面验收。本文不声称覆盖所有许可证、技术环境、插件及版本组合。

## 2. 证据与核查方式

| 标记 | 含义 |
| --- | --- |
| U | 本次在 2025 R2 实窗中看到入口、选项或画面；看到入口不等于完成行为验收 |
| D | 官方帮助或官方 API 确认；以链接对应的版本与编辑器为准 |
| P | 为 Pomelo 提出的实现选择，不是 Ansys 已验证行为 |
| ? | 当前窗口和已读资料不足以确认 |

桌面观察记录经过概括，不保存实际设计名称、网络列表、设计截图、源文件路径或原始 UI 树。

| 观察编号 | 核查表面 | 已确认内容 | 核查深度 |
| --- | --- | --- | --- |
| U01 | 初始 Layout | 倾斜三维 PCB、孔、多层几何、网格和方向三轴；Layers、Nets、Components、Properties 面板 | 画面观察 |
| U02 | View 菜单 | Display Mode、Rotate、Zoom 系列、Fit All、Fit Selection、Pan、Stretch Z、Reset Orientation、Faster Transformations、Animate、Clip Plane | 菜单展开；未选中时 Fit Selection 灰显 |
| U03 | Display Mode 子菜单 | Default、Sketch、Solid；初始 Default 被选中 | 子菜单展开 |
| U04 | Stretch Z | Set Layer Thickness Scale Factor 滑块、倍率、Dynamic、OK、Cancel | 打开对话框；观察到倍率改变时层间距与孔段被拉伸；最终取消，回到原画面 |
| U05 | View 功能区 | Grid、Text、HFSS Extents、Ports/Sources、Package、Add Clipping Plane、Animate、Copy Image、Layout Settings；Push Down/Pop Up 当前灰显 | 控件清单 |
| U06 | Orient 下拉 | Top、Bottom、Right、Left、Front、Back、Trimetric、Dimetric、Isometric | 展开，未改变观察方向 |
| U07 | Color by 下拉 | Layer、Net | 展开，未更改 |
| U08 | Layout Settings > Object | connection points、顶点标签、Color by net、package graphics、rats、非功能内层焊盘抑制、三种 antipad 显示选项、测量单位与有效位数 | 打开页面，未更改 |
| U09 | Layout Settings > Display | 背景、主/次网格间距、网格开关、三种绘制模式、Drawing extent 与 Fit to contents | 打开页面，未更改 |
| U10 | Layout Settings > Selection | 首选/追加选择颜色、未选对象透明度、选中线框实体化、移动光标预选 | 打开页面，未更改 |
| U11 | Layout Settings > Advanced | 按缩放动态调节圆弧精度、固定精度、内部距离容差 | 打开页面，未更改 |
| U12 | Layout > Layers | Laminate、信号/介质行、材料、介质填充、厚度、透明度列、逐层显示复选框、Stackup Viewer | 打开查看后关闭；未 Apply |
| U13 | Layout 菜单 | Draw HFSS Air Box、Draw Ports/Sources、Import/Export、3D Component Definitions、Grid settings | 菜单展开；未执行导出或建模 |

核查结束时已关闭本次打开的参数窗口；没有保存工程、运行仿真或导出用户设计。初次观察时工程已有未保存标记，该标记不用于判断本次是否修改设计。

## 3. 功能目录

下面的编号同时用于开发文档的功能映射。每个编号代表一项可交付能力，便于拆任务和验收。

### 3.1 视角和导航（V01—V09）

| 编号 | 能力 | 入口或交互 | 证据与边界 |
| --- | --- | --- | --- |
| V01 | 自由旋转 | Rotate；当前菜单提示 Alt+Drag | U02、D [s01][s01]；旋转观察方向，不旋转源 PCB |
| V02 | 平移 | Pan；当前菜单提示 Shift+Drag | U02、D [s01][s01] |
| V03 | 连续及步进缩放 | Zoom、Zoom In/Out；旧式组合为 Shift+Alt 拖动 | U02/U05、D [s01][s01] |
| V04 | 区域缩放 | Zoom Area | U02；框定局部观察范围 |
| V05 | 适应全部 | Fit All，当前菜单 Ctrl+D | U02/U05 |
| V06 | 适应选择 | Fit Selection / Fit Selected | U02/U05、D [s05][s05]；依赖有效选择 |
| V07 | 上一个缩放视图 | Zoom Previous | U02/U05；历史的完整字段与容量未测 |
| V08 | 标准方向 | 六个正交方向及三种轴测方向 | U06；另有 Alt 双击屏幕区域的方向吸附，[s01][s01] |
| V09 | 重置方向 | Reset Orientation | U02；恢复标准俯视方向，[s02][s02] |

快捷键是可配置行为。当前菜单显示旧式组合不意味着这是所有安装的唯一操作方式；2024 R1 以后还存在中键导航方案。实现兼容操作时应按命令映射，并在目标安装核对，而不是硬编码一套手势。[Layout Shortcuts][s20]

### 3.2 显示模式和外观（R01—R09）

| 编号 | 能力 | 行为 | 证据 |
| --- | --- | --- | --- |
| R01 | Default | 采用层叠的透明度和显示设置 | U03/U12、D [s02][s02] |
| R02 | Sketch | 轮廓/近似线框方式观察结构 | U03、D [s02][s02] |
| R03 | Solid | 实体显示；仍受逐层线框设置约束 | U03、D [s02][s02] |
| R04 | 逐层线框或填充 | 同一画面可保留某些层的轮廓，其他层填充 | U12、D [s04][s04] |
| R05 | 逐层透明度 | 百分比控制，0 为不透明、100 为全透明；支持多行调整 | U12、D [s04][s04] |
| R06 | 图层颜色 | 同层对象使用图层颜色 | U12、D [s11][s11] |
| R07 | 按网络着色 | Color by > Layer / Net | U07/U08、D [s08][s08] |
| R08 | 层填充图案 | Pattern；影响 Default 的视觉表达 | D [s04][s04]；本次未打开隐藏的 Pattern 列 |
| R09 | 曲线绘制精度 | 随缩放动态调节，或使用固定精度 | U11、D [s10][s10]；内部距离容差不是同一设置 |

显示结果不能只由一个全局模式枚举决定。逐层线框和显示矩阵仍有作用；Solid 下不应期待层透明度呈现出与 Default 一样的效果。官方可见性课程提供了混合模式例子，但课件针对 2023 R1，2025 R2 以本次菜单及对应帮助为准。[Viewing and Visibility][s18]

### 3.3 图层、网络、元件和视图过滤（L01—L11）

| 编号 | 能力 | 行为及数据依赖 | 证据 |
| --- | --- | --- | --- |
| L01 | 逐层显示、隐藏及单层隔离 | 包含 Show This Layer Only、信号层和介质层的批量开关 | U01/U12、D [s12][s12] |
| L02 | 图层 × 图元类别矩阵 | Shapes、Lines、Pads、Holes、Components 分别控制 | U01/U12、D [s03][s03] |
| L03 | 整列批量开关及 Fill/Unfill | 面板列头对同一类别批量生效 | U01、D [s03][s03] |
| L04 | Show Dielectrics | 显示/隐藏介质层，并影响孔的显示组织 | U01、D [s03][s03] |
| L05 | 孔段显示 | 介质隐藏时可连续显示于最上/最下可见信号层之间；介质显示时逐层控制 | D [s03][s03]；不要将显示连续性解释为制造孔跨度 |
| L06 | 网络显示、隐藏和隔离 | Show、Hide、Show All、Show (Hide All Other) | U01 有面板；行为 D [s05][s05] |
| L07 | 网络搜索与分组 | 正则过滤，网络类、Differential、Extended Nets 提供成员组织 | U01、D [s05][s05]；这里只记录查看成员和定位，不包括创建电气分组 |
| L08 | 元件分类、选择和定位 | R/L/C/IC/IO/Other；Fit Selected | U01、D [s06][s06]；分类不是三维封装模型 |
| L09 | 命名可见性视图 | 保存/恢复层与网络显示组合；Surface 为表层预设 | D [s03][s03]；不据此推断保存完整三维相机 |
| L10 | Zone Filter | 多区域层叠下按区域查看可用层 | U01 有入口；多区域行为 D [s03][s03] |
| L11 | 非物理层过滤 | 轮廓、符号、后处理等辅助层与物理层区别管理 | U01/U12、D [s04][s04] |

### 3.4 三维结构表达（S01—S07）

| 编号 | 能力 | 要表达的结构 | 证据与限制 |
| --- | --- | --- | --- |
| S01 | 物理层叠 | 导体/介质的厚度、高度、材料和区域关系 | U12、D [s04][s04]；支持 Laminate、Overlapping、Multizone |
| S02 | 焊盘堆叠、孔和避让显示 | pad/hole 分层；非功能内层焊盘抑制与 antipad 的三种条件 | U08/U12、D [s08][s08]；这些选项也涉及源模型语义，不能当作任意修改几何的许可 |
| S03 | 负片层、空洞与介质填充 | 正/负几何、铜空洞、层内介质填充 | U01 见孔洞；D [s04][s04]；存在设置不表示当前视图等于最终求解体 |
| S04 | 元件封装图形 | 包络/轮廓、高度，资料中还描述散热器基座和鳍片 | U05/U08 有 Package；D [s13][s13]；封装定义主要服务热/机械流程，不等于电磁求解几何 |
| S05 | 外部三维组件和层级 | 3D/Layout 与三维组件；层级进入/返回 | U01/U05/U13 有入口；D [s19][s19]；当前板未验证外部组件；加密组件内容不作还原承诺 |
| S06 | 键合线、焊球等跨层结构 | 三维结构及其网格可参与显示 | D [s03][s03]；当前板未验证完整外形和材料 |
| S07 | 刚柔结合板弯折查看 | 平板/弯折切换、简化弯折图形、刷新和弯折动画 | D [s16][s16]；当前板 Rigid-Flex 入口灰显，未实测 |

层叠编辑器右侧 Stackup Viewer 是**二维层叠示意图**，有归一化、缩放及层信息提示等辅助能力，不能把它当成主视口的三维爆炸视图。[The Edit Layers Window][s04]

### 3.5 选择、检查与测量（I01—I08）

| 编号 | 能力 | 行为 | 证据 |
| --- | --- | --- | --- |
| I01 | 对象/边/控制点选择 | General、Edge、Handle 三种模式 | D [s14][s14]；控制点编辑不在本次移植范围 |
| I02 | 重叠对象循环选择 | B 在光标下候选对象中切换 | D [s01][s01]；三维版需正确处理高度与裁剪 |
| I03 | 悬停预选 | 光标经过对象时预览选择颜色 | U10、D [s09][s09] |
| I04 | 选择颜色区分 | 首次与追加选择可用不同颜色 | U10、D [s09][s09] |
| I05 | 未选对象淡化 | 有选择时按指定透明度弱化其余对象 | U10、D [s09][s09] |
| I06 | 选中线框实体化 | 线框模式中实体显示当前选择 | U10、D [s09][s09] |
| I07 | 属性联动 | 选择、Properties、网络/元件定位联动 | U01、D [s06][s06]；可编辑属性不纳入只读查看器 |
| I08 | 测量覆盖层 | 位置、段长、多段长度、面积/周长等，单位与有效位数可调 | U01 有 Measure、U08 有显示选项；D [s15][s15]；不声称所有操作均是任意空间两点测量 |

### 3.6 内部结构查看（C01—C02）

| 编号 | 能力 | 行为 | 证据 |
| --- | --- | --- | --- |
| C01 | Z 向拉伸 | 用倍率放大层间结构，Dynamic 实时预览 | U04、D [s02][s02]；显示倍率不应写回物理厚度 |
| C02 | 单个或多个剖切平面 | 移动、旋转、数值设定位置/姿态，可裁切结构、网格和场图 | U02/U05 有入口；D [s07][s07]；本次未创建平面 |

本次没有证据确认 Layout 具有与 3D Modeler 完全相同的截面封口、法向翻转对话框或切面材质选项。也没有证据把独立分层爆炸、光线追踪、PBR、环境贴图、阴影、景深或正交/透视切换列为当前 Layout 已核实能力。后续若要加入，应标作 Pomelo 自身扩展或另行核查。

### 3.7 辅助显示、输出与结果可视化（A01—A13）

| 编号 | 能力 | 行为及边界 | 证据 |
| --- | --- | --- | --- |
| A01 | 网格与方向参考 | 主/次网格、开关、工作层高度与方向三轴 | U01/U09、D [s01][s01]、[s17][s17] |
| A02 | 背景颜色 | 文档显示背景可配置 | U09、D [s17][s17] |
| A03 | 文字显示 | Text 开关；源文字和选择顶点标签需区别 | U05/U08 |
| A04 | 连接点符号 | Draw connection points | U08、D [s08][s08] |
| A05 | 飞线显示 | rats 层可见时绘制连接提示 | U08；不作为实体铜连接 |
| A06 | 仿真辅助几何显示 | HFSS Extents/Air Box、Ports/Sources 开关 | U05/U13；需要已有范围/端口数据，本次未创建 |
| A07 | 复制当前图像 | Copy Image | U05；确认入口，未覆盖用户剪贴板 |
| A08 | 设计预览图导出 | 官方 API 提供 JPG preview 输出 | D [s21][s21]；预览图不保证等同当前视口截图 |
| A09 | 三维模型导出 | ECAD 到三维文件的输出 | D [s22][s22]；格式随版本变化，与截图导出及求解模型导出不同 |
| A10 | 网格显示 | 预处理网格/求解网格及相应显示过滤 | D [s03][s03]、[s23][s23]；需要对应网格数据，不是三角化渲染网格的同义词 |
| A11 | 场结果叠加 | 对网络、图层、剖切面或三维几何显示场量 | D [s24][s24]；需要已保存场的仿真结果 |
| A12 | 动画及输出 | 按参数/场量等生成帧，可输出 GIF/AVI/WebM | U02/U05 有 Animate；D [s25][s25]；不是已验证的镜头自动环绕 |
| A13 | 工作区面板布局 | 面板显隐、停靠、保存布局 | U01/U02/U05、D [s02][s02]；属于三维查看的配套工作区能力 |

## 4. 对显示规则的实现解释

以下是从功能行为提出的 **P 级工程分解**，不描述 Ansys 的内部源码或 GPU 实现：

1. 先解析物理层叠、几何和对象身份，得到世界空间结构。
2. 根据层、区域、网络、类别和对象状态求可见集合。
3. 由 Default/Sketch/Solid 与逐层覆盖决定面、边、颜色、透明度。
4. 根据选择状态增加高亮、淡化和选中实体化；不改变源材料。
5. 使用相机、Z 显示变换和剖切平面得到最终视图。
6. 网格、场、标记和测量通过带数据来源的独立覆盖层加入。
7. 输出时固定相同显示快照；保留“屏幕展示”和“物理数据”的区别。

## 5. 核查限制与后续补证

| 问题 | 当前结论 | 进一步验证方法 |
| --- | --- | --- |
| 半透明交叠是否完全排序正确 | ? 只能确认支持透明设置 | 用公开合成层叠在多个角度验证；不推断其采用 OIT |
| 剖切是否自动生成封口 | ? Layout 专用资料未确认 | 用可丢弃模型创建平面，观察截面和拾取 |
| 命名视图是否保存相机 | D 仅确认层/网络可见性 | 改变角度后保存/恢复对比 |
| Package 是否包含当前元件实体 | 只见入口，当前未启用 | 使用有合法包络与高度数据的样本 |
| 3D 组件和层级显示细节 | D 确认能力，未完成本地验收 | 使用未加密公开组件与多级布局样本 |
| 刚柔结合、网格、场和动画 | D 确认，当前板未执行 | 分别准备含弯折、网格、已保存场的公开样本 |
| 任意空间测距 | ? Layout 的测量文档不足以证明全部三维情况 | 跨层、斜面、孔壁两点测量，与物理坐标对照 |
| 图像与 CAD 输出的精度和格式 | D API 有能力，当前未执行 | 在测试目录导出合成模型，与源单位/实体核对 |

## 6. 官方资料索引

访问日期均为 2026-10-18。除特别注明外，使用 v252 帮助；PyAEDT stable 页面是补充资料，不作为本机每个 API 均已测试的证明。

- [s01 — Layout Editor Display][s01]
- [s02 — Layout View Menu][s02]
- [s03 — Working with the Layers Window][s03]
- [s04 — The Edit Layers Window][s04]
- [s05 — Working with the Nets Window][s05]
- [s06 — Working with the Components Window][s06]
- [s07 — Clip Planes（Layout 专用）][s07]
- [s08 — Layout Editor Options: Object Panel][s08]
- [s09 — Layout Editor Options: Selection Panel][s09]
- [s10 — Layout Editor Options: Advanced Panel][s10]
- [s11 — Setting a Layer Color][s11]
- [s12 — Setting Layer Visibility][s12]
- [s13 — Package Definitions for Components][s13]
- [s14 — Selection Modes in the Layout Editor][s14]
- [s15 — Working with the Measure Window][s15]（共享帮助页位于 Circuit，内容明确针对 HFSS 3D Layout）
- [s16 — Analyzing a Design With Rigid-Flex Bends][s16]
- [s17 — Layout Editor Options: Display Panel][s17]
- [s18 — 官方 Viewing and Visibility 课程][s18]（2023 R1 课件）
- [s19 — 官方 Hierarchy & 3D Components 课件][s19]（2023 R1）
- [s20 — HFSS 3D Layout Help][s20]（Layout Shortcuts 与导航选项）
- [s21 — PyAEDT export_design_preview_to_jpg][s21]
- [s22 — PyAEDT export_3d_model][s22]
- [s23 — Microstrip Filter 入门：Plotting the HFSS Mesh][s23]
- [s24 — Plotting Field Overlays in HFSS 3D Layout][s24]
- [s25 — Creating Animations][s25]

[s01]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/Subsystems/HFSS3DLayout/Content/3DLayout/LayoutEditorDisplay.htm
[s02]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/Subsystems/HFSS3DLayout/Content/3DLayout/LayoutViewMenu.htm
[s03]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/Subsystems/HFSS3DLayout/Content/GettingStarted/WorkingWithTheLayersWindow.htm
[s04]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/Subsystems/HFSS3DLayout/Content/3DLayout/TheEditLayersWindow.htm
[s05]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/Subsystems/HFSS3DLayout/Content/GettingStarted/WorkingwiththeNetsWindow.htm
[s06]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/Subsystems/HFSS3DLayout/Content/GettingStarted/WorkingwiththeComponentsWindow.htm
[s07]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/Subsystems/HFSS3DLayout/Content/3DLayout/ClipPlanes.htm
[s08]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/Subsystems/HFSS3DLayout/Content/3DLayout/LayoutEditorOptionsObjectPanel.htm
[s09]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/Subsystems/HFSS3DLayout/Content/3DLayout/LayoutEditorOptionsSelectionPanel.htm
[s10]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/Subsystems/HFSS3DLayout/Content/3DLayout/LayoutEditorOptionsAdvancedPanel.htm
[s11]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/Subsystems/HFSS3DLayout/Content/3DLayout/SettingaLayerColor.htm
[s12]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/Subsystems/HFSS3DLayout/Content/3DLayout/SettingLayerVisibility.htm
[s13]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/Subsystems/HFSS3DLayout/Content/3DLayout/CreatingOrEditingPackageDefinitionsForComponents.htm
[s14]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/Subsystems/HFSS3DLayout/Content/3DLayout/SelectionModesintheLayoutEditor.htm
[s15]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/Subsystems/Circuit/Content/GettingStarted/WorkingwiththeMeasureWindow.htm
[s16]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/Subsystems/HFSS3DLayout/Content/3DLayout/AnalyzingaDesignWithBends.htm
[s17]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/Subsystems/HFSS3DLayout/Content/3DLayout/LayoutEditorOptionsDisplayPanel.htm
[s18]: https://innovationspace.ansys.com/courses/wp-content/uploads/sites/5/2021/07/HFSS_3DL_GS_2023R1_EN_LE02_Visibility.pdf
[s19]: https://innovationspace.ansys.com/courses/wp-content/uploads/sites/5/2024/11/HFSS_3DL_GS_2023R1_EN_WS7.4_FullworkflowPart4.pdf
[s20]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/PDFs/HFSS%203D%20Layout.pdf
[s21]: https://aedt.docs.pyansys.com/version/stable/API/_autosummary/ansys.aedt.core.hfss3dlayout.Hfss3dLayout.export_design_preview_to_jpg.html
[s22]: https://aedt.docs.pyansys.com/version/stable/API/_autosummary/ansys.aedt.core.hfss3dlayout.Hfss3dLayout.export_3d_model.html
[s23]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/PDFs/HFSS%203D%20Layout%20Microstrip%20Filter.pdf
[s24]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/Subsystems/HFSS3DLayout/Content/3DLayout/PlottingFieldOverlays.htm
[s25]: https://ansyshelp.ansys.com/public/Views/Secured/Electronics/v252/en/Subsystems/HFSS3DLayout/Content/ReportsandPostProc/CreatingAnimations.htm
