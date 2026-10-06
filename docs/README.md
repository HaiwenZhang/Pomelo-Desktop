# 文档导航

Pomelo Desktop 是基于 Rust、GPUI 和 GPUI Kit 的本地 PCB 查看器。本文档保留产品能力、工程设计、维护约定和可复用的验证方法，便于使用、贡献及后续开发。

## 功能与使用

- [功能与操作](viewer-features.md)：导入、查看、搜索、显示控制及功能边界。
- [开发进展与验证](development-progress.md)：已有能力、检查命令及验收范围。
- [后续开发计划](native-pcb-viewer-development-plan.md)：改进方向与完成标准。

## 工程设计

- [代码组织](code-organization.md)：crate 边界和应用模块职责。
- [构建依赖](gpui-dependencies.md)：依赖来源、锁文件和构建方式。
- [GPU 设计](cad-gpu-painter-design.md)与[原生后端](native-gpu-backends.md)：数据流、帧目标和资源生命周期。
- [渲染决策](adr/0001-native-gpu-rendering.md)：为何复用 GPUI 原生设备。
- [国际化](i18n.md)与[字体来源](pcb-text-font-provenance.md)：消息契约及资源维护。

## 验证与维护

- [验证方法](validation-guide.md)：CPU、GPU、界面与性能验证。
- [隔离配置](ui-validation-profiles.md)：独立验证实例的启动方法。
- [导航与选择](viewport-navigation-validation.md)：缩放、拾取、搜索定位。
- [显示控制](display-controls-validation.md)：图层、着色、填充和标签。
- [查看状态](view-state-validation.md)：保存、恢复、退出与失败处理。
- [设置](settings-validation.md)：语言、主题、快捷键和焦点。
- [诊断界面](diagnostics-ui-validation.md)：错误、分页、取消及上下文。

公开文档不收录实际项目名称、设计截图、本地案例路径、原始运行日志或专有格式解析细节。通用几何算法、架构与测试方法继续保留。第三方资源的原始许可证、署名和来源记录维护在对应资源目录。

待开发能力见[审阅能力规划](viewer-review-capabilities-plan.md)，实施边界见[审阅开发设计](viewer-review-development-design.md)。
