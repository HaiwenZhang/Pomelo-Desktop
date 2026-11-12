# PCB 三维显示：solidcore 几何内核对比实验开发计划

日期：2026-11-11。分支：`feat/solidcore-3d-experiment`。源码基线：`f9a67b1`。
参考：`references/solidcore`（`6833c2eb54514f6f8fe51fa0b75b564eed4ae459`，已在 GitHub `origin/main` 上，B-rep 内核，Apache-2.0）、`references/solidcraft`（使用 solidcore 的参数化 CAD，MIT/Apache-2.0，另有品牌许可 `LICENSE-BRAND`）。

本文是实验计划，不改变 [PCB 三维显示开发设计](pcb-3d-rendering-development-design.md) §1.1 的主路线决策。实验结论出来后再决定主路线是否调整。

## 1. 结论：可以做

可以做，而且接入点很清楚。现有三维管线的所有实体网格都经过 `pomelo-core::geometry3d::solid::SolidMesh::extrude_*` 这一组函数（轮廓 → Clipper2 → Spade CDT → 顶/底面 + 侧壁）。solidcore 能在同一位置提供替代实现：

```
现有（Direct）：  轮廓环 ──Clipper2 归一/扣孔──► CDT 三角化 ──► 顶底面+侧壁 ──► SolidMesh
内核（Kernel）：  轮廓(线段/圆弧) ──► Region2 ──► 2D 精确布尔扣孔 ──► 拉伸成 B-rep 实体
                    ──► solidcore-tess 按弦高网格化 ──► Mesh ──适配──► SolidMesh
```

`SolidMesh` 之后的分页、实例、空间索引、拾取和 GPU 绘制都不用改，所以对比只比较"实体网格由谁生成"这一件事，变量干净。

需要提前知道的四个约束（详见 §6）：

1. **全局锁**：`solidcore-ops` 的会话是进程内唯一的 `Mutex<Kernel>`，所有 ops 调用串行。Pomelo 现在按铜层并行准备（最多 4 个 worker），走 ops 会被锁成单线程。实验要同时测"ops 串行"和"每线程一个 `Kernel` 直接调内核"两种方式。
2. **逐对象建 B-rep 的开销**：一块大板有几十万个 Trace/Pad。内核路线每个对象都要建拓扑、做容差检查、再网格化，预计比直接拉伸慢一到两个数量级。这正是实验要量化的东西，不是阻塞项。
3. **依赖来源**：`references/` 在 `.gitignore` 里，不能用 path 依赖提交。改用 git 依赖固定 rev（`https://github.com/tangency-labs/solidcore`，即 solidcraft 内 `solidcore/` 的只读镜像）。
4. **API 不稳定**：solidcore 标注为 0.0.x 实验版，ops 层还在合并重复实现（`docs/layers.md` §2）。固定 rev，适配代码集中在一个模块里。

## 2. 目标与非目标

**目标**

- 在 `pomelo-render` 里加一个可选的内核后端，用 cargo feature 和运行时开关切换，默认构建不受影响。
- 同一块板、同一个 `PreviewStackup`，两种后端各生成一份 `PreparedBoard3d`，量化比较准备时间、峰值内存、三角形数、几何误差、失败率和画面效果。
- 验证内核路线带来的、直接拉伸做不到的能力：圆弧精确保留、按视距重新细分（view-dependent LOD）、拓扑面/边身份、精确体积/面积。
- 产出对比报告，给主路线是否引入 solidcore 提供数据。

**非目标**

- 不改主路线，不删除也不重构现有 `SolidMesh` 实现。
- 不做 STEP 导出、元件 3D 模型、弯折板（这些是内核路线的潜在收益，在报告里评估，不在本次实现）。
- 不照搬 solidcraft 的 egui/wgpu 视口。Pomelo 用自己的原生 GPU 后端（D3D11/Metal/wgpu）。solidcraft 只作为调用方式和 LOD 策略的参考，不复制它的代码和品牌素材。
- 不修改 `references/` 下的任何代码。

## 3. 参考项目里用得上的部分

