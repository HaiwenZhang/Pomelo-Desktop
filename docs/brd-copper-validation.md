# BRD 铜皮网格与板框验证

记录日期：2026-10-02。当前开发平台为 Windows。本记录证明 CPU 铜皮语义与网格查询，尚不证明 Native 整板导入或真实铜皮的 GPU 呈现；`AllegroImporter::import` 仍返回未实现诊断。

## 实现与数据契约

- `pomelo-core::copper::CopperMesh` 保留 f64 源坐标、源顺序的顶点及 `ring_offsets`，分别三角化外环和每个孔洞。`outer_count` 区分外环与孔洞索引；GPU 必须执行“外环覆盖减去孔洞并集”，重叠孔洞不能恢复填充。
- 按 Web 相同规则对孔洞稳定 Morton 排序，每 64 个孔洞形成一个带包围盒的绘制范围。重排只影响索引提交顺序，不改变源顶点和源环身份。
- 保留解析圆弧路径；环包围盒包含解析曲线的极值，不依赖离散点包围盒或线宽。新增 `Segment::bounds()` 和 `centreline_bounds()`，分别用于带圆帽的线段/圆弧及铜皮边界。
- 严格凸轮廓使用与 Web 相同的快速路径和裁剪顺序；退化、非凸或不确定轮廓使用锁定的 `earcut 0.4.11`。依赖及许可来自 [GeoRust earcut 官方仓库](https://github.com/georust/earcut)，已写入 `Cargo.lock`；没有复制第三方实现到 vendor。
- `pomelo-import::allegro::semantics::copper::CopperDecoder` 只填充网络归属链中的 class 6 铜皮。普通形状保留外环/孔洞和解析路径；hatch 的外路径、辅助线链及孔洞路径输出为走线；两种矩形记录围绕第一个存储角旋转；板框不要求网络归属。
- `Zone` 持有解析 `paths` 与独立覆盖 `mesh`，不再用一组不区分外环/孔洞的三角形表达整块铜皮。Windows 正式后端的覆盖合成还未接入；已验证的方孔 SDF 实验保持原范围。

新增 CLI：

```powershell
python scripts/cargo.py +stable run -p pomelo-import --bin pcb_inspect --locked --offline -- --locale zh-CN --encoding windows-1252 decode-copper FILE --records REQUEST.json --report REPORT.jsonl
```

请求仍绑定源 SHA-256、大小及索引中的 Key/类型/起止偏移。输出包含 metadata、查询对象及结构化诊断，所有行保持 `scene_validated: false`。新增网格构建失败、铜皮边界为空、矩形图层未定义三种消息同步英/简中/繁中/日/韩；驱动或算法技术详情与译文分开。

## 真实案例对照

从冻结的 156 案例清单中选用计划指定的六块代表板。每文件、每种记录类型 `0x0e / 0x14 / 0x24 / 0x28` 最多 32 个均匀源序号，查询其完整后继几何和所有孔洞。合计 **704 条请求全部通过**。

| 案例 | 布局族 | 请求 | 铜皮 | 顶点 | 孔洞 | 曲线铜皮 | hatch 段 | 板框段 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `15061-1b.brd` | 165 | 128 | 6 | 38 | 0 | 0 | 0 | 0 |
| `AGILEX_I_SERIES.brd` | 174 | 128 | 2 | 2,484 | 23 | 2 | 0 | 0 |
| `ML623_BRD_revD_rdf0074.brd` | 157 | 128 | 4 | 490,675 | 4,619 | 2 | 0 | 0 |
| `S5000C-64_DDR5_BGA_V0.61.brd` | 174 | 128 | 1 | 126 | 0 | 1 | 0 | 0 |
| `SS8633A_AMPB_FPC_DOE_V2_HVT_A_0423_1716.brd` | 172 | 64 | 15 | 410 | 0 | 10 | 248 | 9 |
| `ntpcb_320mb.brd` | 174 | 128 | 0 | 0 | 0 | 0 | 0 | 0 | 45 |
| 合计 | — | 704 | 28 | 493,733 | 4,642 | 15 | 248 | 54 |

累计比较 1,453,179 个三角形索引、2,554,471 个叶字段。`ntpcb_320mb.brd` 的本轮均匀查询没有命中已归属铜皮，因此该文件只提供板框证据，不能宣称其全部铜皮通过。

独立 oracle 使用冻结 Web 的**完整** `AllegroSceneBuilder` 生成预期，再按预先确定的源查询 ID 提取对象；预期不由 Native 报告反推。检查源/Web 哈希、案例集合、源类型计数、确定性抽样、索引身份、单位、解析路径、顶点、三角形、外环数量、环偏移/顺序、环与分块包围盒、曲线标记以及查询范围内的诊断。

对象 ID、图层/网络及所有索引、数量、顺序精确比较；坐标/角度容差为 `1e-12 × max(1, |Web|, |Rust|)`。验证器曾把解析圆弧 `start` 误匹配为整数，现已限定只有孔洞块的 `start/count` 为整数，三角形索引仍精确比较。Web 实际 `earcut 3.2.3` 的 package 与源码哈希单独冻结，避免第三方依赖变化而未被记录。

本地证据：

- `.cache/copper-six-indexes.jsonl`、`.cache/copper-six-probes/manifest.jsonl`：六案例选择、请求与 Native 输出。
- `.cache/copper-earcut-baseline.json`：Web 三角化依赖版本和源码哈希。
- `.cache/copper-six-parity.jsonl` 及 `.summary.json`：独立对照，matched 6 / failed 0。
- `.cache/copper-final-six-probes/`、`.cache/copper-final-recheck.json`：最终 Windows 工作区构建重新生成的查询，数组/数值精确一致，仅忽略 JSON 对象成员顺序。

原始 BRD 和 Web 源码保持只读。两端显式 Windows-1252，仍不代表原始文件实际编码已识别或源文字已验收。

## 负向验证、预算与边界

只修改 `.cache/copper-negative/` 中的报告副本：

| 变更 | 结果 |
| --- | --- |
| `15061-1b.brd` 某铜皮的第一个三角形索引改为另一顶点 | 拒绝：`shape=2181309208.zone.indices.0`，Web 1 / Rust 2 |
| `AGILEX_I_SERIES.brd` 某铜皮交换两个孔洞排序值 | 拒绝：`shape=1287612.zone.ringOrder.1`，Web 14 / Rust 1 |

两次对照均退出码 1。合成测试覆盖重叠孔洞并集、非凸/逆向轮廓、共线/重复点、空间排序与稳定相同 Morton 值、64 孔洞块、解析圆弧包围盒、预算、取消、空边界、未归属对象、矩形旋转、hatch 链缺失/环和板框身份。

默认单网格预算：1 GiB 输出与保守 scratch、8,000,000 点、65,536 环；构建前核对 u32 顶点/索引及 earcut 节点偏移上限。保守 scratch 按最大环每点 512 B 计入；借用输入不计入 core 网格预算，由导入器同时计入路径/环输入及剩余对象预算。导入器默认累计返回对象预算 2 GiB / 8,000,000 条，几何查询与链遍历另有各自门槛。

扫描、拷贝、凸轮廓检查、解析边界和索引写入有取消检查；每次 earcut 前后也检查。**earcut 单个轮廓调用内部没有取消回调**，当前不能宣称已满足产品取消延迟门槛；完整场景、大环最坏耗时与内部抢占仍需实测和改进。

本轮 Windows 工作区 190 项测试、Clippy 全目标/特性及 debug 构建通过。当前仍需完成文字/绘图、场景总预算与 SceneBuilder，再进行 Native 整板差分和真实 Windows GPU 视口验收。本轮不是 156 案例完整铜皮、release 性能或跨平台验收。

## 复现入口

```powershell
python scripts/probe-records.py .cache/copper-six-indexes.jsonl .cache/copper-six-probes target/debug/pcb_inspect.exe --copper --samples 32
node --max-old-space-size=24576 --import file:///C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/tsx/dist/loader.mjs scripts/check-copper-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo .cache/copper-six-probes/manifest.jsonl .cache/web-inputs.json .cache/cases-manifest.jsonl .cache/indexes-windows1252.jsonl .cache/copper-earcut-baseline.json .cache/copper-six-parity.jsonl
python scripts/cargo.py +stable test --workspace --locked --offline
python scripts/cargo.py +stable clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
python scripts/cargo.py +stable build --workspace --locked --offline
```

本地 frozen/case/index/earcut 清单需要保留；不随仓库分发外部案例和缓存。Node/Web 只用于研发对照，正式应用运行不依赖它们。
