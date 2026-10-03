# 最近文件缩略图验证

日期：2026-10-03。依据 `docs/ui-design/03-welcome-dark.png`，补齐欢迎页预览中的主要 PCB 几何。

## 实现与边界

原先只抽样绘制走线中心线。现在 `pomelo-render/src/scene/thumbnail.rs` 消费正式视口的不可变批次，纳入走线/圆弧、铜皮、解析焊盘、自定义焊盘、物理钻孔、绘图及板框。应用的 `services/preview.rs` 负责 PNG 编码、缓存解码和 GPUI BGRA 转换；材料配色由主题模块提供。没有新增依赖，也没有修改 GPUI 补丁、D3D11 设备或 Shader。

预览在导入后台任务生成，固定 192×128 RGBA。七个阶段各自最多 2,000,000 次像素区域工作；非板框实例每类均匀抽样至最多 20,000 项，铜皮和自定义焊盘阶段各有 100,000 个三角形预算。板框使用独立像素预算。铜皮先预留包含全部孔洞的整块工作量，预算不足跳过整个批次；孔洞采用覆盖并集扣除，不将重叠孔洞重新填回，也不擦除下层铜皮。

这是辨认文件的概览 LOD，不是 GPU 回读、当前视角截图或完整板图导出。板文字、选择、高亮和当前图层过滤不进入预览；曲线铜皮/自定义焊盘使用既有填充网格。细走线采用最小可见宽度。大板可能抽样或省略整块铜皮，不能用于完整覆盖、电气关系或加工细节验收。

取消令牌在绘制过程检查，发布前再次检查导入取消状态。错误继续使用既有 `RecentPreviewPrepareFailed` / `RecentPreviewLoadFailed` 五语消息，没有新增硬编码界面文案。

## 回归检查

新增 8 项回归覆盖：铜皮重叠孔洞及下层保留；焊盘旋转/镜像/偏移/孔洞；独立钻孔及 donut 开口；走线宽度/圆帽/定向圆弧；整块铜皮预算；大坐标残差；取消及无效边界；正反整圆。

整圆补充测试曾实际失败：float32 的整圈角度转换为 f64 后略超 `TAU`，被 CPU 查询拒绝。修正读取批次的显式整圆标志并处理 float32 边界，正反方向回归通过。

最终渲染库启用 `native-gpu` 后 **74 passed / 5 ignored**；应用 **53 passed**；五语资源和源码门禁 **9 passed**；工作区全目标全特性 Clippy 与格式通过。忽略项包括硬件/外部资源测试，本轮未重新执行 GPU 硬件像素验收；两个外部缩略图测试已另行显式执行。

```powershell
python -X utf8 scripts/cargo.py +stable test -p pomelo-render --features native-gpu --locked --offline
python -X utf8 scripts/cargo.py +stable test -p pomelo --locked --offline
python -X utf8 scripts/cargo.py +stable test -p pomelo-core --test i18n_resources --test i18n_source --locked --offline
python -X utf8 scripts/cargo.py +stable clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
```

外部验证显式传入本地路径，仅在设置 `POMELO_PREVIEW_PNG` 时写出预览，源 BRD 保持只读。LPDDR4 使用同一命令替换两个路径。

```powershell
$env:POMELO_PREVIEW_BOARD_PATH = 'E:\brd_cases\USBC_FPC.brd'
$env:POMELO_PREVIEW_PNG = "$PWD\.cache\ui-validation\thumbnails\USBC_FPC.png"
python -X utf8 scripts/cargo.py +stable test -p pomelo-render --features native-gpu external_board_thumbnail --locked --offline -- --ignored --nocapture
```

## 真实案例

耗时仅包括 CPU 光栅化，来自本机优化的 debug 测试构建；不包含导入、批次准备或 PNG 编码，不作为正式性能基准。数量是纳入预览的绘制数量，不是源板统计。

| 案例 | 光栅化 | 走线 | 绘图 | 铜皮批次 | 焊盘实例/批次 | 钻孔 | 板框边 | LOD 限制 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| USBC_FPC.brd | 7 ms | 2,958 | 5 | 176 | 459 | 123 | 173 | 否 |
| LPDDR4_case.brd | 16 ms | 9,231 | 60 | 87 | 12,670 | 2,326 | 8 | 是 |

例如 LPDDR4 源板有 97 块铜皮，预览绘制其中 87 块。欢迎页层数、文件名和日期继续来自正式历史元数据。

两个原案例在实窗验证前后 SHA-256 相同：USBC `550CF739AF80C2B5B18A4DE91419ADF2ECD1133CC2644EB0149D21B71E52B44B`；LPDDR4 `A712461845974C82C7A97067FEEB962CD66824D2753D3A2976F2F3ECB11DAA62`。

## 实窗与恢复

最终 Release 私有副本实际导入 USBC，Ctrl+W 返回欢迎页后显示新预览，简中默认 1440×920 完成深浅主题观察（工具截图含边框 1443×921）。切换浅色后 Ctrl+Q 退出，再用同一私有配置打开 `.cache/ui-validation/cases/真实 PCB/LPDDR4 布局.brd`（原 LPDDR4 的本地副本）。中文/空格路径导入成功，浅色主题恢复；返回欢迎页后，新 LPDDR4 预览与未重新导入的 USBC 旧预览同时显示。随后深色双预览也已观察。

另一次正常退出后，仅将私有历史的 USBC `thumbnail_png` 改为无效 base64。重启保留两条历史，LPDDR4 预览正常、USBC 显示 CPU 图标占位，状态栏显示已翻译的加载失败和重新打开建议。点击该历史项正式导入，再返回欢迎页，预览恢复、警告消失。该故障实窗范围是简中深色，不扩大为五语/DPI 全部通过。

运行副本位于 `.cache/ui-validation/profiles/c8fa9b3a5029456eba73253a9194ba0f/pomelo.exe`，与下述最终 Release 的 SHA-256 一致。三次均经 Ctrl+Q 正常退出，窗口清单确认私有窗口已移除。日常 `language.json/theme.json/recent.json/views.json` 哈希均与原基线相同。

本地截图：`.cache/ui-validation/thumbnails/welcome-two-light.jpg`、`welcome-two-dark.jpg`、`preview-corrupt-cache.jpg`、`preview-recovered.jpg`。实际窗口截图与板预览为私有验证数据，不作为公开 fixture 或发布资源提交。

## 产物与未验范围

最终 debug 构建通过（14.60 秒），Release 通过（1 分 00 秒）；两个 PE 子系统均为 **2 / Windows GUI**。上述实窗流程仅在最终 Release 副本执行。

| 产物 | 字节数 | 生成时间 | SHA-256 |
| --- | --- | --- | --- |
| `target/debug/pomelo.exe` | 47,741,440 | 2026-10-03 12:22:22 | `CE8E2E18476A7178B16A75658922AAEC5DC2C818269155BD70D266E4B7634299` |
| `target/release/pomelo.exe` | 35,450,880 | 2026-10-03 12:14:05 | `69380AA23759805CEB611461DB9AC0FFE85FE074B8FB8BDFA7D07049B641A5CA` |

最近文件行的 Button、菜单和焦点结构未改动。全部页面键盘、五语全流程、DPI、多显示器及自动像素差分仍待专项验收；完整 UI 和 BRD MVP 不据此标记完成。
