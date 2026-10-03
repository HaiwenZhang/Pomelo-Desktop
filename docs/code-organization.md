# 代码组织执行约定

更新：2026-10-04。依据研发计划第 4、6.3 节及用户确认的目录结构。当前只开发 Windows。

## 已落实的目录

```text
crates/pomelo-core/src/
├── model/mod.rs
├── geometry/                 # 路径、圆弧、焊盘、铜皮三角化、单位
├── interaction/              # 相机、拾取、选择和空间索引
├── search/mod.rs
└── i18n.rs                   # 共享五语消息契约

crates/pomelo-render/src/
├── scene/                    # 走线、铜皮、焊盘、钻孔批次及覆盖诊断
├── backend/
│   ├── d3d11.rs              # Windows 后端公共入口
│   └── d3d11/                # 各业务管线、GPU 缓存、合成器和硬件诊断
├── text/                     # MSDF 资源、板文字/标签布局、字形实例；旧笔画兼容验证
└── shaders/                  # pcb/trace/copper/pad/text/label.hlsl

crates/pomelo/src/
├── main.rs                   # 最小可执行入口
├── app.rs                    # 应用初始化和事件循环
├── actions.rs                # 应用命令
├── main_window.rs            # 主窗口配置
├── workbench/                # 文档切换、任务及全局协调
├── welcome/                  # 欢迎页组合；正式 Action 与工作台文件回调
├── settings/                 # 原生语言与外观设置弹层
├── document/                 # 文档会话
├── viewport/                 # GPUI 画布及渲染适配
├── panels/                   # 检查器、图层行、显示、搜索结果、拾取过滤
├── services/                 # 启动参数/原生错误反馈、偏好、最近文件、源几何缩略图
└── theme.rs / i18n.rs / assets.rs # 语义配色、五语命令、组件与产品图标
```

`backend/d3d11.rs` 是入口，不把所有管线塞进一个文件；其子目录容纳 Windows 的实际实现。所有业务 HLSL 集中在 `shaders/`，不再与后端 Rust 混放。后续平台启动前不创建 `metal.rs`、`wgpu.rs`、`.metal` 或业务 `.wgsl` 占位文件。独立 wgpu 三角形实验源码和直接依赖已按用户要求移除；历史报告保留。Windows 后端（含验证）不引入 wgpu。

核心及渲染 crate 保留已有公开模块的重新导出，供现有调用者使用；实际源代码归属上述目录，新模块按职责放置，不能继续往根目录平铺。共享字体仍位于根 `assets/fonts/`，其许可和资源清单不属于应用私有资源。

## 尚需继续拆分的职责

搜索 runtime anchor 的核心查询归 `interaction/picking_index/navigation.rs`，与 `SelectionTarget` 的全组身份及 `locate.rs` 的全部几何范围分离。实际 canvas hit 归 `picking_index/visible.rs`，应用捕获 display 快照并保留实际 hit，`panels/inspector.rs` 组合实际源字段与全组统计，`ViewState` 保存可选锚点。验证范围见[锚点应用记录](search-anchor-app-validation.md)。

左右面板布局归窗口级 `workbench/panel_layout.rs`，共享 Kit 尺寸状态；`panels/sidebar.rs` 仅组合标准按钮、标题及窄栏，`viewport` 适配布局与搜索焦点，`services/prefs.rs::PanelStore` 管理 `panels.json`。布局保存与文档查看状态共用已有后台串行写任务，不能在每个文档复制一套窗口级布局，见[面板验证](panel-collapse-validation.md)。

索引定位边界归 `pomelo-core/interaction/picking_index/locate.rs`，复用核心几何及 renderer 提供的实际 MSDF quad；应用 `viewport` 仅在后台调用，源几何 bounds API 保留独立契约。绘制层序为从底到顶，左侧列表使用 `BoardDisplay::layers_front_to_back` 展示从顶到底，置顶/置底与持久化不变。UI 字体策略保持原状，见[定位](selection-navigation-bounds-validation.md)及[TOP 顺序验证](top-layer-order-validation.md)。

