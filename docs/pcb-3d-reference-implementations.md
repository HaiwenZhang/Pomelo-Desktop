# PCB 全层三维几何：参考实现调查

目标是按物理层高渲染每层走线、Zone、焊盘，以及有实际孔腔和层跨度的镀铜过孔。本文区分源码已有能力与建议；没有构建参考仓库、运行样例或验证真实 PCB 的速度和鲁棒性。

## SolidCraft：优先参考平面区域、挤出和网格链路

本次核对的本地提交为 `9c51f9d`。源码证据说明这些入口已存在；本次没有运行其测试，也没有验证 PCB 整板规模。

| 已有实现 | 源码证据 | 对 Pomelo 的用途 |
| --- | --- | --- |
| 解析线段、圆弧及带孔区域 | [`Seg2`](/Users/haiwen/gitrepo/solidcraft/crates/geom/src/profile.rs:14)、[`Region2`](/Users/haiwen/gitrepo/solidcraft/crates/geom/src/profile.rs:458) | 表达走线轮廓、逐层焊盘及 Zone 的最终材料区域 |
| 平面区域布尔 | [`planar_regions_boolean`](/Users/haiwen/gitrepo/solidcraft/crates/kernel/src/prism.rs:32) | 合并重叠材料、扣除孔及避让区域；不代替导入器或 PCB 连接规则 |
| 多区域、带孔挤出 | [`extrude_planar_regions`](/Users/haiwen/gitrepo/solidcraft/crates/kernel/src/prism.rs:142)、[`extrude`](/Users/haiwen/gitrepo/solidcraft/crates/kernel/src/build.rs:198) | 根据铜层的起止高度生成有上下表面和侧壁的几何；圆环区域可用于中空孔段 |
| 按高度截面处理棱柱布尔 | [`prism.rs` 的设计说明](/Users/haiwen/gitrepo/solidcraft/crates/kernel/src/prism.rs:1)、[部分深度切割回归用例](/Users/haiwen/gitrepo/solidcraft/crates/kernel/src/prism.rs:2360) | 借鉴盲埋孔、背钻等沿 Z 分段的构造策略；不是现成 PCB 孔语义 |
| 带法线和面身份的网格 | [`Mesh`](/Users/haiwen/gitrepo/solidcraft/crates/geom/src/mesh.rs:55)、[`Body::tessellate`](/Users/haiwen/gitrepo/solidcraft/crates/kernel/src/body.rs:970) | 转换为 Pomelo 的三维批次；CAD 面编号需另映射到 PCB 对象、层和网络 |
| 共享边界的离散 | [`tess/mod.rs`](/Users/haiwen/gitrepo/solidcraft/crates/kernel/src/tess/mod.rs:1) | 参考相邻面共用边界采样，降低孔壁与上下表面裂缝风险 |
| 相机和射线命中 | [`Camera`](/Users/haiwen/gitrepo/solidcraft/crates/render/src/camera.rs:221)、[`Mesh::raycast`](/Users/haiwen/gitrepo/solidcraft/crates/geom/src/mesh.rs:561) | 参考轨道相机、正交/透视、拾取；该 raycast 逐三角形遍历，整板还需空间加速 |
| 深度、透明、高亮和 X-ray | [`gpu.rs` 管线](/Users/haiwen/gitrepo/solidcraft/crates/ui-egui/src/gpu.rs:633) | 借鉴显示组织；不能直接替换 Pomelo 的 Metal/D3D11/wgpu 三端适配 |

接入时应注意：

