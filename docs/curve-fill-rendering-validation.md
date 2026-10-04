# 曲边铜皮深度缩放验证

> 本文保留历史验证记录，其中引用的辅助脚本及 pomelo-core/import/render 的 example 探针已移除；旧探针命令不再可执行。当前验证使用各 crate 的测试代码。

日期：2026-10-03。参考最新本地 Web `C:/Users/Zen/Desktop/gitrepo/pomelo`，Web 和外部 BRD 案例只读。桌面及 Web **UI 界面字体保持不变**；本次只扩展 PCB 铜皮几何、D3D11 合成和后台调度，没有更改字体或翻译资源。

## 实现与边界

- `pomelo-core/geometry/curve.rs` 保留 f64 源端点与连接边，按可见圆弧区间细分，再裁剪轮廓。物理比例超过 1000 px/mm 时启用，弦误差目标为 0.25 物理像素，按二次幂比例分档，覆盖翻板与 DPI。
- `pomelo-render/scene/curves.rs` 管理不可变快照、可见工作集、两倍视口缓存及 LRU。平移仍在缓存范围内、缩小仍满足精度时复用 `Arc`；新文档即使对象 ID 相同也不能复用旧场景几何。
- 静态铜皮继续使用指定的 `earcut = "0.4.11"`。裁剪后的曲边轮廓可能通过视口边界连接分离区域，采用 triangle fan 与 stencil 奇偶填充；每个孔独立求覆盖后从材料中移除，重叠或嵌套孔不会重新填回。板上自动标签使用同一最终铜皮遮罩。
- Windows 业务绘制位于 `pomelo-render/backend/d3d11`，复用 GPUI 的 D3D11 设备和呈现，不引入 wgpu。动态覆盖仍保留静态铜皮的图层、源顺序及选择身份。后台任务由视口调度，取消或过期结果不会发布；失败经已有结构化诊断和五语 i18n 消息契约呈现。
- 单环最多 1,000,000 个点，轮廓自有点缓冲峰值上限 64 MiB；准备缓存软预算 64 MiB、输出硬预算 512 MiB。可见工作集可超软预算，报告超额并淘汰非活动资源。硬预算核算准备几何缓冲及批次容量，不代表进程总内存上限，旧借用快照、容器管理开销和其他资源另计。
- 所有曲边对象共享每帧 4 MiB 几何上传预算，并等待静态几何、文字及标签就绪；缓存命中不重新上传。字体图集的独立上传预算仍是后续性能验收项。

2026-10-04：圆弧计算改用 Rust 标准 `f64::sin()` / `f64::cos()`。原有 Node/V8 移植代码、对应许可文件和 Windows 安装脚本引用已删除，项目不再直接依赖 libm。源端点及连接边保留原有处理规则；不再要求与 JavaScript 或其他平台逐位一致，末位舍入差异可能改变重复点去重后的数量。已删除依赖 V8 特定末位舍入的测试，保留端点连接、弦误差、裁剪和资源限制等几何行为测试。下方 Web 的零差异数据属于替换前的历史验证，不能作为当前实现的逐位对照结果。

## Web 几何对照

直接调用当前 Web 的 `tessellateCurveRing`、`ZoneShape` 与实际导入器，对照原生无窗口探针。运行时为 Node **24.18.1**、V8 **13.6.233.17-node.50**。

| 案例 | 编码 | 曲边铜皮 / 全部铜皮 | 对照次数 |
| --- | --- | --- | --- |
| USBC_FPC.brd | UTF-8 | 171 / 176 | 2,094 |
| camera_test_board.brd | UTF-8 | 34 / 49 | 426 |
| AGILEX_I_SERIES.brd | Windows-1252 | 354 / 830 | 4,564 |
| 合成圆与分离区域 | — | 97 个查询 | 97 |

合计 **7,181 次查询、77,676 个点，0 差异，最大物理坐标误差为 0**。真实板查询覆盖 1001/8192/50000 逻辑 px/mm、DPI 1/2、圆弧附近和外环中心，并采用两倍缓存视口及候选孔。合成场景覆盖顺逆时针、远坐标、大半径、最高 10⁸ 物理 px/mm、视口外轮廓和裁剪后分离区域。要求点数及顺序一致，坐标误差不超过 10⁻⁶ 物理像素；没有放宽端点差异或删除重复裁剪交点。

案例 SHA-256：

- FPC：`550cf739af80c2b5b18a4de91419adf2ecd1133cc2644eb0149d21b71e52b44b`
- camera：`45026cd064852090d3f7763f4cb751c66d6b3c472d90e1f8f927fbfb6b707d56`
- AGILEX：`3e352e8269cdc5be0f4054d9344205fc36609b80a49c3e2c26abb0ed487ad16a`

