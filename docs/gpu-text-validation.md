# Windows PCB 文字 GPU 验证

记录日期：2026-10-02。当前仅验证 Windows D3D11/HLSL；业务文字实例和绘制继续位于 `pomelo-render`，不增加 GPUI 的 PCB 专用能力。

## 真实 BRD 离屏验证

案例：`E:/brd_cases/SS8633A_AMPB_FPC_DOE_V2_HVT_A_0423_1716.brd`，使用默认 UTF-8 解码。

测试从 AllegroImporter 读取源文字，加载编译嵌入的 stroke 字体，执行文字布局及实例准备，再通过 TraceRenderer 在硬件 D3D11 离屏目标绘制。像素回读仅用于测试，不是产品视口呈现路径。

```powershell
$env:POMELO_TEXT_BOARD_PATH = 'E:/brd_cases/SS8633A_AMPB_FPC_DOE_V2_HVT_A_0423_1716.brd'
python -X utf8 scripts/cargo.py +stable test -p pomelo-render --all-features --lib hardware_ --locked --offline -- --ignored --nocapture
```

实际结果：

```text
REAL_BOARD_TEXT_GPU objects=9 instances=373 uploaded_bytes=47744 frames=1 visible_pixels=259
test result: ok. 2 passed; 0 failed
```

测试断言全部文字实例完成上传、红色非空像素数量大于零，第二次绘制像素完全一致、上传字节未增加、缓存构建次数保持一次。测试目标为 128 像素的离屏目标；像素数量不是字体质量或可读性指标。

该分支只有设置 `POMELO_TEXT_BOARD_PATH` 才执行，普通硬件测试通过不能替代这条真实板证据。测试会在 256 帧内检查上传完成；本案例实际只需一帧。

## 本轮回归

缺字恢复硬件验证：BoardRenderer 合成 fixture 包含源对象 999 的韩文 `한`（核心字体缺字）和对象 1 的 A。恢复布局准确返回一个对象 999 的警告，仅对象 1 进入实例批次；A 随后通过原有合成颜色、图层显隐、缓存及 reset 后像素恢复断言。更新后显式硬件测试两项通过。这证明混合 fixture 的恢复结果能进入硬件绘制，不宣称真实 BRD 缺字窗口已验收。

## 中型与大型真实板上传

硬件分支新增可选 `POMELO_TEXT_BOARD_ENCODING`（支持的编码 tag），并改用产品当前的字体/布局恢复路径。本组案例要求缺字诊断为空，确认没有跳过对象；仍沿用 4,000,000 实例预算和最多 256 帧的测试上限。

| 案例 | 编码 | 源文字 | GPU 实例 | 上传字节 | 上传帧数 | 非空红色像素 |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| USBC_FPC.brd | 默认 UTF-8 | 482 | 44,841 | 5,739,648 | 2 | 1,555 |
| AGILEX_I_SERIES.brd | windows-1252 | 46,480 | 3,094,215 | 396,059,520 | 95 | 11,127 |

USBC 在严格路径完成验证；AGILEX 在严格及当前产品恢复路径分别通过，统计一致。每次命令显式运行 hardware_trace_pixels，一项通过；重复帧像素一致、上传字节不增加、缓存构建仅一次。Clippy 通过。

复现 AGILEX：

```powershell
$env:POMELO_TEXT_BOARD_PATH = 'E:/brd_cases/AGILEX_I_SERIES.brd'
$env:POMELO_TEXT_BOARD_ENCODING = 'windows-1252'
python -X utf8 scripts/cargo.py +stable test -p pomelo-render --all-features --lib hardware_trace_pixels --locked --offline -- --ignored --nocapture
```

此处帧数是离屏测试调用次数，未按显示器刷新节奏运行，不能换算交互帧率或加载时间。128 像素目标中的非空像素只证明绘制产生内容；396 MB 是实例累计上传载荷，不是峰值显存测量。大板窗口流畅度、DPI、LOD 和完整图元合成性能仍未验收。

资源恢复验证：独立 BoardRenderer 完成字形与 PCB 合成后调用 reset，文字上传实例数归零；随后重新绘制三帧，最终整个目标像素与 reset 前一致。文字缓存及管线构建次数各为两次，累计上传字节恰为初次的两倍，实例数恢复到原值。更新后显式硬件测试 2 项通过。此为同一 D3D11 设备上的主动资源重建，不等同于实际设备移除、驱动重启或 GPUI 窗口恢复验收。

产品运行诊断的 scope 已补充 texts，与已有 text_statistics 和 expected_text_instances 字段一致。

BoardRenderer 的文字合成测试现使用嵌入核心字体的真实 A 字形，经 PreparedTexts 和 build_texts 生成实例，取代借用尺寸线实例的旧测试。与已有走线/铜皮合成时，字形保持蓝色图层颜色，不受相同数值 PCB ObjectId 的选择高亮影响；重复绘制像素一致且没有新增上传。隐藏文字层后无蓝色像素，恢复后整个目标像素与此前完全一致，上传字节保持不变。移除文字 source 后缓存清空一次，后续空帧不重复 reset。更新后显式硬件测试 2 项通过。这仍是离屏合成证据，未验证 GPUI 弹窗遮挡或真实窗口交互。

- renderer 全特性库测试：53 通过，3 个外部资源或硬件项明确 ignored。
- 上述显式硬件命令：2 通过，包含真实板文字分支。
- 工作区全目标、全特性 Clippy：通过，`-D warnings`。

## 验证边界与后续工作

本轮证明真实源文字能够经过布局、实例上传和 D3D11 绘制生成非空画面，并复用缓存。它没有验证 GPUI 完整窗口中的文字与铜皮、焊盘、走线合成，也没有逐字与 Web 字形对照。

仍需验证真实窗口缩放、图层显隐、DPI、文字清晰度及遮挡顺序；大板文字需单独测上传、峰值内存、帧耗时及 LOD。当前字体缺少韩文字形；界面五语支持与源 PCB 字形覆盖是不同要求。字体来源与发布许可审查见 [pcb-text-font-provenance.md](pcb-text-font-provenance.md)，本轮 GPU 测试不改变其审查状态。
