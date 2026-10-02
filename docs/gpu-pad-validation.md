# Windows 解析焊盘 GPU 接入验证

最新进展：自定义焊盘网格也已接入组合 renderer；显式硬件测试确认带孔轮廓外环填充、孔洞保留下方颜色，自定义上传等待解析焊盘完成。新增自定义 telemetry 纳入视口完成状态和字节进度，工作区测试与 Clippy 通过。以下真实案例报告属于此前仅解析焊盘版本，新版本 AGILEX 自定义焊盘运行仍待验证。曲边抗锯齿和钻孔显示未完成。

日期：2026-10-02。解析焊盘与过孔各层 pad 已接入 `pomelo-render` 的 D3D11/HLSL 管线，复用 GPUI 设备与窗口目标。自定义焊盘、钻孔显示和文字仍未接入。

## 硬件像素检查

两个 128 字节实例实际上传并绘制：绿色圆盘中心填充，蓝色环形中心透明而圆环填充；缓存重复绘制全部像素一致。组合 renderer 的对应图层绘制也通过检查。测试使用诊断离屏目标和专用 CPU 回读；产品窗口不执行全帧 CPU 回读。该证据不覆盖所有 shape 家族、旋转、极端缩放或完整抗锯齿效果。

## 真实案例提交

最新自定义焊盘版本实运行：debug 二进制 SHA-256 `b50437bcd8b9e105c9b0b77dfdc744494c149414952837a2c160bcad5e0d82dd`。AGILEX 的 122 个自定义放置输出 4,329 顶点、12,255 索引，实际上传 118,284 字节，独立管线与缓存各创建一次，366 次 draw call，最后一帧 122 个区域参与绘制。全部类别完成后 ready=true、submitted=presented=99、last_error=null、custom_gpu_drawn=true。FPC 没有自定义焊盘，仍正常 ready=true、呈现 3 帧，无自定义缓冲或管线创建。原始记录：[AGILEX 自定义](gpu-validation/custom-pads-agilex.json)、[FPC 空自定义](gpu-validation/custom-pads-fpc.json)。两次测试进程已关闭。

此结果更新了前文“真实运行待复验”的状态；下面表格和二进制哈希保留仅解析焊盘版本的历史证据。曲边抗锯齿、钻孔显示、真实板视觉对照和性能验收仍未完成。

Windows debug 二进制 SHA-256：`a1a34c8263f9376a964a34874edecfbcc4ba1c6db26318faba3c81f49b95d85c`。NVIDIA GeForce RTX 5080，后台先准备数据，组合 renderer 分帧依次上传走线、铜皮和解析焊盘，每帧业务上传预算最多 4 MiB。

| 案例 | 解析焊盘 | 焊盘上传字节 | 累计焊盘 draw call | 提交/呈现帧 |
| --- | ---: | ---: | ---: | ---: |
| FPC | 1,042 | 133,376 | 2 | 3 |
| AGILEX（显式 Windows-1252） | 382,766 | 48,994,048 | 298 | 98 |

两例均 ready=true、last_error=null，焊盘管线和缓存各创建一次，报告 `pads_prepared.gpu_drawn=true`。AGILEX 的 122 个自定义放置仅完成 CPU 准备，`custom_gpu_drawn=false`。原始记录：[FPC](gpu-validation/pads-gpu-fpc.json)、[AGILEX](gpu-validation/pads-gpu-agilex.json)。本次测试进程已关闭。

当前顺序为每层铜皮、走线、解析焊盘，板框最后绘制。上述记录证明真实案例上传、GPU 回调和窗口呈现成功，不证明实窗视觉正确或 release 性能达标。自定义网格、钻孔/背钻显示、全部形状像素对照、真实板视觉验收及设备生命周期完整验证仍待完成。
