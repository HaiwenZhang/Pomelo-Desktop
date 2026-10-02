# 显示控制实现与验证

日期：2026-10-02，仅 Windows。

恢复源顺序命令：图层面板新增五语 Button，清空当前文档自定义 layer_order，使既有 ordered_layers 恢复源顺序；同步请求滚动至首行并失效 hover。无自定义顺序时禁用。显示设置和持久化继续使用现有 ViewState，不增加配置字段。9 项 i18n 资源/源码检查、应用 Clippy 与 Windows debug 构建通过；按钮操作、重新打开后的恢复和 GPU 画面仍待实窗验收。

长图层名的虚拟行高度约束：Checkbox 标准内容槽改用单行 truncate 文本，完整名称同时作为 accessibility_label 和标准 Tooltip，避免长名称换行使 uniform_list 的统一行高假设失效。名称来自 PCB 源数据，不翻译；命令保持现有五语。应用 Clippy 与 Windows debug 构建通过；实际省略宽度、Tooltip 键盘触发、五语与 DPI 行高仍待实窗测量。

虚拟化后的滚动条接线修复：图层列表外层使用 GPUI Kit vertical_scrollbar，与 uniform_list 的 track_scroll 共用同一个文档滚动句柄。外层只提供布局及滚动条叠加，不创建另一滚动区域；行内容缩进不影响滚动条容器边界。应用 Clippy 通过。拖动滑块、面板边界定位、深浅主题和 DPI 表现仍需实窗验收。

虚拟列表状态补齐：每个 BoardViewport 独立持有 UniformListScrollHandle，通过 track_scroll 绑定列表，刷新时复用句柄；置底/置顶成功后分别请求索引 0/末行的 Nearest 滚动，让操作对象进入可见范围。未新增跨启动滚动位置持久化。应用 Clippy（all-targets/all-features，`-D warnings`）和 Windows debug 构建通过。滚轮、排序后的定位、标签切换及五语行高仍待真实窗口验证。

配置文件专项补验：扩展真实磁盘 ViewStore 连续文档更新回归，匹配身份重新读取后保留排序、铜皮/文字/辅助图形显隐和 0.75 不透明度。重复层 ID 的更新返回五语错误且原文件字节不变。`test -p pomelo sequential_view_updates --locked --offline` 1 passed、0 failed，Clippy 通过。运行报告新增实际 layer_order 字段，供后续窗口画面与状态对照；这仍不代替真实退出/重启验收。

后续图层顺序进展：每文档 layer_order 保存底层到顶层绘制顺序；每层五语“置于底层/置于顶层”命令移动到对应端点并保留其他层相对顺序，已在端点则禁用。列表与 GPU 帧共用完整顺序，包括渲染发现的补充层。旧配置默认源顺序，新层按源顺序追加，未知层忽略，保存状态拒绝重复 ID。核心库测试及 Clippy 通过；硬件/真实窗口排序、长标签密度和跨启动仍未验收，因此不将下文较早的整体测试结果作为新增排序 UI 的最终验收。

随后图层排序硬件回归通过：使用 BoardDisplay::move_layer_to_edge 将蓝色铜皮层移到红色走线层上方，重叠像素由纯红变为混合，红通道在 126–129；恢复原顺序后整幅像素与初始一致。走线及铜皮上传字节保持不变、缓存重建均仍为 1。命令 `test -p pomelo-render --features native-gpu hardware_copper_overlapping_holes_preserve_underlying_trace --locked --offline -- --ignored --nocapture`，1 passed、0 failed、59 filtered out，exit 0；最新 Clippy 通过。此证据为离屏硬件绘制，不包含实际列表按钮、窗口或跨启动恢复。

当前每文档支持铜皮显示、不透明度、板上文字显示、辅助图形显示和钻孔显示。铜皮不透明度按钮按整百分比调整，每次 5%，范围 0–100%，端点禁用；其他类别使用 GPUI Kit Checkbox。标签和百分比模板同步英文、简中、繁中、日文及韩文。

状态由 `BoardDisplay` 持有，经不可变 GPU 帧快照绘制，并随既有 `ViewState` 保存/恢复。旧 schema 1 文件缺失新增字段时默认显示，铜皮不透明度默认 0.35；不透明度非有限或超出 0–1 时使用配置诊断拒绝。运行报告包含各开关和不透明度。

显式隐藏铜皮同时排除 Zone 拾取；拾取类别过滤与显示设置保持独立。文字和辅助图形目前没有独立拾取支持。隐藏类别只跳过绘制，不清除常驻 GPU 批次；隐藏状态仍可能完成已有上传任务，不能据此声称节约全部准备/上传成本。

验证命令：

```powershell
python -X utf8 scripts/cargo.py +stable test --workspace --all-targets --all-features --locked --offline
python -X utf8 scripts/cargo.py +stable build -p pomelo --locked --offline
```

工作区 32 个测试目标，341 passed、0 failed、7 ignored，exit 0。原始输出见 [display-controls-workspace-tests.log](gpu-validation/display-controls-workspace-tests.log)。ignored 包括 4 项外部案例、2 项硬件和 1 项外部字体，未计作通过。Windows debug 构建 exit 0，二进制 SHA-256 为 `FBAFC17428430F6B71635307AB758382F8BB641C0670E23482EAA7754052E5E7`。此前最新工作区 Clippy 已通过。

另行显式执行的 Windows D3D11 硬件 fixture 验证：铜皮不透明度端点及恢复；铜皮、文字、尺寸线隐藏和重显；重显画面与初始像素一致；已有几何上传量不增加。核心回归覆盖隐藏铜皮不产生 Zone 候选、状态往返与旧配置默认值、百分比连续调节端点。硬件使用测试用离屏目标，不替代应用窗口。

仍待验收：真实板上的全部控件操作、键盘焦点与禁用状态、五语布局、主题/DPI、关闭/重开及退出/重启状态恢复。侧栏可用高度与设计稿匹配仍需真实窗口测量。焊盘填充、更多图元类别、图层顺序和着色控制尚未完成；本记录不宣称完整显示面板或 BRD MVP 已交付。
# 图层列表虚拟化补充（2026-10-02）

Windows 视口图层面板使用 GPUI `uniform_list`，由可见 Range 构建行元素，替换全部行预构建及外层滚动 div。列表保留源图层 ID 对应的 Checkbox 和置顶/置底按钮；显隐与排序继续修改当前文档的 BoardDisplay，绘制顺序使用相同规范化图层序列。元数据整理及排序仍处理完整图层列表，这次优化针对行元素构建。

应用 all-targets/all-features Clippy（`-D warnings`）和 Windows debug 构建通过。尚未实窗验证滚动条、长图层名、五语行高、键盘跨可见范围焦点及排序后的滚动定位，因此不宣称图层面板完整验收。

