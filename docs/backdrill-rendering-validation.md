# 背钻图元渲染与交互验证

> 本文保留历史验证记录，其中引用的辅助脚本及 pomelo-core/import/render 的 example 探针已移除；旧探针命令不再可执行。当前验证使用各 crate 的测试代码。

日期：2026-10-03。参考最新本地 Web `C:/Users/Zen/Desktop/gitrepo/pomelo`。Web 与外部 BRD 只读，UI 字体保持原状。

## 实现

- `scene/pads.rs` 不再把各切除层的背钻标记作为普通圆盘提交；保留原层焊盘与背钻 base，切换显示时无需重新准备几何。
- `scene/drills.rs` 在普通孔之后追加每个背钻过孔的一个独立圆形图元，包括外径、原钻孔内径、类型化 via 身份及切除层范围。双端背钻仍只有一个网纹实例，不按层重复叠加。
- Windows `pad.hlsl` 按 Web 的屏幕空间 8 逻辑像素交叉网纹绘制绿色环和白色网纹，网纹覆盖孔中心。DPI、翻板和缩放不改变网纹的逻辑间距；选中/悬停仍使用原圆形轮廓覆盖层。
- 背钻开关独立于普通“显示钻孔”。背钻在至少一个切除层的过孔类别可见时显示；只显示保护层不会显示背钻。关闭背钻时恢复切除层原焊盘，填充开关作用于这些焊盘。
- 画布拾取采用相同开关、切除层范围和 base 恢复规则，背钻归所属过孔，不建立额外业务对象。自动过孔标签只依赖源层范围，不依赖普通钻孔开关。
- 显示面板使用既有 GPUI Kit Checkbox/FocusScroll，新增五语“显示背钻”；状态持久化为 `show_backdrills`，旧配置缺字段时默认开启。开关清理陈旧 hover。

业务几何与显示/拾取在平台无关模块；Windows 资源提交继续复用 GPUI 的 D3D11 设备，HLSL 位于 `pomelo-render`，没有引入 wgpu。

## 自动验证

| 验证范围 | 当前证据 |
| --- | --- |
| 双端背钻图元数与顺序、内外径及切除层范围 | 新增 CPU 回归通过 |
| 逐层 marker 排除与 base 保留 | CPU 回归通过 |
| 实例/字节预算、取消、无效直径与源对象错误身份 | CPU 回归通过 |
| 拾取：填充/非填充、独立钻孔开关、切除层/保护层、隐藏过孔类别 | CPU 回归通过 |
| 配置往返与旧配置默认开启 | 既有持久化回归扩展后通过 |
| 工作区全部特性测试 | 401 通过、0 失败、13 项硬件/外部案例默认忽略 |
| D3D11 硬件像素 | 显式执行 1 项测试，128 组像素对照全部通过 |
| 既有图元硬件回归 | 自定义焊盘边界、铜皮重叠孔洞、走线端帽/圆弧/裁剪/缓存三项显式通过 |
| fmt、全目标全特性 Clippy `-D warnings`、Windows debug/Release 构建 | 通过 |

硬件测试使用正式 `BoardRenderer` 与 `pad.hlsl`，128×128 目标、位移画布与内缩裁剪；独立 f64 参考按“蓝色原焊盘 → 灰色普通孔 → 绿色背钻网纹”合成。矩阵为保护层、两端切除层、全部隐藏 × 背钻开关 × 普通钻孔开关 × 填充开关 × DPI 1/2 × 翻板。容许最大 RGB 误差 ≤4、平均误差 ≤0.1，实际最大误差 1、最高平均误差 0.003663。128 次显示切换后焊盘和钻孔缓存各构建一次，钻孔实例上传 256 字节。

完整日志与报告位于 `.cache/canvas-parity/backdrill-hardware.log`、`backdrill-hardware/d3d11-backdrill-patterns.json`、`backdrill-workspace.log`、`backdrill-clippy-final.log`、`backdrill-build.log`，三项既有硬件回归日志为 `backdrill-regression-*.log`。硬件测试须启用 `--all-features`；未启用原生 GPU 特性而执行到 0 项测试不计验证成功。

## 真实 Windows 窗口

使用只读 `AGILEX_I_SERIES.brd`，显式 Windows-1252。案例 SHA-256 为 `3e352e8269cdc5be0f4054d9344205fc36609b80a49c3e2c26abb0ed487ad16a`，Web 解析找到 552 个背钻过孔。

