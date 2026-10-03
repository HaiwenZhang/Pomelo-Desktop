# 无控制台启动错误反馈

日期：2026-10-03。依据研发计划 10.3：窗口尚未建立时也能报告启动失败，使用已解析的语言，并提供本地错误记录。

## 实现

Windows 继续使用 GUI 子系统，日常直接打开 `pomelo.exe`。参数错误、GPUI 平台构造失败和 `open_window` 返回错误改为独立于 GPUI/GPU 的原生错误对话框。标题、主要原因、技术详情标签、日志位置/不可保存提示和关闭按钮全部使用共享 rust-i18n 五语资源。

应用 Windows 依赖锁定 `rfd 0.17.2`，启用 `common-controls-v6`、关闭默认的平台依赖，使用安全的 [MessageDialog API](https://docs.rs/rfd/0.17.2/rfd/struct.MessageDialog.html)。自定义关闭按钮所需的 Common Controls v6 由已启用的 GPUI Windows manifest 提供，其 assembly 版本为 `6.0.0.0`；最终 Release 原生窗口已实际显示英文 `Close`。未新增应用 unsafe，也未修改通用 GPU 补丁或业务渲染路线。

`services/startup_error.rs` 负责早期语言读取、原生反馈和有界日志。有效的 `--locale` 优先，其次为已保存偏好，最后跟随系统并安全回退英文；无效/损坏偏好只读取，不在失败路径覆盖。参数诊断仍为 `Diagnostic`，主要文案在展示时翻译；稳定错误码和第三方技术详情单独展示。

GPUI Kit 当前的 Windows 构造函数将 `WindowsPlatform::new` 的失败传给 `expect`。应用仅在这个构造边界捕获 unwind，转换为 `APP_PLATFORM_FAILED`，显示原生反馈并退出；不捕获整个事件循环，不恢复失败的 GPUI 实例。窗口创建错误使用 `APP_WINDOW_FAILED`。参数失败的退出码为 2；平台/窗口创建失败为 1；正常关闭为 0。

日志位于配置根目录下的 `logs/startup-error.log`，未指定覆盖目录时为 Windows 用户配置中的 `Pomelo/logs/`。只保留最近一次启动错误，包含 UTC 时间、locale、翻译后的原因、稳定错误码和技术详情；使用同目录临时文件、flush/sync 与原子替换，最终最多 16 KiB。对话框与日志分别限制展示长度，按 UTF-8 字符边界截断，移除 NUL 等控制字符。不可写或无有效配置根时仍显示原始原因和本地化的日志不可保存提示，不回退写入另一份用户配置。

## 已执行检查

- 应用 59 项普通测试全部通过，新增 6 项覆盖早期语言优先级、五语原生消息、无日志时保留原始失败、超长多字节/控制字符、日志替换以及平台构造失败的诊断映射。
- `i18n_resources` 7 项、`i18n_source` 3 项全部通过。源码门禁增加 `set_title`、`set_description`、`rfd::MessageButtons::OkCustom`，并测试这些原生入口会拒绝硬编码文案。
- 全工作区、全目标、全特性 Clippy（`-D warnings`）通过；脚本语法检查和差分检查通过。
- Windows debug 构建通过（15.83 秒）、Release 构建通过（2 分 03 秒）。两个 PE 的 Subsystem 均为 2 / Windows GUI。

使用现有 `scripts/cargo.py +stable` 构建入口和固定补丁缓存，`Cargo.lock` 锁定新增依赖。下表保留首次实现阶段的产物；最新日志入口版本及验证结果见末节。

| 产物 | 字节数 | 生成时间 | SHA-256 |
| --- | --- | --- | --- |
| `target/debug/pomelo.exe` | 47,785,472 | 2026-10-03 12:57:31 | `F559F5EBE8A526A967A3A81F6D735244524797A598C56F39A304926394B370D4` |
| `target/release/pomelo.exe` | 35,486,208 | 2026-10-03 12:55:52 | `773292DD5CD71A1700A3A3C45A7E14A45677C6296C9B80544DFEDDAD4719873E` |

## 实际桌面证据

`scripts/validate-startup-errors.ps1 -Locale en -Scenario unsupported-encoding` 创建私有配置与核验过的 Release 副本，参数为 `--encoding unsupported-test`，不传 `--locale`，验证早期读取已保存的英文偏好。

实际桌面进程 61348，私有目录 `.cache/ui-validation/startup/f5616e5308fc4f0d8a7f7dac36139739/`。窗口清单新增原生 `Pomelo could not start` 对话框，没有新增 PowerShell/终端。截图和原生可访问树确认：原因包含不支持的编码及五种有效编码、英文技术详情标签、`CLI_UNSUPPORTED_ENCODING`、完整日志路径和自定义 `Close`。系统标题栏关闭控件仍由 Windows 系统语言提供。

原始截图保存在 `.cache/ui-validation/startup/screenshots/en.jpg`，窗口为 559×233，未修改。长日志路径在屏幕中由 Windows 控件做省略显示，可访问树保留完整路径；长路径可用性还需继续检查。日志实际存在，locale 为 en，错误码与原因匹配；该失败没有创建欢迎页或导入文件。

尝试用 Enter 关闭时，Computer Use 检测到用户输入，拒绝注入并要求重新观察，随后停止自动输入。之后该进程已结束，验证脚本返回成功，私有 `result.json` 实际记录 ExitCode 为 2；这证明参数失败退出码，不构成自动 Enter/Escape 关闭验证。日常 `language.json/theme.json/recent.json/views.json` 四项 SHA-256 与原基线全部相同；未更改 BRD 源文件。

## 复验入口与未完成范围

该脚本专门制造启动错误、打开原生反馈并记录退出码，不是日常启动方式。其他语言和分支可分别执行：

```powershell
./scripts/validate-startup-errors.ps1 -Locale zh-CN
./scripts/validate-startup-errors.ps1 -Locale zh-TW -Scenario missing-value
./scripts/validate-startup-errors.ps1 -Locale ja -Scenario unsupported-locale
./scripts/validate-startup-errors.ps1 -Locale ko -Scenario unsupported-encoding
./scripts/validate-startup-errors.ps1 -Locale zh-CN -OverrideLocale ja
./scripts/validate-startup-errors.ps1 -Locale zh-CN -OverrideLocale ja -Scenario log-unavailable
```

以上记录对应首次实现阶段。五语、语言覆盖、不可写日志和关闭行为已由下节补验；真实 DirectX/驱动故障尚未制造，不能宣告 GPU 初始化失败硬件验收通过。本实现不处理 abort、OOM、访问冲突或运行中 GPU 故障；完整 Windows MVP 和发布验收未完成。

## 日志入口与同版五语补验

长日志路径会被 Windows 原生控件省略，因此在成功写入日志时增加本地化的“打开日志文件夹”按钮。默认按钮仍为“关闭”，Enter 关闭、Escape 取消均不会打开目录。日志不可保存时不显示无效入口。应用直接以单个路径参数启动 Windows 文件资源管理器，不经 PowerShell；启动失败会显示五语诊断 `APP_LOG_FOLDER_OPEN_FAILED`，保留原始错误日志。打开目录不重试应用初始化，也不改变原始参数失败退出码。

新增两条消息同步五语；应用新增一项回归，覆盖准确目录、失败诊断重译、相对路径拒绝及原日志保留。应用 **60 项**、i18n **10 项**、工作区全目标全特性 Clippy、格式和脚本语法检查均通过。Windows debug 构建通过（14.79 秒）。独立目标目录的 Release 完整构建通过（2 分 47 秒），用于下表全部实窗；各私有副本 SHA-256 均为 `2345D5D9C839DFD8FEB23D5740EB21F6FCCF1C8FA59222FC99BA5D4BDD59DBAA`。

默认 `target/release/pomelo.exe` 被用户正在运行的进程 29876 锁定，常规 Release 更新返回“failed to remove file / 拒绝访问”。未关闭该进程、未替换正在使用的 exe。新版 Release 位于 `.cache/ui-validation/startup-build/release/pomelo.exe`；默认路径仍保留首次启动错误版本。三份现有产物的 PE 子系统均为 **2 / Windows GUI**。

| 产物 | 字节数 | 生成时间 | SHA-256 |
| --- | --- | --- | --- |
| 当前 `target/debug/pomelo.exe` | 47,798,272 | 2026-10-03 13:09:56 | `8702EF83F0222B68C68AB47A7AA8CB050A7C28126119FA813F2ECCD1E5914B94` |
| 新版独立 Release | 35,495,424 | 2026-10-03 13:13:05 | `2345D5D9C839DFD8FEB23D5740EB21F6FCCF1C8FA59222FC99BA5D4BDD59DBAA` |
| 被运行实例锁定的默认 Release | 35,486,208 | 2026-10-03 12:55:52 | `773292DD5CD71A1700A3A3C45A7E14A45677C6296C9B80544DFEDDAD4719873E` |

所有下面的调用增加 `-TargetDirectory .cache/ui-validation/startup-build`。配置目录均位于 `.cache/ui-validation/startup/<profile>/`，真实退出码由验证脚本等候进程结束后写入 `result.json`，不是从源码推断。

| 实际 locale / 情况 | Profile | 进程 | 操作与结果 |
| --- | --- | --- | --- |
| 简中 / 无效选项 | `5743ba0a81a8420ca1963f12732397b4` | 27592 | 五语对应的原因/技术标签/按钮；Tab 到日志入口，Space 打开准确日志目录；退出码 2 |
| 繁中 / 编码缺值 | `823c75bec1c6443db431eb745ace06c5` | 7628 | 原因包含 `--encoding`，Escape 关闭且未打开目录；退出码 2 |
| 日文 / 不支持的语言参数 | `ffe4d013b6ee40d895b686d7ad39cf32` | 30112 | 无效 `--locale` 安全使用已保存日文，说明及两按钮完整；Enter 默认关闭；退出码 2 |
| 韩文 / 不支持的编码 | `673fa3fea3204466a08abba592109de2` | 13976 | 原因换行、命令标签完整；点击原生标题栏关闭；退出码 2 |
| 日文显式覆盖简中 | `54ba0aa628bf46b3aadf01f9ad5a5cbf` | 40956 | `--locale ja` 令全部应用文案和日志使用日文，保存文件仍为 zh-CN；Escape 关闭；退出码 2 |
| 日文 / 日志不可写 | `cb4370c5e4494b0f899c9cec281e587e` | 20716 | 指定根为阻塞文件；原始原因、技术码和日文日志不可保存提示均保留，只有关闭按钮；Enter 结束；退出码 2 |
| 英文 / 不支持的编码 | `236168fbaf904d44b34beba37377266c` | 43288 | 新版英文原因、技术标签、日志及两按钮完整；点击自定义 Close；退出码 2 |
| 简中 / 超长参数 | `512f3f745bdc492db00d8010b8ddc892` | 35564 | 参数含 800 个汉字，主要原因按 UTF-8 字节限额截断并正常换行，559×313 窗口两按钮可见；Enter 关闭；退出码 2 |

简中日志入口实际打开新的资源管理器窗口 55906236。原生树的路径面包屑包含上述 profile 和 `logs`，文件列表只有 `startup-error.log`，与磁盘文件相符；该窗口随后用 Ctrl+W 正常关闭并从清单移除。八个错误进程均已结束。阻塞配置文件仍为原始 `preserve`，没有生成 `blocked-config/logs`；超长参数日志为 2,547 字节，包含完整源参数。

上述未经编辑的窗口截图保存于 `.cache/ui-validation/startup/screenshots/`：`zh-CN-log-action.jpg`、`zh-TW.jpg`、`ja.jpg`、`ko.jpg`、`ja-override.jpg`、`log-unavailable.jpg`、`en-log-action.jpg`、`long-option.jpg`、`log-folder.jpg`。普通错误窗口宽 559，高约 200–233；超长简中为 559×313。此尺寸证据不代表 DPI 矩阵已完成。

## 正常启动回归与剩余项

同一新版独立 Release 通过正常启动脚本、私有配置 `profiles/4cbf45e3e08f45cca366ddd292d91d84/` 打开 `E:/brd_cases/USBC_FPC.brd`，实际进程 29704。简中深色 1440×920 工作区成功显示真实板 GPU 画面、文档标签、左右面板及悬浮工具栏；文件信息为 3 个铜层、2,958 条线段、94 个引脚、119 个过孔、176 个铜皮，诊断 0 条。保存原始截图 `normal-board.jpg`，正常配置没有创建启动错误日志。

随后尝试 Ctrl+W 时检测到用户输入，停止自动输入，保留用户正在操作的私有窗口。关闭文档返回欢迎页、正常退出码、重启及完整主流程仍待完成，不能把成功显示画面扩大为这些操作通过。

日常四份配置与本轮启动前的独立哈希基线全部相同。USBC 源文件 SHA-256 仍为 `550CF739AF80C2B5B18A4DE91419ADF2ECD1133CC2644EB0149D21B71E52B44B`，与既有案例基线相同。所有模拟错误和日志仅使用工作区的私有目录，未更改源板。

尚需补齐：真实 GPU/平台/窗口创建故障与退出码 1、文件夹启动失败的原生反馈、其他语言超长文本、中文/空格/极长日志目录、DPI、正常完整主流程，以及用户运行实例退出后的默认 Release 更新。当前五语参数失败与日志入口实窗范围已经通过，完整 Windows MVP、全语言全流程和发布验收仍未完成。