- `planar_regions_boolean` 的直线和圆弧保持解析表示；被切割的自由曲线可能按 `PLANAR_FIT_TOL = 1e-3 mm` 近似为线段和圆弧，不能统一宣称全部输入保持精确。[容差与预算](/Users/haiwen/gitrepo/solidcraft/crates/kernel/src/prism.rs:20)
- 当前平面布尔入口设置了 8,192 边界段、131,072 事件等上限；挤出区域归并另有工作预算。这些是失败边界，不是整板性能指标。必须针对大量过孔、窄铜桥、相切边界和大 Zone 先验证，不能仅提高常量绕过限制。[布尔入口](/Users/haiwen/gitrepo/solidcraft/crates/kernel/src/prism.rs:32)、[挤出归并](/Users/haiwen/gitrepo/solidcraft/crates/kernel/src/prism.rs:142)
- kernel 使用 truck、spade，并通过仓库级 `[patch.crates-io]` 使用本地修补的依赖。把 kernel crate 作为依赖时必须重新核对和接入这些补丁；只添加一个 path 依赖不能假定得到相同结果。[kernel 依赖](/Users/haiwen/gitrepo/solidcraft/crates/kernel/Cargo.toml)、[workspace 配置](/Users/haiwen/gitrepo/solidcraft/Cargo.toml)、[补丁说明](/Users/haiwen/gitrepo/solidcraft/vendor/README.md)
- 仓库包声明 `MIT OR Apache-2.0`，vendored 第三方项有独立署名记录；若移植代码，应随具体文件维护来源和适用许可。[包声明](/Users/haiwen/gitrepo/solidcraft/Cargo.toml)、[第三方记录](/Users/haiwen/gitrepo/solidcraft/ATTRIBUTION.md)

工程判断：SolidCraft 已有可评估的几何内核能力，不需要从零重写区域布尔和 B-rep 挤出。但这不意味着应让整板每段走线都先成为一个 B-rep，再全局合并。建议先比较局部几何构造与直接网格构造，最终统一输出 Pomelo 自有网格和对象映射。

## USD BRep / SMLib

### 源码已经提供的几何能力

| PCB 需求 | 已有实现与证据 | 应用边界 |
|---|---|---|
| 带孔 Zone、异形焊盘的平面区域 | `SmApiCreatePlanarFaces` 接受平面曲线集合；内核构造支持任意层数嵌套，材料与空洞交替。[API](/Users/haiwen/gitrepo/usd-brep/source/SM_API/inc/SmApiTrimmedSurfaces.h:58)、[嵌套与输入条件](/Users/haiwen/gitrepo/usd-brep/source/SMLib/src/SmBrep.cpp:3702) | 要求闭合、共面、不相交的有效边界；自交轮廓不能直接送入并期待自动修复。返回的是 sheet BRep，尚无厚度。 |
| 合并走线、焊盘与 Zone；扣除避让区 | `SmApiBoolean2d` 提供并、交、差、异或、合并。[接口](/Users/haiwen/gitrepo/usd-brep/source/SM_API/inc/SmApiBrep.h:107) | 只证明存在区域运算入口；没有验证微小铜隙、热连接及大规模铺铜输入。空输入有特殊成功返回语义，需要适配层处理。 |
| 每层铜厚及完整侧壁 | `SmApiCreateLinearSweep(curves, dir, distance, capEnds, result)` 将轮廓挤出；原曲线先复制；内核用嵌套区域构造器创建两个端盖。[公开 API](/Users/haiwen/gitrepo/usd-brep/source/SM_API/inc/SmApiBrep.h:210)、[复制与调用](/Users/haiwen/gitrepo/usd-brep/source/SM_API/src/SmApiBrep.cpp:1218)、[端盖](/Users/haiwen/gitrepo/usd-brep/source/SMLib/src/SmPrimitiveCreation.cpp:1399) | 带孔轮廓挤出有源码路径支撑；不等同于任意 PCB 输入都能稳定得到闭合实体。应用端仍须校验拓扑和孔腔。 |
| 中空铜筒、钻孔、背钻 | `SmApiCreateCylinder` 创建指定起点、半径、高度的 BRep；`SmApiBooleanDifference` 提供实体差。[圆柱](/Users/haiwen/gitrepo/usd-brep/source/SM_API/inc/SmApiPrimitives.h:92)、[实体差](/Users/haiwen/gitrepo/usd-brep/source/SM_API/inc/SmApiBrep.h:67) | “外圆柱减内圆柱”或“环形轮廓挤出”是建议组合，并非现成 PCB via API。盲埋孔、背钻深度、逐层焊盘尺寸和连接规则由 PCB 模型决定。 |
| 精确实体转换为显示网格 | `SmApiTessellate` 输出 `SmPolyBrep`，支持弦高、角度、边长等控制，以及逐面失败诊断。[接口](/Users/haiwen/gitrepo/usd-brep/source/SM_API/inc/SmApiBrep.h:121)、[参数](/Users/haiwen/gitrepo/usd-brep/source/SM_API/inc/SmApiTessellationParams.h:26) | 这是几何表面三角化；不代表电磁仿真的体网格生成能力，也不提供现成 GPUI 渲染器。 |