| 来源 | 内容 | 用途 |
| --- | --- | --- |
| `solidcore-ops::prism::extrude_planar_regions(plane, regions, start, end)` | 平面区域（外环+孔，线段/圆弧/三次贝塞尔）合并后拉伸成实体 | Trace/Pad/Zone/Via 的主入口 |
| `solidcore-ops::prism::planar_regions_boolean(a, b, op, tol)` | 平面区域精确 2D 布尔，结果边界都是原边界的片段，不重新拟合 | 钻孔扣除、同层铜合并 |
| `solidcore-ops::boolean::{boolean, union_all}` | 3D 实体布尔 | K3 压力实验：铜板减钻孔圆柱 |
| `solidcore-ops::body::make_cylinder` | 圆柱体 | Via 柱身、钻孔刀具 |
| `solidcore-record::profile::{Seg2, Loop2, Region2}` | `Seg2::Arc { center, radius, start, sweep }` | 与 `pomelo_core::model::Arc` 字段一一对应，可以直接映射 |
| `solidcore-tess::{plan, MeshTolerance, TessPlan, Mesh, PlanStats}` | 逐面在 (u,v) 上网格化，共享边采样保证水密；`PlanStats` 列出被拒绝的面/边 | 生成显示网格并检查完整性 |
| `solidcore-tess::simplify`、`preview` | 大网格的显示 LOD、有界粗网格 | LOD 实验 |
| solidcraft `crates/ui-egui/src/display_lod.rs` | 按屏幕误差逐级细化（每级弦高减半）、静止后再换、三角形预算 | LOD 策略参考 |
| solidcraft `crates/viewport-gpu`（`tess.wgsl`） | 在 compute shader 里求值 `TessPlan` | 远期参考，本次不做（§9） |
| solidcore `docs/layers.md` | 应用只依赖 ops/record/tess，不直接依赖内核 | 依赖边界；实验中直接调内核的变体要单独标明 |

## 4. 实验变体

为把"拉伸+网格化本身的成本"和"精确几何带来的成本与收益"分开，设四个变体。四个都输出 `SolidMesh`，共享下游。

| 变体 | 输入几何 | 扣孔 | 生成实体 | 回答的问题 |
| --- | --- | --- | --- | --- |
| **D**（基线） | Clipper2 多边形 | Clipper2 | 现有 CDT 拉伸 | — |
| **K1** 公平对照 | 与 D 相同的多边形环，全部转 `Seg2::Line` | 与 D 相同（Clipper2 先做） | `extrude_planar_regions` + tess | 同样的输入下，内核路线慢多少、网格多大 |
| **K2** 精确几何 | Trace 用中心线+圆端/圆弧段，圆焊盘、Via、钻孔用 `Seg2::Arc` | `planar_regions_boolean` | `extrude_planar_regions` + tess（按弦高） | 精确曲线的成本，换来的 LOD/精度收益有多少 |
| **K3** 3D 布尔 | 铜层整片 + 钻孔圆柱 | `boolean(Subtract)` | 3D 布尔 + tess | 通用 CAD 做法在 PCB 规模下是否可用（只跑小板） |

每个变体再分两种调用方式：

- **ops 串行**：按 `docs/layers.md` 规定只走 `solidcore-ops`，受全局锁限制。
- **per-thread kernel**（仅 K1/K2）：每个 worker 线程持有自己的 `solidcore::kernel::Kernel`，直接调 `solidcore::prism::extrude` 等内核函数。违反 solidcore 的分层规则，只为测量并行上限；报告里单独列出。

## 5. 架构与文件落点

### 5.1 依赖

```toml
# Cargo.toml [workspace.dependencies]
solidcore        = { git = "https://github.com/tangency-labs/solidcore", rev = "<固定 rev>" }
solidcore-ops    = { git = "...", rev = "<同上>" }
solidcore-record = { git = "...", rev = "<同上>" }
solidcore-tess   = { git = "...", rev = "<同上>" }

# crates/pomelo-render/Cargo.toml
[features]
kernel-3d = ["dep:solidcore", "dep:solidcore-ops", "dep:solidcore-record", "dep:solidcore-tess"]
```

- 本地开发可以在 `.cargo/config.toml`（不提交）里用 `[patch]` 指到 `references/solidcore/crates/*`，方便调试。
- 默认 feature 不开，`cargo build` 和 CI 主任务不受影响。新增一个 CI 任务或本地脚本跑 `--features kernel-3d`。
- solidcore 工作区是 edition 2021、MSRV 1.85，Pomelo 工具链 1.98.1，兼容。两边 `spade` 都是 2.15，确认能合并成一份。`solidcore-tess` 在非 wasm 下引入 `rayon`，需要确认它的线程池不会绕过 Pomelo 的全进程 CPU 许可（§6 风险 R5）。