元件的交互分组归 `pomelo-core/interaction/component_reference.rs`，使用带类型 pin/finger 源锚点，按精确 reference 聚合；源模型的独立 placement 保留原身份。`search` 和 `interaction/selection` 共享分组语义，应用 `viewport` 负责后台准备与 GPUI 适配，`panels` 展示成员，`pomelo-render/backend/d3d11/board.rs` 消费完整高亮集合。组成员和相机数学不依赖 GPUI/GPU。UI 字体策略保持原状，验证边界见[元件分组记录](component-reference-validation.md)。

欢迎页主体和最近文件列表已提取为 `welcome/`：打开文件、设置和查看全部派发正式 Action，最近文件的打开/重定位/移除由工作台提供回调；历史状态与持久化仍由 services/recent 管理。图层行展示位于 `panels/layers.rs`，显示控制位于 `panels/display.rs`，网络/元件搜索结果位于 `panels/search.rs`，拾取过滤位于 `panels/picking.rs`；视口保留虚拟列表、输入/异步任务、可见性、选择、顺序和滚动状态以及事件绑定。语言与外观设置已落入 `settings/mod.rs`，实际实现与验证范围见 [设置验证](settings-validation.md)。侧栏组合、任务反馈与选中对象成员区域尚未全部提取，应用资产服务已在 `assets.rs` 组合 GPUI Kit 默认资源与实际使用的 Lucide 图标，品牌 SVG 位于 `crates/pomelo/assets/pomelo.svg`；`services/preview.rs` 在后台根据真实源几何生成最近文件缩略图。后续功能开发持续按用户给出的目录落入真实实现，不以空目录代表已完成；全部 UI 模块拆分仍待继续。

## 启动错误反馈

`services/startup_error.rs` 复用偏好模块的配置根目录，在 GPUI 窗口尚未建立时提供原生错误对话框、有界日志和打开对应日志目录的系统入口；失败时保留原日志。五语语义键位于核心消息契约，`app.rs` 负责构造边界、窗口错误接线和退出码。Windows 原生对话框依赖只归应用 crate，验证范围见 [启动错误验证](startup-error-validation.md)。

## 三角剖分

workspace 使用 `earcut = "=0.4.11"`，`pomelo-core` 通过 `earcut.workspace = true` 引入，`Cargo.lock` 锁定版本 0.4.11。等号将用户要求的版本精确固定。实际调用位于 `pomelo-core/src/geometry/copper.rs`，CPU 三角剖分不依赖 GPUI 或 GPU；洞轮廓和三角覆盖正确性由铜皮回归验证。

2026-10-02 复核运行 `python -X utf8 scripts/cargo.py +stable test -p pomelo-core --test copper_mesh --locked --offline`，10 项通过、0 失败，覆盖凹轮廓、反向绕序、共线与重复点、重叠孔洞并集、资源限制和取消。研发计划第 4.1、6.3 节已同步目录与依赖约定。

## UI 设计落地

欢迎页的 `ScrollState` 类型位于 `welcome/mod.rs`，生命周期归 Workbench，包含主区和三个历史区域各自的 ScrollHandle。展示模块通过公开 Kit 滚动条绑定既有 handle；打开和查看全部派发正式 Action。`welcome/recent.rs` 复用通用 FocusScroll 显示行内聚焦命令，继续查看行同时接入主区和自身列表，不新增 Tab 停靠点、不持有历史保存或 GPU 状态。

`welcome/mod.rs` 读取窗口逻辑宽度与 rem 计算功能介绍区横向/纵向布局；只组合展示，不持有渲染或导入状态。侧栏设置入口派发已有 `OpenSettings` Action，由 Workbench 处理，不再额外传递同一设置回调。共享文案继续使用核心五语入口。

