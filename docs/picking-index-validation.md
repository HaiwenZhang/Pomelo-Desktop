# Windows 走线拾取索引验证

## 四类图元混合候选补充验证

新增 `real_board_mixed_candidates_match_category_reference_queries`，从走线端点、pin/via 原点和 Zone 填充顶点各采样最多 32 个位置，分别查询对象候选、排除 net 0 的网络候选、隐藏源图层后的对象候选。统一查询上限 20、容差 0.15 mm，对照独立分类查询与走线全扫描的稳定合并结果；比较类别、源 ID、距离及顺序。分类几何函数仍共用，因此此项验证候选合并，不证明曲线/边界精度或 GPU 几何一致性。

FPC 的 333 项查询全部一致（394 段、90 pin、476 via、15 Zone），一轮优化 debug 查询 P95 0.0149 ms、最大 0.0239 ms。原始记录见 [混合候选 FPC JSON](gpu-validation/picking-mixed-fpc.json)。计时不含参考查询、导入、UI 排队和 GPU/显示反馈，尚不是 release 性能验收。

AGILEX（显式 Windows-1252）的 384 项查询全部一致（146,571 段、16,978 pin、15,956 via、830 Zone），同轮优化 debug 查询 P95 **82.3246 ms**、最大 **96.4105 ms**，原始记录见 [混合候选 AGILEX JSON](gpu-validation/picking-mixed-agilex.json)。仅 CPU 查询已超 50 ms 目标；现有混合路径除走线外仍扫描，应优先建立 pad/via/Zone 空间索引，并分别测量其阶段耗时。不能用先前纯走线索引结果宣称混合拾取达标，也不能据此直接断言具体某类图元占用了全部时间。

显式复验时按下述环境变量设置案例、编码与独立报告路径，并指定 `real_board_mixed` 测试过滤器，避免两项忽略测试写入同一报告。

### AGILEX 单类别耗时定位

**后续优化复验：** 新增 `ZoneFillIndex`，构建时验证源轮廓并缓存各环边界，按外环边界排除 Zone、按孔洞边界排除无关环；查询继续使用原始填充顶点与外环减孔洞并集语义。没有复制或重建 mesh，额外边界内存纳入拾取索引预算。当前扫描的是 Zone/环边界元数据，尚未建成 BVH。

相同 384 查询全部与参考一致，新一轮优化 debug P95：Zone **2.0793 ms**（最大 4.8071 ms），全类别 **6.7171 ms**（最大 7.6324 ms）；pin 1.2070 ms、via 3.8835 ms、segment 0.0375 ms。原始数据见 [Zone 边界索引复验](gpu-validation/picking-mixed-agilex-zone-index.json)。性能已显著改善，但不是 release 多轮/实窗反馈验收；曲边仍使用填充近似且无边界容差。下表保留优化前记录。

同样 384 个位置/状态，额外分别启用一个类别执行生产 `query_objects` 路径；这些是独立重复查询，不是总查询内嵌计时，P95 不可直接求和。优化 debug 一轮结果：

| 类别 | 查询 P95 ms | 最大 ms |
| --- | ---: | ---: |
| pin | 1.2263 | 5.0623 |
| via | 3.7644 | 5.3981 |
| segment | 0.0405 | 0.0644 |
| Zone | 65.0924 | 84.1011 |
| 全类别 | 68.1289 | 90.4985 |

384 项混合参考比较仍全部通过，工作区 Clippy 通过。原始坐标、候选与各类别计时见 [AGILEX 分项记录](gpu-validation/picking-mixed-agilex-phases.json)。结果把当前主要瓶颈定位到 Zone 填充扫描；应优先构建经过验证的 Zone 边界空间索引和孔洞候选索引，再评估 pin/via 索引。不能仅使用缓存的源边界静默跳过非法几何；索引构建应验证源数据并保留取消、内存预算和源对象错误身份。后续仍需 release 多轮及含 UI 反馈的验收。

日期：2026-10-02。验证范围为 CPU 走线索引与精确参考扫描的候选一致性，不代表画布鼠标拾取、完整图元拾取或产品性能门槛已经通过。

