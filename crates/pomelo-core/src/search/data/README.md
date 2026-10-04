# 离线搜索排序数据

`unihan.postcard` 是官方 ICU4X 生成器的输出，编译时嵌入 `pomelo-core`；应用运行及普通构建不执行生成器、不读外部数据文件、不联网。

- 生成器：`icu4x-datagen 2.1.1`。
- 数据来源：CLDR `48.0.0`、ICU export `release-78.1rc`；冻结版本用于对照当前 Web 的 Node 24.18.1 / ICU 78.3 / CLDR 48.0 / Unicode 17。
- 根汉字顺序：**Unihan**。ICU4X 默认 `implicit` 不能替代此项，已在扩展汉字及简繁名称中复现差异。
- 语言：`en`、`zh-CN`、`zh-TW`、`ja`、`ko` 及生成器所需回退数据。
- 标记：七项 Collation V1，以及 `NormalizerNfdDataV1`、`NormalizerNfdTablesV1`；不包含日期、货币或分词资源。
- 大小：1,284,927 字节。
- SHA-256：`37c34672d6bae78434672a07d301c9320181d3416edba80cf69d3bdeb8c85dab`。
- 许可与版权：相邻 `LICENSE`（Unicode License V3）；发布时保留通知。

开发阶段的数据生成与 Web 对照脚本已移除。保留冻结数据、版本、哈希及许可记录；普通构建直接嵌入现有资源。用户界面和错误摘要仍统一使用 rust-i18n 五语消息。

上游说明：[ICU4X 数据生成器](https://docs.rs/crate/icu4x-datagen/2.1.1)、[Collator 数据接口](https://docs.rs/icu_collator/2.1.1/icu_collator/struct.Collator.html)。
