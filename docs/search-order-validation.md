# 搜索排序与最新 Web 对照

日期：2026-10-04。Windows 正式搜索已接入平台无关的 ICU4X / Unihan 离线排序；本记录验证名称匹配、排序和结果身份，不代替完整查看器验收。

## 行为与实现

沿用最新 Web `src/lib/board/search.ts`：完全匹配优先，其次前缀，再按 Unicode 名称排序；相等的排序权重保留源插入顺序。数字保持 Web 默认 `numeric=false`，例如 `U1 → U10 → U2`。没有增加自然排序、重音不敏感匹配、源名称翻译或归一化去重。

Web 的 `toLocaleLowerCase()` / `localeCompare()` 未指定语言，使用 JavaScript 运行环境默认语言，与界面翻译语言无关。桌面工作台创建时冻结系统语言，按既有五语映射选取排序数据；未支持系统语言回退英文。切换界面语言不重排已有文档。浏览器偏好与系统语言不一致时，两端必须先对齐排序语言再比较；本记录不宣称不同运行语言下仍有唯一相同顺序。

`pomelo-core/search` 持有不可变 Collator，不使用 GPUI、GPU、Windows 排序 API 或浏览器。查找保留原名称、typed target、成员计数与稳定身份；查询最多保留 `limit` 个引用，取消不发布部分结果。Rust 小写处理覆盖本轮五语样本；首尾空白明确采用 ECMAScript 集合，包含 BOM、保留 NEL。

初始化错误使用稳定 `SEARCH_COLLATION_UNAVAILABLE`、参数 `locale` 和 rust-i18n 五语摘要，第三方详情独立保留；应用通过正式文档失败路径显示，不静默退回字节顺序。`SearchIndex::new` / `build` 返回可检查错误，所有生产调用和已有测试已适配。ICU provider 显式启用 `sync`，线程测试及 GPUI 编译覆盖后台索引共享。

离线资源固定为 1,284,927 字节，编译进核心；普通构建、运行不调用生成器。版本、来源、许可、哈希及复现入口见[资源说明](../crates/pomelo-core/src/search/data/README.md)。Windows 继续使用 D3D11/HLSL，UI 字体源文件哈希保持 `2cb472958b85ea4a32316961c2902b535903697719b9743231743bf1e5c0ed70`。

## 独立 Web 证据

脚本直接导入当前 Web 的 `BoardSearchIndex`。五语对照仅为 JavaScript 内置大小写/比较函数指定运行默认语言，不改搜索类的匹配、排序、heap、ID 或稳定排序规则。还单独运行无适配器的实际默认语言 `zh-CN`。JSON 对象先归一化字段顺序，结果数组及插入顺序保持原样。

真实板由两端各自完整解析及构建搜索条目；先比较全部条目名称、身份、计数和源顺序，再比较空词、完整名、大小写、前后缀及六种结果上限。合成输入额外覆盖 CJK 混排、扩展汉字、假名、韩文组合、重音、组合字符、标点、数字、补充平面和重复名称。

| 样本 | 条目 | 查询/语言 | 五语查询总数 | 差异 |
| --- | ---: | ---: | ---: | ---: |
| 合成名称 | 242 | 2,514 | 12,570 | 0 |
| USBC_FPC | 29 | 420 | 2,100 | 0 |
| camera_test_board | 49 | 576 | 2,880 | 0 |
| AGILEX_I_SERIES | 7,642 | 1,836 | 9,180 | 0 |
| 合计 | — | — | 26,730 | 0 |

基线：Node 24.18.1 / ICU 78.3 / CLDR 48.0 / Unicode 17.0。Web search.ts SHA-256：`880778293ad3c75f43ce42daaf52e1d584cb35bb57b67b21e4a3e1e78dedfa81`。正式探针 SHA-256：`a6ae1eef63b33f2c178adb1520b8d8c9b2aa72cbf2b79ae3f099c08cddd998f5`。

报告在 `.cache/search-parity/{synthetic,fpc,camera,agilex}/report.json`，同目录保留输入、完整两端结果、案例哈希、运行语言和原生查询时间。旧字节排序对照累计 1,065 组差异（合成 455、FPC 150、camera 0、AGILEX 460），证明样本能够区分旧算法；此计数只比较排序，不把旧 Rust trim 行为混入。

初次脚本按 JSON 字段顺序误报，旧报告 `synthetic/key-order-adapter-report.json` 保留；不是产品错误。随后使用 ICU4X 2.3 和 2.1 默认内置数据均得到 78 组真实差异，最小样本包含 `線 / 线 / 中 / 𠀀`。核对官方生成源码后确认默认 `CollationRootHan::Implicit` 与 Web ICU 的 Unihan 根不同；使用统一 Unihan 根与对应语言 tailoring 的生成数据后全部消除。没有移除失败字符或缩小结果上限。2.3 的失败证据保留为 `synthetic/icu-2.3-{differences,native}.json`。

复验示例：

```powershell
python scripts/cargo.py +stable build -p pomelo-import --example search_order_probe --release --offline --locked
node --import file:///C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/tsx/dist/loader.mjs scripts/check-search-order-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo target/release/examples/search_order_probe.exe - .cache/search-parity/synthetic synthetic
node --import file:///C:/Users/Zen/Desktop/gitrepo/pomelo/node_modules/tsx/dist/loader.mjs scripts/check-search-order-parity.mts C:/Users/Zen/Desktop/gitrepo/pomelo target/release/examples/search_order_probe.exe E:/brd_cases/AGILEX_I_SERIES.brd .cache/search-parity/agilex windows-1252
```

## 回归与剩余门槛

工作区全目标/全特性 478 项测试通过、17 项按既有策略忽略；五项新增回归覆盖 Unihan 五语顺序、canonical tie、数字顺序、ECMAScript 空白、Send/Sync 与取消。Clippy `-D warnings`、格式检查、脚本语法及真实数据再生成哈希检查通过。日志为 `.cache/search-collation-*.log`。

本轮最终 Release 构建与实窗验证结果另补；不把先前 Release 或独立探针当作新版应用通过。查询微秒为探针单次取整时间之和，只作诊断，不是六代表板性能报告。完整案例、所有显示/隐藏与 finger-only 工作流、五语输入法、DPI、GPU 故障恢复、长时间运行及发布包仍按总计划验收。

上游契约：[ICU4X Collator](https://docs.rs/icu_collator/2.1.1/icu_collator/struct.Collator.html)、[官方数据生成器](https://docs.rs/crate/icu4x-datagen/2.1.1)。
