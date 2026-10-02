# 紧凑文字：15061 Windows GPU 验证

日期：2026-10-02。案例：`E:/brd_cases/15061-1b.brd`，文本解码 Windows-1252。

## 本次实际执行

专用硬件测试 `hardware_real_board_compact_text_upload_draw_and_reset` 使用正式 `PreparedTextInstances`、`TraceRenderer<PreparedTextInstances>`、D3D11 分块缓存及 `shaders/text.hlsl`。调用 D3D11 HARDWARE 创建设备，没有 WARP 回退；在 128×128 离屏目标读取诊断像素。正式应用的 CPU 准备与 BoardRenderer 已使用紧凑来源，本测试单独验证文字管线。

| 验证项 | 结果 |
| --- | --- |
| 源文字对象 | 69,797，全部准备，缺字诊断为 0 |
| 紧凑实例 | 5,473,611，64 字节/实例 |
| 首次上传 | 350,311,104 字节，168 次帧回调，每次最多两块 |
| 可见文字像素 | 14,646 个红色像素 |
| 重复绘制 | 全部像素一致，上传字节不增加，缓存只构建一次 |
| 改色和半画布裁剪 | 7,450 个绿色像素，红色为 0，裁剪外绿色为 0；上传字节不增加 |
| reset 后重建 | 管线及缓存各重建一次，首回调重新上传最多 32,768 实例 |
| reset 后完整恢复 | 再次完成全量上传，168 回调；像素与 reset 前裁剪画面完全一致 |

原始记录：[text-compact-15061-hardware.log](gpu-validation/text-compact-15061-hardware.log)。最后一次执行 1 项通过、0 失败。工作区 Clippy 通过。

复现命令（PowerShell，工作区根目录）：

```powershell
$env:POMELO_TEXT_BOARD_PATH = 'E:/brd_cases/15061-1b.brd'
$env:POMELO_TEXT_BOARD_ENCODING = 'windows-1252'
python -X utf8 scripts/cargo.py +stable test -p pomelo-render --features native-gpu hardware_real_board_compact_text_upload_draw_and_reset --locked --offline -- --ignored --nocapture
```

## 证据边界

这是带优化的 test profile 单次硬件正确性执行。帧回调数是分块上传次数，不能解释为实际窗口帧率；没有测量产品视口 GPU 耗时、进程峰值内存或物理显存释放。统计中的上传字节只覆盖文字实例。

128×128 全板缩放结果只证明全量管线和可见输出，不证明每个字形在所有缩放、DPI、镜像和五语界面条件下的视觉质量。本次不绘制走线/焊盘/铜皮整板合成，不包含弹窗覆盖、交互、真实窗口、设备故障注入或发布包验收。reset 是显式 renderer 生命周期接口，完整重传像素一致不等同于真实驱动设备丢失已经验收。

先前 FPC 新旧布局逐像素对照见 [text-compact-pixels-fpc.log](gpu-validation/text-compact-pixels-fpc.log)。15061 没有绕过旧预算构建 128 字节来源作像素对照；其输入规模与紧凑 CPU 几何实验一致，完整领域字段正确性仍按导入回归证据评估。
