# 索引几何定位边界验证

日期：2026-10-03。Windows 应用的搜索、检查器定位及选择恢复改用共享 `SegmentIndex::selection_bounds`。UI 界面字体和 Web 源码保持不变。

## 实际 Web 契约与修正

重新检查最新 `BoardIndex.locateSteps` 后确认：**bounds 汇总所有成员的索引条目，包含隐藏图层；显示状态只决定优先选用哪个锚点**。此前元件分组报告中的“显示相关定位 bounds 待对齐”是过宽的描述；不应把隐藏图层过滤出定位范围。本轮对齐索引几何边界，Web 的搜索锚点选择仍需另行验证。

平台无关实现位于 `pomelo-core/interaction/picking_index/locate.rs`。成员原点、placement 位置不作为额外几何；不支持的 pad 不生成定位边界，完全没有可绘制条目的成员得到 `None`，应用沿用已有五语空定位反馈。pin 的 backdrill 标志不产生普通 pad 条目；via 的普通 pad、钻孔、base pad 和独立 backdrill 条目按 Web 索引契约计入范围。铜皮只用外轮廓的中心线或第一 mesh 环，不让孔洞坐标或笔画宽度扩大范围。

绘图定位复用拾取索引中的实际描边及 MSDF 字形 quad 边界，删除应用定位路径对字符数量、字符宽度和行高的近似依赖。字体 metrics 仍来自原 Web 方案；没有更改 UI 字体或字体资源。完整组成员参与范围计算，不受检查器 256 项分页限制。原始 `SelectionTarget::bounds` 继续作为独立源几何 API，未改变它的现有调用契约；应用正式定位使用新入口，后台取消不发布部分结果。

## 差分结果

`selection_bounds_probe` 使用正式 importer、MSDF 布局和 `SegmentIndex`，不初始化 GPU。`check-selection-bounds-parity.mts` 调用真实 Web `locateSteps`/`boundsFor`，并同时记录旧源边界 API 的结果。所有网络和元件搜索条目全部检查；各对象类别和 track 各至多均匀抽取 64 个，不能将抽样对象结果视为全部对象验收。

| 输入 | 网络 | 元件组 | 对象 | track | 总查询 | 新入口差异 | 旧源入口差异 |
|---|---:|---:|---:|---:|---:|---:|---:|
| USBC_FPC / utf-8 | 18 | 11 | 257 | 64 | 350 | 0 | 0 |
| camera_test_board / utf-8 | 32 | 17 | 332 | 64 | 445 | 0 | 0 |
| AGILEX_I_SERIES / windows-1252 | 3,828 | 3,814 | 320 | 64 | 8,026 | 0 | 0 |
| 自有 selection-navigation-bounds.json | 3 | 4 | 12 | 0 | 19 | 0 | 18 |

三块真实板合计 8,821 查询，连同合成场景共 8,840 查询，全部在 `1e-6 mm` 容差内。最大实际误差约 `1.42e-14 mm`。真实板中旧算法也全部匹配，不能据此声称在这些板上复现过旧错误；合成场景明确区分偏移 pad、无几何成员、不支持 pad、pin backdrill 标志、多语旋转/镜像/多行文字和铜皮外环规则，18 个旧结果不匹配，修正后全部匹配。

合成适配器只转换源字段布局，所有边界和字形由真实 Web 类计算。fixture 的一个减去轮廓故意超出外环，用于区分“全部源环”和“外环索引”契约，它不代表合法 PCB 设计或真实 BRD 解析。报告位于 `.cache/canvas-parity/locate-{fpc,camera,agilex,synthetic}/report.json`，包含输入 SHA-256、Web 源码指纹、完整新旧结果及运行时版本。

六项核心回归另覆盖 600 个成员越过首个分页、typed 身份、准确 glyph、空几何和取消。最终工作区全目标全特性测试 **439 通过、0 失败、17 默认忽略**，包含本轮图层列表顺序回归；Clippy、格式、debug/Release 构建通过。默认忽略的硬件测试不计为本轮执行通过。

复验入口：

```powershell
python scripts/cargo.py +stable build -p pomelo-render --example selection_bounds_probe --locked --offline
& C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/.bin/tsx.cmd scripts/check-selection-bounds-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo target/debug/examples/selection_bounds_probe.exe E:/brd_cases/USBC_FPC.brd .cache/canvas-parity/locate-recheck utf-8
& C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/.bin/tsx.cmd scripts/check-selection-bounds-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo target/debug/examples/selection_bounds_probe.exe tests/fixtures/selection-navigation-bounds.json .cache/canvas-parity/locate-synthetic-recheck synthetic
```

## 实窗与退出恢复

同一最终 debug 的私有简中深色副本完成 FPC 元件 J1 搜索定位：`ComponentGroup(Pin(ObjectId(15255)))`、30 个 pin，观察到完整成员及选择轮廓，缩放约 1071%。使用 Ctrl+Q 正常退出，私有 `views.json` 存储 typed group；重开相同副本和配置，选择目标、全部 30 成员、相机中心/缩放/翻板状态精确恢复，实窗可见选择轮廓。该闭环补齐上一轮“新组真实落盘及重开未验”的单例边界；多文档、finger-only 恢复及全部语言仍需验证。

遥测为 `.cache/canvas-parity/locate-window-j1.json`、`locate-reopened-window.json`，重开 accessibility 为 `top-layer-reopened-accessibility.txt`；启动参数见 `top-layer-launch.json` / `locate-reopen-launch.json`。同设备 D3D11、RTX 5080 正常呈现，无 GPU 错误。私有验证窗口正常退出；用户正在使用的另一个实例保留，未修改其配置。

最终 debug SHA-256：`f0249a62e43ab28a4d76a53645d1a4cb58ef276a83b749b0baad22d8299b5e7f`；Release：`616f3092217d7b7ec385d4c96646e5291efd29895f851cdf0b32e3de4462f9d1`。实窗验证的是同一 debug 副本；Release 构建通过，本轮未单独实窗验证 Release。完整证据指纹见 `.cache/canvas-parity/locate-provenance.json`。

## 未完成范围

Web 搜索的可见锚点和显示 rank 选择、全部对象与 156 案例、多语查询排序、滚轮映射、整板视觉、DPI/设备恢复与性能/发布仍未完成。没有以索引 bounds 对照代替整板 GPU 或全部 UI 验收。macOS/Linux 本阶段不实施。
