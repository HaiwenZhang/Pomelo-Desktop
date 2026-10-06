# 代码组织

| 目录 | 职责 |
| --- | --- |
| `crates/pomelo-core` | 领域模型、几何、搜索、选择、显示状态与共享消息 |
| `crates/pomelo-import` | 文件导入、场景转换与结构化诊断 |
| `crates/pomelo-render` | 场景准备、文字布局、图形资源与平台渲染后端 |
| `crates/pomelo` | 桌面窗口、文档会话、视口、面板、设置与偏好保存 |
| `locales` | 五种语言的消息资源 |
| `assets/fonts` | 字体资源、来源记录与许可证 |
| `support/gpui-compat` | GPUI 包名适配 |
| `scripts` | 资源及打包工具 |

领域逻辑保持独立于界面和 GPU。文件读取、导入与场景准备在后台执行，界面模块负责展示和命令分发。平台后端消费共享场景，各自管理设备资源和着色器。

Allegro、Altium、ODB++、PADS、KiCad 与 HFSS 3D Layout 的原生 Rust 解析统一位于 `crates/pomelo-import/src/formats`，各格式为同级模块。格式分发、二进制/文本读取、定义缓存、网络归属和几何转换均属于该 crate，直接输出共享场景；格式职责和支持范围见[解析器说明](../crates/pomelo-import/src/formats/README.md)。共享场景初始化和边界更新放在 `formats/scene_builder.rs`，PADS 保存填充的几何构建放在 `formats/pads/saved_fill.rs`。

开发检查见[开发与验证](development-progress.md)，依赖配置见[构建依赖](gpui-dependencies.md)。

## 应用模块

| 模块 | 负责内容 |
| --- | --- |
| `workbench/` | 文档切换、全局命令、面板布局、导入任务与保存协调 |
| `document/` | 文档会话、生命周期与后台准备流水线 |
| `viewport/` | 相机输入、选择、显示状态与渲染适配 |
| `panels/` | 图层、搜索、检查器、显示控制及诊断展示 |
| `welcome/` | 欢迎页和最近文件展示 |
| `settings/` | 语言与外观设置 |
| `services/` | 启动、偏好、最近文件、预览及错误反馈 |

展示组件通过命令或回调接入既有状态所有者，避免自行管理导入任务和 GPU 生命周期。窗口级面板布局与各文档查看状态分别保存。

### 按修改目的找文件

以下路径相对于 `crates/pomelo/src`。

| 修改内容 | 入口 |
| --- | --- |
| 导入、渲染数据准备、索引构建与查看状态恢复 | `document/preparation.rs` |
| 文档队列调度和准备结果发布 | `workbench/mod.rs` 的 `start_next_import` |
| 视口状态、初始化和生命周期 | `viewport/mod.rs` |
| 工具栏、面板、画布和状态栏的组合 | `viewport/layout.rs` |
| GPU 帧组装、上传状态和画布绘制 | `viewport/frame.rs` |
| 搜索、选择定位和成员分页任务 | `viewport/search.rs` |
| 点击拾取、重叠候选切换 | `viewport/picking.rs` |
| 悬停拾取和提示延迟 | `viewport/hover.rs` |
| 相机命令和拖动手势 | `viewport/navigation.rs`、`viewport/gesture.rs` |
| 检查器选中对象详情及成员列表展示 | `panels/selection_details.rs` |
| 面板展示接入视口状态和回调 | `viewport/layer_panel.rs`、`viewport/inspector_panel.rs`、`viewport/diagnostics_panel.rs` |
| 比例尺文案 | `viewport/scale_label.rs` |
| 配置文件的限量读取、原子保存和损坏备份 | `services/preferences/json_file.rs` |
| 语言、主题、面板、最近文件和查看状态的持久化规则 | `services/preferences/` 下的对应业务模块 |

`BoardViewport` 保持交互状态的唯一所有者；面板接收只读展示数据和命令回调。`DocumentPreparation` 是后台任务输入快照，`prepare_document` 不访问活动窗口；工作区在发布结果前继续核对请求身份和取消状态。配置业务存储组合 `JsonFileStore`，不复用另一项业务存储作为底层实现。

## 数据与任务边界

导入产生共享场景；搜索、选择和渲染准备消费领域数据，界面读取准备后的结果。后台结果发布时检查取消及请求身份，防止已关闭文档或过期请求覆盖当前视图。

文件及偏好读写不放入界面绘制路径。长集合使用分页或虚拟化，场景准备和 GPU 上传按资源版本失效，不在每帧重新解析文件或复制整板几何。

## 修改约定

几何与交互修改优先进入核心层；平台绘制进入渲染层；界面布局和系统对话框进入应用层。所有用户可见消息使用共享国际化契约。新增资源同时维护来源和许可材料。

路径说明归属，类型说明角色，函数说明动作，字段说明对象及必要单位。跨线段、铜皮、焊盘、绘图和文字的拾取索引命名为 `BoardPickingIndex`；解析器的中间结果命名为 `ParsedBoard`。有副作用的函数使用 `update_scene_bounds` 这样的动词名称，不使用容易被误认为查询的 `bounds`。

应用内部使用实际模块路径，例如 `crate::services::preferences`、`crate::panels::inspector`，不在可执行入口添加隐藏目录层次的别名。拆分模块应围绕独立职责和清晰输入输出，避免仅为减少行数建立泛称 `utils`、`helpers` 或 `common` 的收纳文件。