独立验证配置根目录归 `services/prefs.rs`，四个 store 复用 `POMELO_CONFIG_DIR` 规则；启动编排位于 `scripts/launch-ui-validation.ps1`，不把测试配置目录逻辑放进欢迎页。未设置覆盖变量时日常偏好路径保持原样，无效显式覆盖不回退写入日常目录，见 [UI 验证配置目录](ui-validation-profiles.md)。

`welcome/recent.rs` 使用 `Presentation::{Sidebar, Continue, All}` 表达侧栏、开始页三项预览和完整历史，WorkBench 选择展示方式并传入既有文件命令。三个 Kit Scrollable wrapper 分别拥有稳定 ID；展示模块不改历史上限、编码、预览缓存或磁盘写入。完整列表的可用高度由 `welcome/mod.rs` 主区组合约束，实际范围见 [欢迎页布局验证](welcome-layout-validation.md)。

`workbench/document_tabs.rs` 组合公开 Base `Tab` / `Tabs`、Kit 格式标签、关闭按钮及文档菜单，拥有设计稿对应的标签外观。文档生命周期、全局切换/关闭和横向滚动状态仍由 Workbench 管理；完整名称提示复用元素所属生命周期，菜单入口使用共享五语消息。实际范围见 [工作区标签记录](workbench-tabs-validation.md)。

应用焦点滚动组件 `panels/focus_scroll.rs` 默认显示纵向检查器控件，文档标签显式使用横向模式。每个范围不增加 Tab 停靠点，按所属滚动区和当前焦点显示内容；不持有文档业务状态，不改变 Kit 按钮及菜单的键盘行为。

`panels/tabs.rs` 组合原生标签栏及独立可聚焦的溢出菜单，按实际文字排版宽度测量；选中面板与尺寸仍由各文档视口持有。`panels/layers.rs::settings` 组合底部图层设置、Slider 和百分比，SliderState 与正式显示命令仍归视口。工作台处理控件被移除后的焦点恢复，Windows 字体策略归 `theme.rs` 并跟随界面语言更新。最新样式、源码检查及实窗待验范围见 [侧栏对齐记录](sidebar-style-validation.md)。

`viewport/toolbar.rs` 负责文档命令栏展示和 Kit 控件分组；输入实体、工具、相机和着色状态由视口持有，以回调接入正式命令，不把渲染资源或领域几何搬进界面模块。

2026-10-03 按 `docs/ui-design` 重建欢迎页及工作区：标准标题栏、文档标签、横向命令栏、左右可调侧栏、固定深色画布和状态栏。侧栏使用 GPUI Kit `h_resizable` / `resizable_panel`，每文档持有尺寸状态；GPU 注册及业务 Shader 边界保持不变。预览不是 GPU 视口回读，不替代正式 PCB 渲染。实际窗口范围与未完成项见 [UI 验证记录](ui-design-validation.md)。

`services/preview.rs` 负责有界 PNG 编码/后台解码和 GPUI BGRA 转换；最近文件元数据归 `services/prefs.rs`，解码图片缓存、历史维护与异步保存归 `services/recent.rs`。`welcome/recent.rs` 仅按当前语言显示日期与源层数，工作台回调只捕获路径和编码，避免每次渲染复制 PNG。`panels/inspector.rs` 提供属性行展示及类型化源字段适配；分页与定位状态继续由视口负责。新增 `base64 0.22.1` 与 `chrono 0.4.45` 仅是应用直接依赖，沿用既有锁定版本，核心层依赖边界保持不变。

`viewport/presentation.rs` 负责按显示单位生成比例尺消息并保留小数刻度；核心相机仍负责刻度和逻辑像素长度，不依赖 UI。回归核对毫米/mil 标签的数值与核心投影一致，并检查五语消息可格式化。

单图层类别状态及序列化归 `pomelo-core/src/display.rs`；参考/索引拾取消费相同显示规则。`panels/layers.rs` 负责展开行及三个标准 Checkbox，视口管理展开对象、可变高度 `ListState`、过滤事件及显示快照。D3D11 合成器使用已有走线作用域、焊盘范围过滤和自定义铜皮批次过滤，不改变 Shader ABI 或 GPU 注册入口。详见 [单图层过滤验证](layer-primitives-validation.md)。