### 网格数组、法线与拾取身份

- Python `PolyBrep.to_mesh_arrays` / `to_numpy_arrays` 已输出顶点、法线、平面多边形索引流和逐面法线；支持 face-varying 角点法线，适合区分铜层平面与孔壁的硬边。[数组结构与说明](/Users/haiwen/gitrepo/usd-brep/source/SmPyLib/src/SmPyPolyBrep.cpp:339)
- 提取逻辑直接遍历 `SmPolyVertex` / `SmPolyFace`，不需要经过 USD 文件。可以参考该 C++ 遍历方式，生成 Pomelo 使用的顶点、索引与法线数组。[提取实现](/Users/haiwen/gitrepo/usd-brep/source/SmPyLib/src/SmPyPolyBrep.cpp:88)
- `SmPolyFace::GetOriginalFace()` 可返回原 BRep 面指针；源码明确指出它可作为标签，back-pointer 无效时不能当活对象访问。[面来源](/Users/haiwen/gitrepo/usd-brep/source/SMLib/inc/SmPoly.h:1157)
- 上述公开数组只有 `points / normals / faces / face_normals`，**没有自动输出稳定的 PCB 对象 ID 或三角形到原面 ID 表**。[实际数组成员](/Users/haiwen/gitrepo/usd-brep/source/SmPyLib/src/SmPyPolyBrep.cpp:54) 建议在提取时保存局部面编号，并由 Pomelo 维护 `对象 / 网络 / 层 / 原面 → 网格范围` 映射；不能将指针地址持久化为对象 ID。

### 接入 Pomelo 的具体约束

1. **这是 C 风格 C++ API，而非可直接由 Rust `extern "C"` 调用的 C ABI。** 函数使用 `SmTArray<T>`、引用、C++ 对象以及同名重载。[数组与引用示例](/Users/haiwen/gitrepo/usd-brep/source/SM_API/inc/SmApiBrep.h:210)、[重载示例](/Users/haiwen/gitrepo/usd-brep/source/SM_API/inc/SmApiPrimitives.h:92) 建议另建薄 C ABI 或 `cxx` 桥，隐藏内核类型与释放规则。
2. 原生调用者需要链接 `SM_API` 与 `SMLib`，并配置运行时依赖；仓库提供独立 C++17 样例。[原生接入说明](/Users/haiwen/gitrepo/usd-brep/Examples/SM_API/README.md:39) 建议先限定到这两层，避免将 USD schema、Python 和 Hydra 观看流程当作 PCB 查看器的前置条件；实际最小依赖和打包体积仍需构建确认。
3. **必须按入口管理所有权和失败。** 实体差成功修改并返回 A、删除 B；失败则 A/B 仍分配着但可能已经改变，结果为空。[布尔契约](/Users/haiwen/gitrepo/usd-brep/source/SM_API/inc/SmApiBrep.h:42) 平面建面成功消费曲线，而线性挤出保留输入曲线；封装不能统一猜测“输入永远只借用”。[建面契约](/Users/haiwen/gitrepo/usd-brep/source/SM_API/inc/SmApiTrimmedSurfaces.h:58)
4. **不能让 Rust 线程池并发执行任意 `SmApi*`。** 默认 context 是进程全局且有未加锁状态；仓库规定每进程一个并发作业，发行配置中的操作本身也为单线程。[并发规则](/Users/haiwen/gitrepo/usd-brep/.agents/operations/concurrency.md:10) 建议先使用单一几何工作队列；要增加建模并发时评估独立进程。
5. 默认三角化严格拒绝缺面结果；只有显式允许 partial 且提供报告时才接受局部输出。[失败处理](/Users/haiwen/gitrepo/usd-brep/source/SM_API/src/SmApiBrep.cpp:694) PCB 主视图建议保留严格模式，按对象报告错误；缺铜面不得默默当成功。
6. macOS 文档写明 Apple Silicon 支持，默认可构建内核；OpenUSD/Python 路线另有依赖，公开 Packman 资源也存在缺项。[平台说明](/Users/haiwen/gitrepo/usd-brep/README.md:76) 这不是已在 Pomelo 完成的编译兼容性验证。
7. 仓库 README 声明 USD BRep 使用 Apache-2.0，若干构建工具使用 MIT，并指向第三方声明。[许可证说明](/Users/haiwen/gitrepo/usd-brep/README.md:106) 本次仅记录项目声明，没有审核依赖清单或作法律适用判断。

