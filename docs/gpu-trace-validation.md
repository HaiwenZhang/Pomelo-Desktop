# Windows 真实走线 GPU 接入验证

日期：2026-10-02。当前只实现 Windows / D3D11 / HLSL。真实 BRD 走线与圆弧已接入桌面视口；焊盘、过孔、铜皮、板框和文字尚未接入正式视口，不能据此认为完整 PCB Viewer 已验收。

## 实现边界

GPUI 补丁仍只提供通用 renderer 注册、绘制、状态隔离与资源生命周期。`pomelo-render` 持有 128 字节走线实例、图层批次、HLSL、D3D11 管线与不可变缓冲；应用视口借用 GPUI 同设备上下文，GPUI 负责窗口目标和呈现。

后台准备 CPU 批次，单个 GPU 上传块最多 16,384 个实例，每帧最多上传两个块（4 MiB）。相机、DPI 和颜色变化复用已上传几何；场景引用变化重建缓存，renderer reset 后重新创建资源。坐标采用高低位拆分，圆弧保留方向、长弧和完整圆标志。新增视口范围与上传进度消息同步提供英、简中、繁中、日、韩译文。

## 硬件像素验证

在 NVIDIA GeForce RTX 5080 上显式创建硬件 D3D11 设备，运行与生产相同的 TraceRenderer/HLSL，绘制 128×128 离屏目标。该测试单独执行诊断 CPU 回读，生产窗口路径不执行全帧 CPU 回读。

```powershell
$env:POMELO_TRACE_PIXEL_OUTPUT = '.cache/native-trace-pixels.png'
python scripts/cargo.py +stable test -p pomelo-render --all-features hardware_trace_pixels --locked --offline -- --ignored --nocapture
```

结果：1 项硬件测试通过。检查圆端帽、完整圆周及透明中心、负向四分之一圆弧、抗锯齿覆盖、裁剪、DPI、缓存复用、reset 后图像一致、大坐标平移和翻转。另用 32,771 个实例验证跨帧上传及同一缓冲中不同图层的非零起始偏移。

早期失败来自取样点与另一条绿色圆弧重叠；改为目标圆弧被排除的左侧圆周、避开其他图元后通过，未放宽像素容差。

![走线与圆弧诊断图](gpu-validation/trace-pixels.png)

机器记录：[trace-pixels.json](gpu-validation/trace-pixels.json)。这些结果不证明所有 BRD 画面、任意极端坐标或真实设备丢失恢复正确。

## 真实 BRD 窗口提交

案例：`SS8633A_AMPB_FPC_DOE_V2_HVT_A_0423_1716.brd`，SHA-256 `97684af89fb6fe5858963237c0c6e745b8b0e017b692ae3c0ac5262f00fbc3a4`。Windows debug 应用 SHA-256 `cf18b774a7d18d03fe623d41608a887f8660d9166704e245bebbea2a5fc248c4`。

最终构建实运行结果：394 个走线实例，上传 50,432 字节，管线与缓存各创建一次，2 次业务 draw call；GPUI 报告 submitted=1、presented=1、last_error=null。独立再次读取报告，静止计数不变。原始记录：[trace-fpc.json](gpu-validation/trace-fpc.json)。

此证据证明真实案例进入 GPU 回调并由窗口呈现，不等同于真实板截图或逐像素对照。窗口自动化在原生文件对话框定位失败，启动参数运行的窗口未被工具枚举，故本轮没有宣称实窗视觉验收。

### AGILEX 编码与多帧上传复验

初次启动没有视口报告，原因已定位：默认 UTF-8 在源偏移 `0x1e9bfc` 解码失败，CLI 独立解析同样返回结构化编码错误。保持严格 UTF-8 默认，不做自动编码猜测；新增桌面 `--encoding` 显式选项后使用与冻结 Web 对照一致的 Windows-1252 复验：

```powershell
$env:POMELO_BOARD_GPU_TRACE = '.cache/native-trace-agilex-1252.json'
target/debug/pomelo.exe --locale ja --encoding windows-1252 E:/brd_cases/AGILEX_I_SERIES.brd
```

真实结果：146,571 个走线实例，18,761,088 字节上传，submitted=presented=5，last_error=null；管线与缓存各构建一次，累计 58 次业务 draw call。再次读取计数保持不变。机器记录：[trace-agilex.json](gpu-validation/trace-agilex.json)。案例 SHA-256 `3e352e8269cdc5be0f4054d9344205fc36609b80a49c3e2c26abb0ed487ad16a`；复验应用 SHA-256 `2159cdd21f43f5d17cf0e2b62380df978194a937efab571d41d48d58ee42d533`。

该结果仅证明真实大板导入、分帧上传与窗口提交完成，不是帧率、峰值内存、视觉或完整大板交互验收。编码选择不随 `--locale ja` 改变；此启动选项应用于该次进程打开的文件，文件对话框中的逐文档编码选择及失败重试仍待实现。

## 工作区检查与剩余项

最新全工作区 all-features 测试 224 项通过、1 项硬件测试默认忽略；该硬件项已另外显式通过。新增启动参数测试覆盖语言/编码分离、选项值不作为文件、`--` 路径分隔和五语参数诊断。Clippy 全目标/全部特性 `-D warnings`、格式化与 Windows debug 应用构建通过。工具链 rustc 1.98.1，GPUI Kit 0.7.0，GPUI pre 0.3.7。

后续继续接入焊盘/过孔、铜皮孔洞并集、板框和文字，完成导航、图层控制、拾取与搜索，再进行真实板画面对照、大板性能、设备故障和文档关闭资源回收验收。macOS/Linux 保持后续规划。
