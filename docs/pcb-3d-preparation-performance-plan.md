# PCB 三维准备与帧率约束

三维实体使用纯 Rust CDT（Spade）和有上限的 CPU 并行准备。帧率与输入响应优先于后台吞吐量。本文只说明实现、资源约束和可公开复现的验证方法。

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

全进程的三维准备共享 CPU 许可，避免多个文档各自占用一组满负荷线程。`preview_with_workers(..., 1)` 可显式要求串行；请求更高并发仍受默认 CPU 上限限制。当前未接入实时帧耗时反馈，预留 CPU 余量不等于在所有设备上保证固定帧率。

## 内存与取消

按层工作量分配互不重叠的内存额度，总和不超过调用方预算的 7/8，剩余额度用于合并元数据和空间索引。合并前检查临时驻留。若某层额度不足，先释放部分结果，再用原始完整预算串行重试。应用共享内存预留随准备结果的生命周期释放。

取消信号覆盖许可等待、任务间循环与几何处理检查点。Spade 批量插入无法在调用中途强制中断，在调用前后检查取消。取消或失败不发布部分板数据。

## 渲染与拾取

准备结果保持分页、可见性裁剪、渐进上传和缓存复用。颜色、透明度、选择和相机变化应复用几何。CPU 准备时间、GPU 执行时间、首次可交互时间与窗口整帧时间分别评估。

拾取使用保守的浮点包围盒，兼容 f64 源几何与 f32 GPU 顶点的舍入差异。同深度命中按源对象身份稳定决胜，避免并行合并顺序影响重叠对象的选择。

## 公开验证

回归使用仓库内合成几何和公开夹具，覆盖多孔凹多边形、孔与外边界接触、重叠孔、闭合性、法线、CDT 冲突拒绝、取消、并发上限、层合并、来源映射、拾取和低预算回退。

```sh
cargo test -p pomelo-core --test solid_mesh --locked
cargo test -p pomelo-render --test board3d --test solid_prepare --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all --check
```

GPU 像素测试使用合成场景检查深度、孔洞、透明度、裁剪和缓存。窗口验收独立检查导航、选择、输入延迟、持续帧时间和多文档资源释放。不同操作系统与 GPU 需要各自验证；离屏 GPU 时间不能作为窗口 FPS。

## 代码入口

- `crates/pomelo-core/src/geometry3d/solid.rs`：归一化、挤出和几何预算。
- `crates/pomelo-core/src/geometry3d/solid/cdt.rs`：约束三角化与面分类。
- `crates/pomelo-render/src/scene/board3d/parallel.rs`：CPU 许可、内存分配和确定性合并。
- `crates/pomelo-render/src/scene/preparation_memory.rs`：应用共享准备额度。
- `crates/pomelo-render/src/scene/board3d/spatial.rs`：空间索引与拾取遍历。
- `crates/pomelo-render/src/backend/solid_pipeline.rs`：原生分页上传与绘制。

可用并行度的含义和平台限制见 [Rust 标准库文档](https://doc.rust-lang.org/std/thread/fn.available_parallelism.html)。