报告 `.cache/canvas-parity/curve-fill-parity/report.json` 保存五个参考 Web 源文件指纹和实际运行时；`request.json` / `native.json` 保存输入与结果。这是 CPU 几何对照，不能代替实际浏览器 WebGPU 截图比较。

## 真实 D3D11 验证

显式执行三项硬件测试，通过 **216 组、3,503,964 个独立解析覆盖像素**。覆盖圆盘、带重叠/嵌套圆孔的外环、裁剪后分离区域，三档比例 × DPI 1/2 × 翻板 × GPUI 裁剪 × 普通/选中/悬停颜色。独立解析预期排除距边界不超过 0.35 物理像素的区域，RGB 容差为 1；因此不代表曲边抗锯齿逐像素已对齐 Web。

另外验证：

- 铜皮最终遮罩排除 432 个孔内标签像素，保留 3,157 个材料内标签像素。
- 两个动态对象共 19,200,024 字节分 5 帧上传，每帧总量不超过 4,194,304 字节；不允许上传时没有新增资源。复用与资源重置后的重新上传通过。
- 普通静态铜皮的重叠孔洞/底层走线、die pad 显示分类两项硬件回归通过。
- 工作区全目标全特性 **418 项通过、0 失败、17 项默认忽略**；格式、全目标全特性 Clippy `-D warnings` 及 Windows debug/Release 构建通过。硬件测试另行显式执行，不以默认忽略作为通过依据。

证据为 `.cache/canvas-parity/curve-hardware.log`、`curve-static-copper.log`、`curve-static-die-pad.log`、`curve-workspace-test-final.log`、`curve-clippy-final.log`、`curve-debug-build-final.log` 和 `curve-release-build.log`。

## 窗口调度修正与验收限制

首次 Release 的真实窗口截图暴露循环：视角相关的曲边准备状态触发底部上传行出现/消失，画布高度在 714 与 741.3333 逻辑像素间变化，重新启动曲边任务与标签缓存。冻结旧报告时静止相机已有 29,544 帧、19,691 次标签构建、572,771,808 字节标签上传。

修正为仅由静态整板首次上传决定底部上传行；曲边/动态标签仍影响状态栏和完整 `ready`，但不改变画布高度。修正版私有配置呈现记录显示 NVIDIA GeForce RTX 5080、GPUI_D3D11、11 帧、无 GPU 错误；曲边 revision 稳定为 2，三个对象仅各上传一次，共 2,864 字节，标签构建稳定为 2 次。间隔读取快照未继续增加。

证据：`curve-window-before.json`、`curve-window-stable-a.json`、`curve-window-stable-b.json`、`curve-window-stable-final.json`、`curve-launch-final.json`。全部使用独立验证配置，没有修改日常偏好。Computer Use 能观察旧版窗口，但未在可操作列表中找到修正版窗口；**新版实窗截图、平移、翻板和点击拾取尚未验证**，以上遥测不能视为最终视觉或完整交互验收。冻结报告后核对进程路径，只停止本次私有预览实例；没有操作用户已有窗口。

最终二进制 SHA-256：

- debug：`ce2544e272213de42ea5036f2c903a3c95c0ffb7455206dd738450529b4a129a`
- Release：`4674579445d3495606de95dba4aa4b81551abdb88e57b5530486c19b12c77e89`
- 几何探针：`1d3e4dd6c4fad5d1f668c84dbac58e6e3a31d6543f677f54d53a0c0184ac10c3`
- 保持原状的 `crates/pomelo/src/theme.rs`：`2cb472958b85ea4a32316961c2902b535903697719b9743231743bf1e5c0ed70`

源码、探针、应用二进制、UI 主题、许可与安装脚本指纹冻结在 `.cache/canvas-parity/curve-provenance.json`，其中包含 Web 对照报告和私有启动元数据。

仍需完成整板 WebGPU 与原生视觉/交互比较、曲边边缘抗锯齿、完整案例与多 DPI/设备恢复、后台预处理峰值内存和帧耗时，以及安装包与完整发布验收。macOS/Linux 未开发、未验证。

## 历史复验入口（旧探针已移除）

```powershell
cargo +stable build -p pomelo-render --example curve_fill_probe --locked --offline
& C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/.bin/tsx.cmd scripts/check-curve-fill-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo target/debug/examples/curve_fill_probe.exe .cache/canvas-parity/curve-fill-parity E:/brd_cases/USBC_FPC.brd:utf-8 E:/brd_cases/camera_test_board.brd:utf-8 E:/brd_cases/AGILEX_I_SERIES.brd:windows-1252
cargo +stable test -p pomelo-render --all-features --locked --offline hardware_curve_ -- --ignored --nocapture
cargo +stable test --workspace --all-targets --all-features --locked --offline
cargo +stable clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
```