### 5.2 代码落点

| 路径 | 内容 |
| --- | --- |
| `crates/pomelo-render/src/scene/solid/backend.rs`（新增） | `SolidBackend` 枚举：`Direct` / `Kernel { variant, threading }`；解析环境变量 `POMELO_3D_BACKEND`（如 `kernel-k2-ops`） |
| `crates/pomelo-render/src/scene/solid/kernel/mod.rs`（新增，`cfg(feature = "kernel-3d")`） | 对外只暴露与 `SolidMesh::extrude_*` 同签名的函数 |
| `.../kernel/profile.rs` | `Vec<Point>` 环 / Trace 中心线 / `model::Arc` / Pad 形状 / `DrillShape` → `Region2` |
| `.../kernel/extrude.rs` | 调 `extrude_planar_regions`、`planar_regions_boolean`；K3 的 3D 布尔 |
| `.../kernel/tessellate.rs` | `solidcore_tess::plan` + `TessPlan::evaluate`；`PlanStats` 有拒绝面或超差即返回错误 |
| `.../kernel/adapter.rs` | `solidcore_tess::Mesh` → `SolidMesh`：平移回局部坐标、按 B-rep 面法线把三角形归类为 `Top/Bottom/OuterWall/HoleWall`、重排 `surfaces` 索引范围 |
| `.../kernel/session.rs` | 失败回调（`set_failure_hook`）把失败对象写成 `.smb` 到临时目录，方便复现；per-thread kernel 的线程局部持有 |
| `crates/pomelo-render/src/scene/solid/{traces,pads,vias}.rs`、`scene/solid.rs`（修改） | 在调用 `SolidMesh::extrude_*` 的 4 处按 `SolidBackend` 分派 |
| `crates/pomelo-render/src/scene/board3d.rs`（修改） | `preview_with_workers` 接收 backend；ops 串行模式下 worker 数强制为 1，避免空等锁 |
| `crates/pomelo-render/examples/solid_profile.rs`（修改） | 增加 `--backend` 参数，输出多一组字段（§7） |
| `crates/pomelo-render/examples/solid_compare.rs`（新增） | 同板跑 D 和 Kx，输出几何差异：顶面面积差、体积差、Hausdorff 近似距离、三角形数 |
| `crates/pomelo-render/tests/kernel3d.rs`（新增，feature gate） | 单对象等价性测试 |
| `docs/pcb-3d-solidcore-experiment-report.md`（实验结束时新增） | 对比报告 |

错误映射：`solidcore_ops::Error` / `TessError` / 拒绝面 → `SolidError::Unsupported("KERNEL_...")` 或 `SolidError::Invalid(...)`，走现有 `warnings` 通道，界面上显示为准备警告。取消：`solidcore_ops::cancel::within(token, ..)` 包住每个调用，`CancelToken` 由 Pomelo 的 `CancellationToken` 桥接。

## 6. 风险与应对

| # | 风险 | 影响 | 应对 |
| --- | --- | --- | --- |
| R1 | ops 全局锁 | 并行准备退化成单线程 | 同时测 per-thread kernel；报告里把两者分开 |
| R2 | 逐对象 B-rep 太慢 | 大板准备时间不可接受 | 只对原型做一次（圆焊盘、Via 已经是实例复用）；Trace 按 tile 合并成区域再拉伸，减少调用次数；结果本身就是实验结论 |
| R3 | 网格化漏面（`PlanStats` 拒绝面） | 实体不完整却显示为成功 | 拒绝面一律视为该对象失败；统计失败率；`.smb` 落盘复现，必要时向 solidcore 提 issue |
| R4 | 相邻铜片接触时 `extrude_planar_regions` 报 "pieces touch" | 同网络重叠 Trace/Pad 构造失败 | 先 `planar_regions_boolean(Union)` 合并；或退回逐对象分别拉伸 |
| R5 | `solidcore-tess` 的 rayon 线程池与 Pomelo CPU 许可冲突 | UI 掉帧 | 在受控的 rayon 线程池（`ThreadPoolBuilder` 限定线程数）内调用；实验中记录导航帧时间 |
| R6 | 内核坐标与容差 | 大坐标下精度、1e-7 mm 拉伸容差 | 沿用现有做法：先平移到板中心局部坐标再交给内核 |
| R7 | API 变动 | 升级 rev 时编译失败 | 固定 rev；适配代码集中在 `kernel/` 模块；实验期间不跟上游 |
| R8 | 编译时间与二进制体积 | 开发体验 | 只在 feature 下编译；P0 记录增量 |
| R9 | 许可证 | solidcore Apache-2.0，与 Pomelo 相同；solidcraft 有品牌许可 | 只依赖 solidcore；solidcraft 只读参考，不复制代码或素材；如果最终并入，补充 `NOTICE` 和第三方许可清单 |

