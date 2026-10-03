# 侧栏与命令栏设计稿对齐记录

后续独立 Release 实窗、文档标签外观、最终简中字体的最低窗口补验见 [文档标签记录](workbench-tabs-validation.md)。下文保留此前 debug 阶段的证据及当时未完成项，不表示新版 Release 从未运行。

日期：2026-10-03。当前仅 Windows；依据 `ui-design/01-workspace-dark.png`、`02-workspace-light.png`，继续执行 UI 优先。此记录不代表逐像素验收完成。

## 本次修改

- 左右侧栏统一使用 Kit Large 下划线标签栏、左右 `px_4` 留白和选中标签半粗字重；保留 Kit 原生标签语义。溢出菜单作为独立可聚焦按钮放在标签栏旁边。
- 图层筛选框恢复 Kit 默认高度，搜索图标使用 `size_4`，输入状态和过滤事件沿用正式路径。
- 图层行颜色块由 `size_3` 调整为 `size_4`；列表行两侧 `px_3` 留白，展开行增加圆角与主色淡边框。源颜色、源层类型、展开、独立图元过滤、显隐及排序仍由既有数据和命令管理。
- `panels/tabs.rs` 根据 GPUI 实际文字排版宽度和侧栏当前尺寸判断标签是否溢出；全部标签按半粗字重测量，避免仅切换选中项就造成菜单入口跳变。溢出时提供 Kit 弹出菜单，列出完整名称并标记当前面板，复用视口的切换命令。没有增大左侧栏 200 或右侧栏 240 的最小宽度。
- 菜单按钮的无障碍名称及提示新增共享 `ui.panel_navigation` 消息，英文、简中、繁中、日文、韩语均已补齐。没有把语言判断或估算字符长度作为布局条件。

## 命令栏补充

命令栏展示已提取至 `viewport/toolbar.rs`，以状态快照和命令回调组合文件入口、导航工具、搜索、着色和适应整板。图层/网络着色使用 Kit `ButtonGroup` 连体边框，选中项主色填充、未选中项轮廓边框；两项保留原始 `ColorMode` 与正式重绘路径。文件、导航、着色与适应整板之间使用 Kit `Separator`；选择/拖动按钮增加边框和 `toggled` 无障碍按下状态，画布悬浮栏也同步按下语义。Kit 的 `ghost().outline()` 实际边框透明，因此导航和未选中着色按钮改用可见的 outline variant。输入实体、相机、工具与着色状态均留在各文档视口，文案仍使用共享五语消息。

## 检查器与图层设置

- 检查器详情、模式组、选择命令、拾取过滤、显示控制和文件信息共用 `px_4` 内容边缘；保留各自自然高度及既有焦点滚动。
- 拾取过滤使用设计稿顺序：走线、过孔、焊盘、铜皮，复用现有类别命令。取消每项至少占半栏的限制，按完整名称自然排列和整项换行。铜皮使用新增 `view.pick_copper` 五语消息；不改变格式诊断中的对象名称或类别含义。
- 图层设置展示归 `panels/layers.rs::settings` 所有：标题、铜皮不透明度名称、滑块和右侧百分比。滑块与数值同排，数值不收缩；新增 `view.copper_opacity_label` 与 `ui.percent(value)` 五语消息，原 SliderState、事件、显示状态和 GPU 更新路径保持。

## 字体与失效焦点

Windows 按当前界面语言选择已安装的 UI 字体：Segoe UI、Microsoft YaHei UI、Microsoft JhengHei UI、Yu Gothic UI、Malgun Gothic。枚举同时匹配 DirectWrite 返回的中文/韩文字体族名称；未安装时使用 GPUI 系统回退。不捆绑或下载字体，主题初始化、切换外观与切换语言均应用同一策略。字体改变后，标签是否溢出继续用实际排版宽度判断。

实窗发现：焦点位于溢出按钮时打开设置，切换到标签无需溢出的语言，关闭设置会恢复已移除按钮的焦点，使快捷键失去分发路径。工作台通过 GPUI `on_focus_lost` 恢复仍存在的最近祖先，缺少祖先时回到工作台；正常弹层的焦点返回继续由 Kit 管理。没有修改 GPUI 补丁或缓存代码。

