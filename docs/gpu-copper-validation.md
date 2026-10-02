# Windows 铜皮 GPU 接入验证

日期：2026-10-02。实现位于 `crates/pomelo-render`，复用 GPUI 的 D3D11 设备、窗口目标和呈现入口。

## 硬件覆盖测试

显式运行 `hardware_copper_overlapping_holes_preserve_underlying_trace` 通过，使用硬件 D3D11 设备，无软件回退。检查重叠孔洞的并集保持透明、孔洞下方红色走线保留、单孔透明、半透明铜皮填充，以及缓存重复绘制的全部像素一致。诊断回读只存在于测试中，生产窗口不进行全帧 CPU 回读。

追加并重新显式运行的硬件检查通过：主动 renderer reset 后当前上传量归零，管线和缓存重建后全部像素与原图一致；第二个区域位于前一区域的孔洞内时仍正确填充，并与下方走线进行半透明混合，证明区域间遮罩清理生效；父级右半画布裁剪排除左侧铜皮且保留右侧填充，裁剪变化不重建几何缓存。渲染 crate 全 target / feature Clippy `-D warnings` 再次通过。主动 reset 不等同于真实设备丢失、跨设备重建或窗口 resize 的验收。

## 真实 FPC 窗口提交

案例 `SS8633A_AMPB_FPC_DOE_V2_HVT_A_0423_1716.brd`。应用 debug 二进制 SHA-256：`01ca94b7223f290c47c6b62f626b8c4dc220aa5cf7205b163cd8c6463d288196`。

NVIDIA GeForce RTX 5080 上运行结果：394 个走线源段、16 个板框源段，合计 410 个实例、52,480 字节；15 个铜皮区域、410 顶点、1,140 索引，上传 11,120 字节。铜皮管线和缓存各构建一次，45 次 draw call，15 个区域参与绘制；窗口 submitted=2、presented=2、ready=true、last_error=null。原始报告见 [copper-fpc.json](gpu-validation/copper-fpc.json)。

初次隐藏窗口测试进程存活但没有视口报告，不能作为 GPU 成功证据；关闭该测试进程后用普通窗口运行取得上述报告。测试结束后关闭本次启动的进程。

## AGILEX 分帧上传

同一 debug 二进制使用 `--locale ja --encoding windows-1252 E:/brd_cases/AGILEX_I_SERIES.brd` 实际运行。案例 SHA-256：`3e352e8269cdc5be0f4054d9344205fc36609b80a49c3e2c26abb0ed487ad16a`。

830 个铜皮区域、12,147,361 顶点和 35,722,179 索引完成 337,246,492 字节上传；走线与板框共 146,620 个实例完成 18,767,360 字节上传。窗口 submitted=presented=86、ready=true、last_error=null；铜皮管线与缓存各构建一次，累计 79,760 次铜皮 draw call，最后一帧有 830 个区域参与绘制。再次读取报告计数不变，测试后关闭本次进程。原始报告：[copper-agilex.json](gpu-validation/copper-agilex.json)。

这是上传与呈现证据，没有测量 release 帧耗时、峰值 CPU/GPU 内存或进行真实板视觉对照，不能作为大板性能与正确性完整验收。

## 尚待完成

最新逐层合成实现：`BoardFrame.layer_order` 表示从后到前的顺序，组合 renderer 在每层先绘制铜皮、再绘制该层走线，全部层结束后绘制板框。重复层 ID 去重，源数据中未列出的层按源批次顺序追加，避免静默丢失图元。桌面默认沿用领域模型的图层顺序；用户调整图层顺序的界面仍待开发。

硬件像素测试已验证交换层顺序后，上层半透明铜皮覆盖下层走线并按预期混合，重复层 ID 不会重复混合；改变顺序不增加上传字节数，不重建走线或铜皮缓存。渲染测试和工作区 Clippy 通过。下述早期“整体铜皮先绘制”的实运行证据对应旧版本；新逐层合成尚须真实板运行和视觉复验。

资源准备与绘制已拆分，组合入口先统一 prepare，再执行各类 draw。新增实际桌面组合 payload 的硬件检查通过：首帧走线上传期间铜皮上传量为零，第二帧铜皮上传完成，走线覆盖铜皮的像素正确，两类缓存各创建一次。此次渲染 crate 全部 22 项测试包含两项显式硬件测试通过，工作区 Clippy 通过。该拆分为逐层合成提供基础，并不表示逐层合成已经完成。

当前铜皮整体先绘制，走线和板框随后绘制；最终逐层合成、曲边抗锯齿、真实窗口视觉对照、DPI 与设备恢复仍待完成。本记录证明代表板实际上传、回调提交和窗口呈现，不证明全部 PCB 画面正确，也不代表 BRD MVP 已完成。

工作区全 feature 测试、全 target / feature Clippy `-D warnings`、Windows debug 构建通过；日志位于 `.cache/copper-viewport-{tests,build}.log`。
