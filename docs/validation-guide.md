# 验证方法

## 原则

验证应对应可观察行为，并区分核心算法、CPU 场景准备、GPU 像素和实际窗口交互。用合成几何覆盖边界；外部文件验证仅用于已授权的本地检查，不将文件名、路径、设计图或原始日志写入公开文档。

## 常规检查

以 `v` 或 `V` 开头的分支用于 release 打包：推送仅触发发布打包工作流，普通 CI 跳过；PR 的来源或目标是这类分支时，普通 CI 检查也跳过。对应版本标签（如 `v0.3.0` 或 `V0.3.0`）推送后执行打包并发布 GitHub Release，标签版本须与 `Cargo.toml` 一致。发布流程内部仍保留原有构建和包验证。

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

只修改文档时检查链接、路径和命令。代码修改先运行最近的测试，跨模块接线再扩大范围。上述命令是检查入口，不表示本次文档维护已经执行它们。

## 几何与交互

Allegro 18.0/18.1 的合成回归覆盖 `0x00150000` / `0x00150200` 布局、重定位头部、正序链表、92 字节配对网络记录、记录间零填充及头部哨兵终止。`fixed_decoder` 和 `index` 另验证指针数组按源容量读取，覆盖 0、1、100、4096 项及预算、截断边界。完整导入检查网络归属、坐标、文字间距和取消；未知链表引用与截断记录仍须报错：

```sh
cargo test -p pomelo-import --locked --test allegro_181
```

本地批量样本可通过 `scripts/validate-brd-cases.py` 和 `validate_brd_case` 示例验证。以文件头选择布局，不能从目录名或编辑器版本推断；场景构建成功与无诊断通过分别统计。

Allegro 26.1 的合成测试覆盖文件标识 `0x00170300`、头部字段、记录边界和解码。另有本地样例回归，检查完整导入的对象数量、网络引用、有限坐标和无诊断结果：

```sh
POMELO_ALLEGRO_261_CASES=/path/to/local/cases cargo test -p pomelo-import --locked --test native_cases allegro_261 -- --ignored
```

该检查只验证 CPU 导入，不代表 GPU 显示验证。

EDB 12.1 的本地十二层样例回归检查对象数量、网络和元件引脚引用、有限几何、铜区网格及搜索索引。输入文件保留在仓库外，通过环境变量指定：

```sh
POMELO_HFSS_12_LAYER_CASE=/path/to/local/edb.def cargo test -p pomelo-import --locked --test hfss_case -- --ignored
```

该样例使用现有 HFSS 原生读取器；通过 CPU 导入检查不代表完成画布显示或完整三维数据验收。

多版本 EDB 样例可设置 `POMELO_EDB_CASES` 后运行 `hfss_case` 中的 `edb_archive_versions` 测试；字段表规律和未见版本的兼容边界见 [EDB 格式兼容](edb-format-compatibility.md)。

带非零元件变换、矩形钻孔和辅助图形层的本地 DEF 可设置 `POMELO_HFSS_T40_CASE`，运行 `hfss_case` 中的 `transformed_components` 测试。三维孔壁检查使用 `POMELO_HFSS_3D_CASE` 指向 DEF，然后运行：

```sh
cargo test -p pomelo-render --test board3d imported_hfss_plated --locked -- --ignored --nocapture
```

该检查核对每个可解析的镀层 Via 都有全跨度且材质可见的三维实体。三维预览中的所有 Via 都按实心显示，不挖孔；常规 `board3d` 回归核对圆形、矩形、槽形和多边形 Via 的中心有覆盖，且铜区和走线保持完整。Pin 孔仍按原始轮廓切除。WARP 像素回归同时覆盖三维实心 Via 与二维精确多边形孔。TEDB 本地导入回归命令见 [EDB 格式兼容](edb-format-compatibility.md#pyedb-tedb-回归)。

几何检查覆盖直线、圆弧、退化路径、凹轮廓、孔洞、重复点和极端坐标。相机检查覆盖坐标往返、光标锚点缩放、适应整板及最小可见范围。

搜索与拾取检查覆盖排序、对象分组、类别过滤、隐藏图层、组成员边界及定位范围。合成数据应有可独立计算的预期结果，不能仅复述实现。

## 渲染

CPU 检查批次、资源预算、取消、增量准备和缓存失效。GPU 检查裁剪、透明度、层序、孔洞、文字、覆盖层和高 DPI，像素读回仅作为测试手段。

硬件测试需要对应平台和设备。被忽略或未运行的测试单独列出，不能算作通过。不同平台的画面语义需要实机复核。

## 桌面交互

使用[隔离配置](ui-validation-profiles.md)检查五语、明暗主题、窄窗口、键盘导航、焦点恢复、多文档、后台取消、重载和正常退出。保存测试包括损坏配置、无法写入、重复更新和重启恢复。

## 性能

使用 release 构建，记录平台、GPU、构建版本、数据规模、窗口尺寸、操作和统计口径。分别观察 CPU 堆、图形资源、进程内存、上传时间及持续交互表现。

停止操作后检查是否仍无意义刷新；长列表和搜索确认工作量随可见范围变化。单次读数和函数基准不代表完整应用性能，也不作为全部文件的性能承诺。
