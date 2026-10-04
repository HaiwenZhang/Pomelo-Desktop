# 自定义焊盘轮廓验证

> 本文保留历史验证记录，其中引用的辅助脚本及 pomelo-core/import/render 的 example 探针已移除；旧探针命令不再可执行。当前验证使用各 crate 的测试代码。

日期：2026-10-03。参考最新 Web 项目 `C:/Users/Zen/Desktop/gitrepo/pomelo`，源码及 `E:/brd_cases` 均只读。UI 界面的字体保持现状；本次没有修改 UI 字体、主题或 Web 源码。

## 修正范围

自定义焊盘此前在非填充模式仍绘制填充几何。本次在 `pomelo-render/scene/pads/outlines.rs` 准备共享轮廓，Windows 使用 GPUI 同设备 D3D11 和已有 HLSL 走线管线编码绘制，不使用 wgpu。

轮廓优先保留源解析直线和圆弧，变换时处理所属焊盘的旋转、镜像与圆弧方向；没有解析路径时闭合所有轮廓，包括内部孔洞。与 Web `PadShape.iterateEdges` 一致，轮廓实例宽度为零，由 shader 提供最小 0.5 逻辑像素线宽。实例保留 pin/via 类型、源 ID、网络和图层，避免相同数字 ID 的 pin 与 via 相互高亮。

普通绘制在非填充模式显示轮廓；选择和悬停叠加层使用轮廓，不重新填满内部。轮廓上传接入已有逐帧预算、缓存和 reset 生命周期。切换填充状态或选择只更新显示状态，不重新构建源几何缓存。

显示面板新增标准 GPUI Kit Checkbox“填充焊盘”，使用现有字体、间距、焦点滚动与主题。消息 `view.fill_pads` 同步 en、zh-CN、zh-TW、ja、ko 五语；状态沿用文档视图的持久化机制。

## 验证结果

| 验证 | 范围 | 结果 |
| --- | --- | --- |
| 最新 Web 真实几何对照 | USBC_FPC，UTF-8，12 条边界 | 零差异 |
| 最新 Web 真实几何对照 | AGILEX_I_SERIES，Windows-1252，694 条边界，其中 189 条圆弧 | 零差异 |
| CPU 回归 | 内外环、所属对象、高位/残差坐标、解析圆弧旋转/镜像、预算与取消 | 3 项通过 |
| D3D11 硬件像素 | pin 外环/孔洞、via 圆弧、下方走线、相同 ID 的不同对象类型、选择/悬停、图层隐藏、缓存与 reset | 通过 |
| 原铜皮硬件回归 | 逐帧上传后钻孔合成及既有铜皮像素断言 | 通过 |
| 工作区检查 | 所有特性、锁文件、离线测试 | 396 通过，11 个硬件/外部案例测试默认忽略，零失败 |
| 静态检查及构建 | fmt、全目标全特性 Clippy `-D warnings`、Windows debug 应用 | 通过 |

几何 oracle 直接加载 Web `AllegroParser`、`SceneBuilder` 和 `PadShape.iterateEdges`，对比实际原生 GPU ABI，保留重复边界。坐标绝对容差为 `1e-7 mm`；圆弧标量使用 f32 相对容差，周期角容差为 `2e-6`。报告记录源文件和 Web 参考文件 SHA-256。AGILEX 需要显式 Windows-1252，未进行静默有损回退。LPDDR4 的自定义边界计数为零，不计作有效几何覆盖。

## 原生窗口与键盘

独立配置目录中打开 USBC_FPC，真实 GPU 为 NVIDIA GeForce RTX 5080，后端 `GPUI_D3D11`，ready 为 true，无 GPU 错误。12 个自定义边界实例上传 1,536 字节。

关闭“填充焊盘”后，在约 529% 缩放下观察到透明内部和可见边界。点击自定义焊盘边界拾取 Pin 15223，并按网络模式选择 VBUS_CONN（Net 641），轮廓正确高亮。Tab 可到达复选框并显示焦点框；连续 Space 分别切回填充和轮廓模式，轮廓缓存构建次数保持为 1。

正常 Alt+F4 退出后，用同一独立配置和同一可执行文件重开，`filled=false`、相机 `52.93772193332696` 逻辑像素/mm（约 529%）及 Net 641 选择均恢复。重新呈现时自定义填充 draw calls 为零、轮廓有实际 draw calls，GPU 无错误。预览正常关闭，未修改日常用户配置。

控件沿用既有显示面板布局，不新增弹层或额外滚动容器。本次完成真实深色窗口的鼠标、键盘与重开检查；不将其表述为五语、浅色、所有 DPI 的完整 UI 验收。

最终应用为 `target/debug/pomelo.exe`，验证副本 SHA-256 与其一致：

```text
f63e83b131b9f59bd237b19a3a3a6c68f555d5d574802923fb9d88ea951bf5d0
```

## 原始证据与复验

证据保存在工作区 `.cache/canvas-parity/`：

- `custom-outlines-usbc/report.json`、`custom-outlines-agilex/report.json`
- `custom-outline-gpu.log`、`custom-outline-copper-regression.log`
- `custom-outline-workspace-final.log`、`custom-outline-clippy-final.log`、`custom-outline-build.log`
- `custom-pad-filled-window.jpg`、`custom-pad-unfilled-window.jpg`（原始窗口截图）
- `custom-outline-window-evidence.json`（冻结的操作结果）、`custom-outline-window-restored.json`（重开后的遥测）

```powershell
cargo +stable build -p pomelo-render --example custom_pad_outline_probe --locked --offline
& C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/.bin/tsx.cmd scripts/check-custom-pad-outline-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo target/debug/examples/custom_pad_outline_probe.exe E:/brd_cases/USBC_FPC.brd .cache/canvas-parity/custom-outlines-usbc utf-8
& C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/.bin/tsx.cmd scripts/check-custom-pad-outline-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo target/debug/examples/custom_pad_outline_probe.exe E:/brd_cases/AGILEX_I_SERIES.brd .cache/canvas-parity/custom-outlines-agilex windows-1252
cargo +stable test -p pomelo-render --all-features --lib --locked --offline hardware_custom_pad_outlines_preserve_holes_owner_kind_and_cache -- --ignored
```

本记录补齐自定义焊盘的非填充边界，不代表整板与 Web 的全部视觉和交互已一致。特殊背钻、极深缩放长线/曲线铜皮、多 DPI、设备恢复及大板性能等门槛仍见 [画布总体对照记录](canvas-web-parity-validation.md)。当前只开发 Windows。