同一 debug 二进制 `b49fe53fb55e36beed5847aa52bb73cafee8c9d44bc843070fa8d1b856d295f2` 的私有副本在 650 逻辑像素/mm 下验证：

1. 只显示切除层 7，关闭普通孔，中央背钻仍有绿色环和白色交叉网纹；自动标签 B22-8-7 保留。
2. 点击圆中心选中 Via 313287，源坐标 `(131.4577, 89.522808)`；选中轮廓正确，标签不变为单独拾取对象。
3. 打开普通孔时，网纹仍覆盖灰色孔中心。
4. Tab 从既有工具栏按顺序进入填充、钻孔、背钻控件；背钻焦点可见，Space 关闭后恢复普通层焊盘。
5. 正常关闭再打开同一私有配置，背钻关闭与选中 Via 313287 恢复；重新打开背钻、关闭普通孔后恢复网纹中心。

重开后的冻结遥测证明 GPUI_D3D11、NVIDIA GeForce RTX 5080、硬件渲染、ready、提交/呈现与 `last_error: null`。显示切换前后走线、焊盘、钻孔缓存各为 1，上传量分别固定为 18,767,360 / 47,793,920 / 2,146,560 字节。标签属于动态视图资源，不把其缓存变化解释为板几何重传。

证据包括 `backdrill-selected.jpg`、`backdrill-keyboard-off.jpg`、`backdrill-restored.jpg`、`backdrill-enabled.jpg`、`backdrill-no-drills.jpg`、`backdrill-window-restored.json`、`backdrill-window-enabled.json`、`backdrill-window-no-drills.json`、`backdrill-views-restored.json`。私有配置为 `.cache/ui-validation/profiles/e8bd8832b36a498c93e0d4d96accc110`。用户开始操作该预览后已停止自动输入，窗口保留；日常偏好与其他窗口未操作。

## Web 拾取对照与边界

`scripts/check-canvas-picking-parity.mts` 新增 `backdrills` 采样模式：所有 552 个背钻，各取中心、环内、环外和斜向点，在 100/650/3000 逻辑像素/mm、8 个显示状态下，对照首选对象及 object/track/net/component 四模式。直接运行最新 Web `BoardIndex`，不重写它的拾取算法。共 52,992 次查询，包含关闭普通孔、关闭背钻、非填充、隐藏切除层、仅切除层可见及两种孔同时关闭。

**全量对照已完成：52,992 次全部匹配，0 差异。** 最终报告为 `.cache/canvas-parity/backdrill-agilex-final/report.json`，原生输出为同目录 `native.json`；终态日志为 `backdrill-picking-final.stdout.log`。报告保存 BRD 和实际运行 Web 源文件的 SHA-256。作业信息在 `backdrill-picking-job.json`，进度在 `backdrill-picking-final.stderr.log`。此前使用更低缩放的运行被中断，只有 request，没有结果，不计入成功。

参考 Web、原生源文件、BRD、应用与探针指纹位于 `backdrill-provenance.json`。以上证据覆盖背钻专项；仍未完成整板 WebGPU/D3D11 截图比较、特殊对象全部组合、性能/设备恢复/发布验收和新增控件五语实窗矩阵。

Windows Release 构建也已完成，日志为 `backdrill-release-build.log`。`target/release/pomelo.exe` SHA-256 为 `e11ed0071a5339cd3b9daad588cbc9ee63b7e1dad1acaf4a54cae6d4b4806293`，PE 子系统为 2（Windows GUI）。该 Release 未新增实窗验证，不能将上述 debug 窗口证据直接计为 Release 发布验收。

## 复验

```powershell
$env:POMELO_CANVAS_GPU_REPORT_DIR = Join-Path $PWD '.cache/canvas-parity/backdrill-hardware'
cargo +stable test -p pomelo-render --all-features --lib --locked --offline hardware_backdrill_patterns_match_web -- --ignored
cargo +stable test --workspace --all-features --locked --offline
cargo +stable build -p pomelo-render --example canvas_pick_probe --locked --offline
& C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/.bin/tsx.cmd scripts/check-canvas-picking-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo target/debug/examples/canvas_pick_probe.exe E:/brd_cases/AGILEX_I_SERIES.brd .cache/canvas-parity/backdrill-agilex-final windows-1252 backdrills
```
