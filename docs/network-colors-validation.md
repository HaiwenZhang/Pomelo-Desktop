# 网络着色实现与验证

日期：2026-10-03。范围：Windows 原生工作区、正式 D3D11/HLSL 合成绘制与查看状态。完整 UI、BRD MVP、真实大板性能仍未完成。

## 行为和架构

命令栏“图层着色 / 网络着色”现为可用的互斥按钮，当前选择可见。着色模式归 `pomelo-core::display::ColorMode`，保存于各文档 `BoardDisplay`，由原视图快照与原子保存流程持久化。旧版本缺失字段默认图层着色，未知枚举值拒绝加载并走已有五语配置诊断；没有新增配置文件或修改源 BRD。

沿用本地 Web 项目 `src/lib/render/pcb-net-colors.ts` 的 203 项 ARGB 调色板和 `color-mode.ts` 规则：非零 `NetId` 取 `id % 203`，零网络保持源图层色。相同源网络跨图层及图元使用同一基础 RGB；调色板循环且含重复色，不保证所有网络颜色唯一。名称、ID、源统计仍用于辨别对象。

调色板的单份实现位于 `pomelo-render/src/scene/colors.rs`。CPU 铜皮/自定义焊盘批次、网络列表及网络检查器标记调用同一取色函数；Shader 编译时由该表生成 HLSL 公共函数，避免维护两份颜色表。走线/圆弧和解析焊盘/过孔铜环在 vertex shader 中读取原有整数网络 ID，按模式选材质；铜皮及自定义轮廓使用逐批次常量颜色。原始 alpha 和铜皮不透明度保留。

模式切换只改变帧常量，不重解析、不重建或上传静态几何，不为每个网络拆分走线绘制批次。选择/关联/悬停仍遵循原高亮优先级，保持当前选择与相机；悬停查询按原显示快照失效。物理钻孔、板框、绘图和板文字保留原材料，图层显隐和类别过滤规则不变。

全部 PCB 业务仍在 `pomelo-render`，GPUI 补丁仅为通用入口；未改动 vendor、生成缓存、实例 ABI 或 GPUI 的设备/呈现链路。两种着色文案复用既有五语 `MessageKey`，没有新增硬编码界面文字。

## 自动检查

- 核心 85 项通过，含五语资源/源码 9 项；扩展旧配置默认、Net 模式序列化、未知值拒绝及完整 ViewState 往返。
- 应用 51 项通过；真实临时配置文件回归验证 Net 模式保存，以及另一文档仍保持 Layer 模式。
- 渲染库全功能普通测试 66 passed、4 ignored；新增 CPU 取色契约测试，核对循环、未分配网络和原 alpha。
- Windows debug 构建、工作区全部目标/功能 Clippy `-D warnings` 通过。

## 硬件证据

正式 `BoardRenderer` 离屏诊断显式运行 1 项通过：同网络走线、铜皮、解析焊盘/过孔、自定义轮廓 RGB 一致；铜皮 50% alpha 保留，钻孔色不变。网络、单对象、走线及悬停高亮仍优先；切回 Layer 模式恢复整张像素数组。绘图和板文字在 Net 模式中保留原像素。期间走线、铜皮、解析/自定义焊盘及钻孔的几何上传字节均未增加。

另在同次硬件测试用 205 个格子覆盖全部 203 项调色板、零网络及 `u32::MAX`，分别核对走线和解析焊盘 Shader 实际像素与 CPU 材料色一致；切回 Layer 后全部恢复源红色，上传不增加。

```powershell
python -X utf8 scripts/cargo.py +stable test -p pomelo-render --features native-gpu --lib hardware_copper_overlapping_holes_preserve_underlying_trace --locked --offline -- --ignored --nocapture
```

原始输出：[network-colors-hardware-tests.log](gpu-validation/network-colors-hardware-tests.log)。这是 128×128 硬件诊断，不是正式产品回读路径，不代表大板帧率、显存释放或完整实窗验收。

原始日志位于仓库已有的忽略目录；下面保留本次成功输出的关键摘录，随本记录保存：

```text
LAYER_PRIMITIVE_FILTERS_PIXELS_VERIFIED uploads_unchanged=true
NETWORK_COMPOSITE_PIXELS_VERIFIED highlights_preserved=true uploads_unchanged=true
NETWORK_PALETTE_PIXELS_VERIFIED samples=205 trace_and_pad=true net_zero_fallback=true
test backend::d3d11::trace_d3d11::pixel_tests::hardware_copper_overlapping_holes_preserve_underlying_trace ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 69 filtered out; finished in 0.95s
```

## Windows 实窗证据

最新 debug 程序打开 `E:\brd_cases\LPDDR4_PGB.brd`，点击网络着色后，画布从源图层色改为按真实网络分色，按钮选中态互斥。`Ctrl+K` 搜索 GND 并 Enter 后定位源网络 2035，检查器显示真实 825 走线、234 引脚、319 过孔、4 铜皮、293.831411 mm。搜索行及检查器标记使用该网络基础色，画布选择高亮覆盖它。

保留 GND 选择时切换两种着色模式，选择、统计与相机保持，其他网络基础颜色变化；在设置中切换深/浅主题，GPU 画布继续显示，选择色跟随主题。最后恢复浅色和跟随系统语言。

以 Net 模式关闭文档、正常退出，再启动并重新打开 LPDDR4：网络着色按钮保持选中，网络分色画布及 GND 源选择 2035、相机和检查器统计恢复。该证据来自实际重启后的窗口，不以序列化单元测试代替它。

随后打开 `USBC_FPC.brd`，该文件保持原先 Layer 模式，未继承 LPDDR4 的 Net 模式；实际切换 Net 后按真实网络分色，再恢复该文件的 Layer 模式。关闭 USBC、再次打开 LPDDR4，后者仍恢复 Net 模式及 GND 选择，没有被另一文件覆盖。

## 后续验收

同窗多文档进一步实窗复验：USBC / LPDDR4 同时打开，Ctrl+Shift+Tab / Ctrl+Tab 切换后分别保持 Layer / Net；USBC 无选择、86% 相机，LPDDR4 保留 GND 源网络 2035、177% 相机及检查器统计。详见 [工作区标签验证](workbench-tabs-validation.md)。

五语工作区全流程、DPI、多显示器、色觉可辨性与大板耗时仍待专项验收。当前共享 Web 调色板的部分暗色在深色画布上对比度较低；不能将同源取色和像素通过扩大为可访问性或整板性能通过。