使用 `picking_real_board` 显式忽略测试导入真实 BRD，采样 128 个源走线，分别查询源端点、线段/解析圆弧中点，以及隐藏源图层后的中点。容差 0.15 mm，候选上限 20；比较全部返回候选的源 ID、顺序及距离。每板 384 个查询全部一致。测试和参考扫描共用精确距离函数，因此不能证明该函数与 GPU 画面或源 EDA 的几何一致性。

| 案例 | 编码 | 走线数 | 构建 ms | 索引查询 P95 ms | 参考扫描 P95 ms |
| --- | --- | ---: | ---: | ---: | ---: |
| SS8633A FPC | UTF-8 | 394 | 0.0360 | 0.0022 | 0.0049 |
| AGILEX I SERIES | 显式 Windows-1252 | 146,571 | 17.6217 | 0.0783 | 3.7356 |

这是一轮优化 debug 测试，不是 release 三轮性能基线。P95 取排序后 `floor(0.95 * (N - 1))` 项，测量不含文件读取、导入、UI 任务排队、相机转换、GPU 绘制或屏幕反馈。源哈希、每次查询坐标、候选和耗时保存在 [FPC 原始记录](gpu-validation/picking-fpc.json) 与 [AGILEX 原始记录](gpu-validation/picking-agilex.json)。

复验设置 `POMELO_PICK_CASE` 为案例绝对路径、`POMELO_PICK_REPORT` 为输出 JSON 绝对路径。AGILEX 另设 `POMELO_PICK_ENCODING=windows-1252`；UTF-8 案例不设置该变量。运行：

```powershell
python -X utf8 scripts/cargo.py +stable test -p pomelo-import --test picking_real_board --locked --offline -- --ignored --nocapture
```

仍待完成：鼠标接入、重叠候选切换、对象/走线/网络/元件四模式、焊盘/铜皮命中、GPU 像素对照、真实 DPI 交互、release 延迟与内存基线。索引排序目前只在前后检查取消。

## 当前 Windows 构建与启动复验

### 候选切换版本启动补充

Zone 边界索引、四模式重复点击切换、画布 `N` action 和五语候选序号接入后，Windows debug 构建通过。可执行 SHA-256 为 `56bf62780770d34655adbbd6c9ac427a371087807a764e0b308b6fcf248d1e81`。以 `--locale zh-CN` 打开 FPC，记录 [候选版本运行 JSON](gpu-validation/picking-candidates-fpc-runtime.json) 显示 GPU ready、提交/呈现各 5 次、RTX 5080 硬件适配器、无 GPU 错误；选择目标为空，默认模式 Net。

本轮原生 `sky.list_windows` 清单未返回 Pomelo，无法通过该工具验证实际点击、N 键、候选序号和检查器布局。上述 JSON 仅证明准备/上传/呈现链路运行。已核对本轮测试进程 PID 31704 与 workspace 可执行路径后停止；启动 shell 退出 1 来自测试结束时的主动停止，不作为程序崩溃证据。下文为较早版本记录。

同日工作区 `test --workspace --all-features --locked --offline` 通过；其中忽略的实机/外部样本测试不计入该结论。两项 `hardware_` D3D11 像素测试另以 `--ignored --nocapture --test-threads=1` 显式通过，Windows debug 构建通过。

当前程序 SHA-256：`15aa013dd46c296325e78f56e9518946a1385e6133fc91b9e8431059225c969c`。使用 `--locale zh-CN` 打开 FPC，运行记录 [picking-fpc-runtime.json](gpu-validation/picking-fpc-runtime.json) 显示索引 394 段、搜索 33 条，GPU ready、提交/呈现各 5 次、无错误，适配器 NVIDIA GeForce RTX 5080。完成记录后按本次启动 PID 和绝对可执行路径核对并停止测试进程，关闭产生的非零 shell 退出码不作为程序启动失败。

该记录证明当前文档准备和 GPU 呈现链路运行，不证明左键事件、目标高亮、键盘焦点或检查器布局已经实窗验收。初始走线网络左键路径已接入源码，其余模式及图元仍待完成。
