# PCB 网格准备与帧率约束

二维 Zone 使用 Earcut 与独立轮廓覆盖；三维实体使用纯 Rust CDT（Spade）。两者均采用有上限的 CPU 并行准备。帧率与输入响应优先于后台吞吐量。本文只说明实现、资源约束和可公开复现的验证方法。

## 几何准备

外轮廓和孔轮廓先经过 Clipper2 归一化，再作为 CDT 约束边。由外部面传播奇偶状态，排除孔内及凹多边形外侧的三角形，随后生成顶底面与侧壁。保留面积、绕向、索引范围、封闭性和资源检查。

曲线容差保持 0.005 mm。三维 Via 按实心显示，不从其焊盘、走线和铜区扣除中心孔；Pin 孔保持实际轮廓。二维几何不受三维三角化算法影响。

## CPU 并发

令 `C = std::thread::available_parallelism()`，查询失败取 1。为界面、渲染与系统工作留出余量：

```text
R = 1                  (C <= 2)
R = 2                  (3 <= C <= 8)
R = ceil(C / 4)        (C > 8)
W = max(1, min(4, C - R))
```

小规模任务串行处理。较大任务按铜层分配，重任务先派发，并按原层顺序合并。各任务使用完整层高表，Via 实体只准备一次。合并时重新映射源 ID、顶点与实例所有者、网格、分片和空间索引。

全进程的二维 Zone 与三维准备共享 CPU 许可，避免多个文档各自占用一组满负荷线程。`preview_with_workers(..., 1)` 可显式要求串行；请求更高并发仍受默认 CPU 上限限制。当前未接入实时帧耗时反馈，预留 CPU 余量不等于在所有设备上保证固定帧率。

## 二维 Zone 准备

二维保留原有外轮廓和独立孔剖分，不把孔合并为单个挖孔网格。孔的并集扣除、按孔显示过滤、源坐标、Morton 排序及分块边界不变。

`CopperMesh::prepare` 校验并拥有轮廓元数据和预留输出，返回不能直接渲染的 `PendingCopperMesh`。`finish` 使用原 Earcut 和凸轮廓快速路径，生成原顺序的索引。导入器按有界批次准备多个 Zone，将连续任务按点数平衡到长期复用的线程池；批次完成后才继续发布进度。失败或取消不返回部分场景。

CPU 上限使用上面的 `W`。设当前批次最大保守临时空间为 `S`，并行临时额度为 `B`：

```text
B = min(调用方剩余额度, 当前可用物理内存 / 4, 物理内存总量 / 8)
W_effective <= min(W, 独立任务数量, max(1, floor(B / max(1, S))))
```

小任务直接串行。物理内存信息不可用时使用有界回退；单任务超过物理内存软额度时独占准备许可。场景的长期内存仍由导入器预算管理，该公式控制并行临时存储。全进程同时限制 CPU 许可和临时空间准入，避免多个文档分别用满资源。

Allegro 的 `SceneBuilder::with_mesh_workers(1)` 可显式要求串行。并行延后完成网格可能增加同时驻留的元数据；如触发资源限额，则释放本轮私有结果并以原串行流程重试。其他格式在通用 Zone 完成边界使用相同线程和内存策略。

## 内存与取消

按层工作量分配互不重叠的内存额度，总和不超过调用方预算的 7/8，剩余额度用于合并元数据和空间索引。合并前检查临时驻留。若某层额度不足，先释放部分结果，再用原始完整预算串行重试。应用共享内存预留随准备结果的生命周期释放。

取消信号覆盖许可等待、任务间循环与几何处理检查点。Spade 批量插入无法在调用中途强制中断，在调用前后检查取消。取消或失败不发布部分板数据。

## 圆形轮廓与屏幕细分

圆形过孔和可复用的实心圆焊盘保留至少 128 段的准备原型；一般圆轮廓保留至少 32 段，仍检查弦高容差与资源预算。圆柱侧壁使用连续径向法线，顶底面保持平面光照，显式多边形保留折角。法线处理使用原有顶点和实例布局，不为圆柱侧壁复制顶点。

可复用的实心圆原型预备 8、16、32、64 段的索引范围，并保留完整细分索引。当前正交相机按屏幕尺寸选择弦高误差不超过半个物理像素的级别；近处使用更细的级别，远处减少微小三角形。各级别共享顶点与 GPU 缓冲，导航时只改变绘制范围，不重新准备或上传几何。

带孔轮廓与显式自定义多边形不使用实心圆索引替代。拾取始终使用完整准备网格；GPU 显示细分仅引入有界的屏幕误差。所有索引范围在准备和原生绘制入口校验。

## 渲染与拾取

准备结果保持分页、可见性裁剪、渐进上传和缓存复用。颜色、透明度、选择和相机变化应复用几何。CPU 准备时间、GPU 执行时间、首次可交互时间与窗口整帧时间分别评估。

拾取使用保守的浮点包围盒，兼容 f64 源几何与 f32 GPU 顶点的舍入差异。同深度命中按源对象身份稳定决胜，避免并行合并顺序影响重叠对象的选择。

## 公开验证

回归使用仓库内合成几何和公开夹具，覆盖多孔凹多边形、孔与外边界接触、重叠孔、闭合性、法线、CDT 冲突拒绝、取消、并发上限、层合并、来源映射、拾取和低预算回退。

```sh
cargo test -p pomelo-core --test solid_mesh --test copper_mesh --locked
cargo test -p pomelo-import --lib formats::zone_mesh --locked
cargo test -p pomelo-render --test board3d --test solid_prepare --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all --check
```

GPU 像素测试使用合成场景检查深度、孔洞、透明度、裁剪和缓存。窗口验收独立检查导航、选择、输入延迟、持续帧时间和多文档资源释放。不同操作系统与 GPU 需要各自验证；离屏 GPU 时间不能作为窗口 FPS。

## 代码入口

- `crates/pomelo-core/src/task/preparation.rs`：共享 CPU 与临时空间准入。
- `crates/pomelo-core/src/geometry/copper.rs`：二维轮廓准备和 Earcut 完成边界。
- `crates/pomelo-import/src/formats/zone_mesh.rs`：有界批次、机器资源策略和线程池。
- `crates/pomelo-core/src/geometry3d/solid.rs`：归一化、挤出和几何预算。
- `crates/pomelo-core/src/geometry3d/solid/cdt.rs`：约束三角化与面分类。
- `crates/pomelo-render/src/scene/board3d/parallel.rs`：CPU 许可、内存分配和确定性合并。
- `crates/pomelo-render/src/scene/preparation_memory.rs`：应用共享准备额度。
- `crates/pomelo-render/src/scene/board3d/spatial.rs`：空间索引与拾取遍历。
- `crates/pomelo-render/src/backend/solid_pipeline.rs`：原生分页上传与绘制。

可用并行度的含义和平台限制见 [Rust 标准库文档](https://doc.rust-lang.org/std/thread/fn.available_parallelism.html)。