## 本次验证

`pomelo-render/src/scene/thumbnail.rs` 从正式不可变图元批次生成固定大小的 CPU 概览，复用核心解析焊盘距离、走线/圆弧查询和铜皮网格。应用主题模块传入材料颜色，返回普通 RGBA 与 LOD 摘要，不依赖 GPUI、PNG 或平台窗口。`services/preview.rs` 做应用适配，后台导入传入取消令牌并在发布前再检查；不改变 D3D11 视口或 GPUI 通用入口，见 [缩略图记录](recent-thumbnail-validation.md)。

`panels/diagnostics.rs` 共用页行与可持有焦点的分页按钮；视口和工作台保留页码、展开、请求身份校验与焦点状态。固定高度仅用于滚动窗口，页身份属于 Scrollable wrapper，内部滚轮不再同时移动上级面板。合成案例生成器、fixture 和正式 importer 回归分别位于 `scripts/`、`tests/fixtures/`、`pomelo-import/tests/annotations.rs`，见 [诊断 UI 验证](diagnostics-ui-validation.md)。

`panels/file_info.rs` 接收当前语言、展开状态、共享场景引用、诊断展示及切换回调，组合 Kit `Collapsible` 和公开 Base Button；视口保留展开状态与诊断分页。成员列表的固定窗口、分页 revision 和滚轮边界仍在视口协调，不把导入或 GPU 状态迁入展示模块。详见 [检查器布局验证](inspector-layout-validation.md)。

`panels/focus_scroll.rs` 提供不增加 Tab 停靠点的焦点作用域与离屏命令显示；每个视口持有外层 `ScrollHandle`。`FocusScroll` 使用公开 keyed state 接入动态选择命令、过滤及显示控件，窗口尺寸改变时重新检查聚焦命令的边界。文件信息与诊断标题、分页命令继续保留稳定作用域，内容保持自然高度，Kit 滚动条和内层列表独立工作。展示模块读取真实焦点显示边框，见 [检查器焦点验证](inspector-focus-validation.md)。

成员卡片复用 `FocusScroll` 的嵌套作用域，视口拥有独立内层滚动与分页 `NavigationFocus`；成功结果发布时复位列表，并仅为仍聚焦的分页命令转移页边界焦点。卡片使用已有五语消息显示类型、ID 和完整命令提示。当前实现及实窗范围见同一焦点验证记录。

工作区负责从视口捕获查看快照，直接排入后台串行合并；`services/prefs.rs` 负责身份匹配、状态校验及原子落盘。后台任务共享结果，供运行时错误观察和退出等待使用，写入不依赖 GPUI 主线程在退出期间继续分发。实窗及超时边界见 [退出恢复验证](view-exit-validation.md)。

网络显示模式仅作为领域枚举及序列化状态位于 `pomelo-core::display`；共享调色板与 CPU 材料/HLSL 生成位于 `pomelo-render/src/scene/colors.rs`，Windows 后端和业务 Shader 消费原整数网络 ID。应用的网络搜索行和检查器使用同一色标，视口只分发命令和发布帧快照。没有把 PCB 取色实现移入 GPUI 补丁或重新上传每帧几何；当前平台仍仅 Windows，其他后端以后复用相同领域模式和颜色表。详见 [网络着色验证](network-colors-validation.md)。

工作区 364 项测试通过、0 失败、7 项按原条件忽略；工作区全部目标/功能 Clippy 通过。Windows 硬件显式运行的走线/紧凑文字与带孔铜皮合成测试 2 项通过。

原始证据：[工作区测试](gpu-validation/layout-reorganization-workspace-tests.log)、[硬件测试](gpu-validation/layout-reorganization-hardware-tests.log)。迁移不代表全部真实窗口或 156 案例整板验收完成；15061 的全量紧凑文字已另行通过 [D3D11 硬件验证](text-compact-gpu-validation.md)，整板合成、实窗与性能门槛仍待验收。