### 不宜从现有接口直接推断的结论

- 当前接口覆盖建面、区域布尔、挤出、圆柱与三角化，因此它是可评估的实体几何后端；**没有证据表明整板数十万对象逐个 BRep 后仍满足交互加载、显存和增量更新时间**。也没有进行与专用 2.5D 网格方案的对比测试。
- 内核建面函数的头部注释与实现对容差参数存在历史差异：注释提示限制，但参数标为 `NotUsed`，实际读取 BRep 自身容差。[源码](/Users/haiwen/gitrepo/usd-brep/source/SMLib/src/SmBrep.cpp:3706) 不能将 wrapper 中的常数当成可用的 PCB 建模精度保证。
- 已知问题包括 healer 报成功却修复失败、周期面修复异常、倒角附近三角形反向，以及圆形边界面积/体积测量误差。[已知问题](/Users/haiwen/gitrepo/usd-brep/KNOWN_ISSUES.md:16) 这些不证明正常 PCB 铜筒一定失败，也不能据此声称内核已经通过 PCB 专项验收。

### 针对全层导体的建议验证范围

先建立小型真实尺寸样板：带孔和孤岛的内层 Zone、线弧走线、逐层不同形状的焊盘、同位置不同跨度过孔、盲埋孔、背钻、热连接与微小铜隙。比较轮廓挤出与实体布尔两种方式的孔腔、厚度、法线、拾取身份、失败诊断、时间及内存。通过后再决定是否让该内核承担全部常规几何，或保留为复杂操作的独立后端。

## 对 Pomelo 的实施建议

优先验证 SolidCraft 的平面材料区域与挤出链路，理由是它提供与 PCB 分层构造相近的 Rust 实现；usd-brep 作为同一组合成输入下的另一候选，评估实体构造、复杂切割和三角化。此排序是接入成本的工程判断，不是正确性或性能排名，也不预先要求长期维护两个运行时后端。

无论采用哪条路线，对外都应输出 Pomelo 自有的不可变网格、法线、边界及对象映射，几何内核类型留在准备模块内部。源对象、物理叠层和孔段语义仍由 `pomelo-core` / `pomelo-import` 管理，最终渲染仍接入 GPUI 的 Metal、D3D11、wgpu 独立 pass。

首个验证闭环应包括全部铜层的走线、Zone、逐层焊盘和中空过孔；默认叠合显示，介质可隐藏，支持逐层显隐、网络高亮和选择。不要以元件装配、仅外层铜面或层展开截图代替这个闭环。按对象和局部材料区域准备几何，归并时保留源身份，不能为了减少实体数量而合并不同网络或丢失选择对象。后续是否采用整层 B-rep、直接挤出网格或混合方案，以专项正确性、准备时间、内存及取消行为的验证结果决定。