## 7. 度量指标

全部输出 JSON（扩展 `solid_profile` 的现有格式），每个组合跑 3 次取中位数。

| 类别 | 指标 |
| --- | --- |
| 时间 | 准备总时间、拉伸/布尔时间、网格化时间、适配时间；冷启动 vs 热缓存 |
| 资源 | 峰值 RSS、`PreparedBoard3d::allocation_bytes`、GPU 上传字节 |
| 网格 | 顶点数、三角形数、实例数；相同弦高（0.005 mm）下的比值 |
| 正确性 | 失败对象数和类型、拒绝面数、超差面数、非水密网格数 |
| 几何误差 | 与解析真值比：圆形 Via/焊盘的最大弦高偏差、顶面面积相对误差、体积相对误差（内核用 `Body::volume`，D 用网格体积） |
| 帧率 | 同一组相机路径下的整帧时间 p50/p95（复用现有 frame-profiling）；预期两者接近，因为下游相同 |
| LOD | K2 下弦高 0.05 / 0.01 / 0.005 / 0.001 mm 各自的三角形数和准备时间，与 D 现有 8/16/32/64 段圆 LOD 比较 |

### 7.1 测试板与 D 基线

测试板放在 `/Users/haiwen/Downloads`，不入库。基线用 `cargo run --release -p pomelo-render --example solid_profile <board>` 测得（2026-11-11，Apple M4 10 核，16 GB，默认 `PreviewStackup`，默认并行度，单次运行）。`sources` 是三维可拾取源对象数，近似等于需要建实体的对象数。

| 档 | 文件 | 大小 | 导入 | D 准备 | sources | 实例 | 三角形 | 准备结果内存 | 进程峰值 RSS |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 小 | `DemoCase_LPDDR4.brd` | 7 MB | 50 ms | 65 ms | 19,878 | 20,183 | 0.40 M | 18 MB | 93 MB |
| 中 | `GTX1660TI_142-1G161-1000-A00_26.1.brd` | 76 MB | 305 ms | 311 ms | 86,863 | 79,612 | 1.94 M | 83 MB | 367 MB |
| 大 | `ARM_Test.mcm` | 488 MB | 3.7 s | 22.0 s | 3,891,433 | 3,808,959 | 40.7 M | 2.07 GB | 4.46 GB |

由此得到两点：

- **没有千级对象的小板**。K3（3D 布尔）和 P1 之后的首轮整板验证需要更小的输入，所以 `solid_profile` 加 `--layer <名称>` 和 `--window x0,y0,x1,y1`（mm）参数，从 DemoCase 裁出单层或局部区域（几百到几千个对象）。
- **大板对内核路线是压力上限**。按每个对象 1 ms 估算，ARM_Test 串行要 1 小时以上。每次运行设 30 分钟上限，超时记录已完成的对象数和推算的总时间，不算作崩溃。实例复用的原型（圆焊盘、Via）只建一次，实际调用次数会少于 sources，需要单独统计"内核调用次数"。

## 8. 阶段计划

估时按一个人全职计算，供排期参考。

### P0 依赖打通（1–2 天）

- 加 git 依赖和 `kernel-3d` feature，macOS 和 Windows 都能 `cargo build -p pomelo-render --features kernel-3d`。
- 冒烟测试：`make_box` → `solidcore_tess::plan` → `Mesh`，三角形数和包围盒正确。
- 记录：`cargo tree -d` 的重复依赖、增量编译时间、release 二进制体积变化。

**验收**：feature 关闭时 `cargo test --workspace` 与 main 结果一致；feature 打开时冒烟测试通过。

### P1 单对象适配与等价测试（3–4 天）

- 实现 `profile.rs`、`extrude.rs`、`tessellate.rs`、`adapter.rs`，覆盖 K1 和 K2。
- 测试样本：直线 Trace、圆弧 Trace、闭合 Trace、矩形/圆/椭圆/自定义多边形焊盘、带钻孔焊盘、实心 Via、带孔岛的 Zone、背钻跨度。
- 每个样本断言：网格水密；`SurfaceKind` 分类正确；顶面面积与 D 的差在弦高误差界内；体积与 `Body::volume` 一致。

