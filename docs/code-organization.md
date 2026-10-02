# 代码组织执行约定

更新：2026-10-02。依据研发计划第 4、6.3 节及用户确认的目录结构。当前只开发 Windows。

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
├── text/                     # 字体加载、布局和紧凑笔画实例
├── shaders/                  # pcb/trace/copper/pad/text.hlsl
└── triangle.rs               # 已有独立 wgpu 实验；非产品呈现路线

crates/pomelo/src/
├── main.rs                   # 最小可执行入口
├── app.rs                    # 应用初始化和事件循环
├── actions.rs                # 应用命令
├── main_window.rs            # 主窗口配置
├── workbench/                # 文档切换、任务及全局协调
├── welcome/                  # 欢迎页组合；操作回调由工作台提供
├── settings/                 # 原生语言与外观设置弹层
├── document/                 # 文档会话
├── viewport/                 # GPUI 画布及渲染适配
├── panels/                   # 检查器、图层行、显示、搜索结果、拾取过滤
├── services/                 # 启动参数、偏好、最近文件
└── theme.rs / i18n.rs
```

`backend/d3d11.rs` 是入口，不把所有管线塞进一个文件；其子目录容纳 Windows 的实际实现。所有业务 HLSL 集中在 `shaders/`，不再与后端 Rust 混放。后续平台启动前不创建 `metal.rs`、`wgpu.rs`、`.metal` 或业务 `.wgsl` 占位文件。已有三角形 WGSL 保留为独立实验。

核心及渲染 crate 保留已有公开模块的重新导出，供现有调用者使用；实际源代码归属上述目录，新模块按职责放置，不能继续往根目录平铺。共享字体仍位于根 `assets/fonts/`，其许可和资源清单不属于应用私有资源。

## 尚需继续拆分的职责

欢迎页主体和最近文件列表已提取为 `welcome/`，文件操作由工作台传入回调；最近文件的状态与持久化仍由 services/recent 管理。图层行展示位于 `panels/layers.rs`，显示控制位于 `panels/display.rs`，网络/元件搜索结果位于 `panels/search.rs`，拾取过滤位于 `panels/picking.rs`；视口保留虚拟列表、输入/异步任务、可见性、选择、顺序和滚动状态以及事件绑定。语言与外观设置已落入 `settings/mod.rs`，实际实现与验证范围见 [设置验证](settings-validation.md)。侧栏组合、任务反馈与选中对象成员区域尚未全部提取，应用资产服务也尚未按最终结构交付。后续功能开发持续按用户给出的目录落入真实实现，不以空目录代表已完成；全部 UI 模块拆分仍待继续。

## 三角剖分

workspace 使用 `earcut = "=0.4.11"`，`pomelo-core` 通过 `earcut.workspace = true` 引入，`Cargo.lock` 锁定版本 0.4.11。等号将用户要求的版本精确固定。实际调用位于 `pomelo-core/src/geometry/copper.rs`，CPU 三角剖分不依赖 GPUI 或 GPU；洞轮廓和三角覆盖正确性由铜皮回归验证。

2026-10-02 复核运行 `python -X utf8 scripts/cargo.py +stable test -p pomelo-core --test copper_mesh --locked --offline`，10 项通过、0 失败，覆盖凹轮廓、反向绕序、共线与重复点、重叠孔洞并集、资源限制和取消。研发计划第 4.1、6.3 节已同步目录与依赖约定。

## 本次验证

工作区 364 项测试通过、0 失败、7 项按原条件忽略；工作区全部目标/功能 Clippy 通过。Windows 硬件显式运行的走线/紧凑文字与带孔铜皮合成测试 2 项通过。

原始证据：[工作区测试](gpu-validation/layout-reorganization-workspace-tests.log)、[硬件测试](gpu-validation/layout-reorganization-hardware-tests.log)。迁移不代表全部真实窗口或 156 案例整板验收完成；15061 的全量紧凑文字已另行通过 [D3D11 硬件验证](text-compact-gpu-validation.md)，整板合成、实窗与性能门槛仍待验收。