## 已取得的证据及限制

使用多次实际重建的 `target/debug/pomelo.exe` 检查真实 `LPDDR4_PGB.brd`，默认窗口 1440×920。以下分别记录操作证据与范围，未操作的流程不算通过。

| 检查 | 结果与范围 |
| --- | --- |
| 图层卡片及底部设置 | 简中浅色默认窗口观察 TOP 淡色卡片、16px 色块、主色边框和独立百分比；日文深色窄栏另观察卡片及选项整项换行 |
| 窄栏 | 实际将左侧拖到 200、右侧拖到 240；英文/日文保留完整选项的换行、文件信息和底部 35% 数值。此轮窄栏证据发生在最后字体调整前 |
| 完整名称菜单 | 英文及日文窄左栏弹出全部三项并标记当前项；鼠标切换 Components 显示真实 66 元件列表，键盘切换 Nets 显示 395 个命名网络（状态统计另含无名项为 396） |
| 键盘菜单 | 从已聚焦搜索输入框，Tab 经两种着色、Fit board 到 Switch panel；Space 打开，Escape 返回按钮，Enter 重开，Down/Enter 切换网络面板。鼠标点击 Kit 按钮本身不建立键盘焦点，不能据此判断 Tab 不可达 |
| 移除焦点恢复 | 修复后，日文窄栏溢出按钮经 Tab 获得焦点；Ctrl+, 打开设置并切换韩语，Escape 关闭后，无需鼠标重新聚焦即可再次 Ctrl+, 打开设置。此项在最后字体调整前验证 |
| 字体后的默认窗口 | 最终字体版本检查英、简、繁、日、韩即时切换及两侧完整标签；英文默认 256/288 侧栏可显示三个左侧标签和四个检查器模式，拾取 Copper 按整项换行。设置弹层中的语言选项与说明可见，不代表所有字体/DPI 的字形质量验收通过 |
| 透明度联动 | 最终版本实际拖动 35%→59%→35%；右侧数值及真实铜皮 GPU 画面同步变化。UIA 数字观察有一帧滞后，重新读取后为 59。UIA set_value 曾返回窗口查找错误，未改变数值；重新选择同一窗口后以拖动恢复，应用仍正常运行 |
| 着色命令 | 最终版本实际点击图层着色，再切回网络着色；主色选中项与连体轮廓跟随状态，真实板颜色同步切换 |

此前 debug 文件锁定已通过正常 Ctrl+Q 退出、重新选择最新窗口对象及重建解决，未强制结束进程。最终 debug 构建通过；最终 Release 构建通过，耗时 1m 01s，生成 `target/release/pomelo.exe`。应用 51 项、i18n 9 项、全工作区全目标/全特性 Clippy、格式检查和 `git diff --check` 均通过。最后一次实窗运行是上述 debug 版本；Release 尚未单独运行，不把构建结果视为 Release 实窗验收。

最后尝试最小窗口复验时工具检测到并发用户输入，输入停止；只读刷新确认窗口仍在运行并出现新的用户选择/缩放，未继续改变视图。**最终字体版本的 1000×650 窗口、200/240 窄栏组合及溢出消失焦点场景还需复验**。过滤后列表高度、完整五语键盘路径、DPI/多显示器和逐像素差分也尚未完成。本记录不把此前字体下的布局证据冒充最终字体下的结果。

## 复验命令

```powershell
python -X utf8 scripts/cargo.py +stable fmt --all -- --check
python -X utf8 scripts/cargo.py +stable test -p pomelo --locked --offline
python -X utf8 scripts/cargo.py +stable test -p pomelo-core --test i18n_resources --test i18n_source --locked --offline
python -X utf8 scripts/cargo.py +stable clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
python -X utf8 scripts/cargo.py +stable build --release -p pomelo --locked --offline
# 正常退出正在运行的 debug Pomelo 后执行。
python -X utf8 scripts/cargo.py +stable build -p pomelo --locked --offline
```