**验收**：`tests/kernel3d.rs` 全部通过；拒绝面会被报告成错误，不会悄悄漏面。

### P2 整板接入（3–4 天）

- `SolidBackend` 分派接入 `traces.rs`、`pads.rs`、`vias.rs`、`solid.rs`。
- 实现 ops 串行和 per-thread kernel 两种调用；接好取消和失败落盘。
- 用 `POMELO_3D_BACKEND` 在应用里切换，确认显示、拾取、显示过滤和颜色在内核后端下都能用。
- 实现 K3（仅小板，加对象数上限，超出直接拒绝）。

**验收**：小板在 K1/K2 下能完整显示，拾取结果与 D 一致；取消在 1 秒内生效。

### P3 基准与几何对比（3 天）

- `solid_profile --backend`、新增 `solid_compare`。
- 测试板见 §7.1：DemoCase 裁剪区域（微型）、DemoCase（小）、GTX1660TI（中）、ARM_Test（大，30 分钟上限）。
- `solid_profile` 增加 `--layer`、`--window`、`--timeout` 参数，并输出内核调用次数。
- 每块板 × {D, K1-ops, K1-thread, K2-ops, K2-thread} 跑完；K3 只跑 DemoCase 裁剪区域。汇总成表。

**验收**：所有组合都有数据，失败也有明确的失败原因。

### P4 画面与 LOD 实验（2–3 天）

- 两种后端的同视角截图对比（顶视、等轴测、近距离 Via、剖切），沿用 `tests/unit/backend/*_pixels.rs` 的像素测试思路，只做差异图，不加入 CI。
- K2 LOD 原型：参考 solidcraft `display_lod.rs`，相机静止后按屏幕误差对可见页重新调用 tess 加细，后台生成后替换。只做原型，量化"近看圆弧更圆"的收益和代价。
- 线框模式下比较边线：tess 的 B-rep 边折线 vs 现有 `edges.rs` 特征边提取。

**验收**：截图集和 LOD 数据写入报告。

### P5 报告与决策（1–2 天）

- 写 `docs/pcb-3d-solidcore-experiment-report.md`：方法、数据表、截图、失败案例、结论。
- 结论按 §10 的判定规则给出建议，并更新 [开发设计](pcb-3d-rendering-development-design.md) §1.1 的选型表。

**合计**：约 13–18 个工作日。

## 9. 不在本次范围、但报告里要评估的方向

- **GPU 求值 TessPlan**：solidcraft 在 wgpu compute shader 里求值，CPU 只做规划。Pomelo 的 D3D11/Metal 后端需要各自实现一份，成本高，先看 P3 里网格化时间占比再决定。
- **混合路线**（开发设计 §1.1 第三行）：常规铜皮继续直接拉伸，只有元件实体、异形钻孔、弯折区域、STEP 导出走内核。很可能是最终推荐方案，报告里给出这种组合的估算。
- **STEP 导出**：`solidcore-step` 已有，内核路线下顺带获得。
- **元件 3D 模型**：读取 STEP 元件模型并放置，需要内核。

## 10. 判定规则

| 结果 | 建议 |
| --- | --- |
| K1/K2 在中板上准备时间 ≤ D 的 3 倍，失败率 < 0.1%，内存 ≤ D 的 1.5 倍 | 考虑整体切换到内核路线，进入正式设计 |
| 中板可用但大板不可用，或慢 3–20 倍 | 采用混合路线：铜皮保持直接拉伸，内核只处理复杂实体和导出 |
| 小板以上就不可用，或失败率高 | 维持现状；保留 feature 代码作为将来 STEP 导出/元件模型的起点，或者删除 |

帧率不作为判定依据，因为下游相同；如果帧率出现明显差异，说明网格规模不同，需要回头查三角形数。

## 11. 开始前确认的事项

- [x] solidcore 固定 rev：`6833c2eb54514f6f8fe51fa0b75b564eed4ae459`（已确认在 GitHub `origin/main` 上）。
- [x] 测试板：见 §7.1。
- [ ] 报告里能否放这三块板的截图（GTX1660TI 和 ARM_Test 看起来是第三方设计）。
- [ ] per-thread kernel 变体绕过了 solidcore 的分层规则，确认只用于测量、不进入正式实现。
