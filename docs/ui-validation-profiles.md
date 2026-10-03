# UI 验证配置目录

更新：2026-10-03。当前仅 Windows。复验 UI 的语言、主题、历史与查看状态时，用独立配置目录运行正式应用，避免测试切换写入日常偏好。

## 应用规则

`POMELO_CONFIG_DIR` 指定应用配置根目录本身，必须为绝对路径。`services/prefs.rs` 的四个 store 共用此根目录，分别读写 `language.json`、`theme.json`、`recent.json`、`views.json`；不会再次追加 `Pomelo`。未设置变量时，原默认行为不变，Windows 使用 `%APPDATA%\Pomelo`。

显式传入空值或相对路径时，配置根目录不可用，不回退到日常目录写入；沿用共享 i18n 的配置目录诊断。合法但不可读写的绝对目录继续通过各 store 原有本地化诊断报告失败。配置覆盖不改变 BRD 读取位置、文本编码、导入管线或 GPU 后端。

## 启动

日常使用直接双击 `target/release/pomelo.exe` 即可，不需要 PowerShell。Windows debug/Release 入口均使用 GUI 子系统，不创建或附着控制台窗口。下述 PowerShell 脚本仅用于隔离 UI 验证配置。

先构建对应版本，在项目根目录的正常桌面 PowerShell 执行：

```powershell
python -X utf8 scripts/cargo.py +stable build -p pomelo --release --locked --offline
.\scripts\launch-ui-validation.ps1 -Locale ja -Theme light -Configuration release -CopyRecentHistory
```

`Locale` 支持 `en / zh-CN / zh-TW / ja / ko`，`Theme` 支持 `dark / light`，`Configuration` 支持 `debug / release`。默认是英文、深色、debug，脚本不自动构建。每次启动在 `.cache/ui-validation/profiles/<新 GUID>` 创建独立目录，写入 schema 1、UTF-8 无 BOM 的语言/主题配置，并复制当前构建的 `pomelo.exe` 到该目录运行。复制前后检查源文件和副本的 SHA-256，相同才启动；构建尚在改变文件时报告失败。

返回的 `BinaryPath` 是实际运行的副本，`SourceBinaryPath` 是构建产物，`BinarySha256` 标识此次验证版本，另返回进程和配置目录。已运行的验证副本不会锁住下一次 Cargo 输出，也不会随重新构建自动升级；新代码必须重新启动一份副本验证。仅在启动子进程期间设置配置环境变量，随后恢复调用方原值。

`TargetDirectory` 默认 `target`，可接受项目相对路径或绝对路径，必须与构建时的 Cargo `--target-dir` 一致。此前直接启动原构建产物的用户窗口仍在运行时，可以使用另一份标准 Cargo 构建目录，不关闭该窗口：

```powershell
python -X utf8 scripts/cargo.py +stable build -p pomelo --locked --offline --target-dir .cache/ui-validation/build
.\scripts\launch-ui-validation.ps1 -Locale zh-CN -Theme dark -Configuration debug -TargetDirectory .cache/ui-validation/build -CopyRecentHistory
```

独立构建目录首次需要重新编译依赖，后续正常增量构建；不改包名、应用入口、锁文件或 Cargo 构建规则。

`-CopyRecentHistory` 只复制日常目录中的 `recent.json`，供长名称、真实预览和历史列表验证；不复制查看状态，不改写源历史。省略此开关即可验证无历史初次启动，无需清空用户历史：

```powershell
.\scripts\launch-ui-validation.ps1 -Locale zh-CN -Theme dark -Configuration release
```

验证中打开真实 BRD 后，生成的历史、PNG 预览和视图状态均留在对应私有目录。`.cache/` 已被 Git 忽略，其中仍可能包含本地源路径和板缩略图，不作为公共 fixture 或发布资源提交。多个实例也应分别使用不同配置目录。

## 实窗证据与检查

可选 `-BoardPath` 在私有实例启动时走正式命令行打开一个现有本地案例；省略时仍显示欢迎页。脚本使用完整路径并为单一文件参数加引号，不复制或修改板文件：

```powershell
.\scripts\launch-ui-validation.ps1 -Locale zh-CN -Theme dark -Configuration release -BoardPath 'E:\brd_cases\USBC_FPC.brd'
```

进程启动成功不表示界面验证成功。必须按实际 `BinaryPath` 确认副本拥有桌面窗口，再检查截图和交互；在隔离桌面中启动的进程可能不出现在窗口工具的清单中。退出用应用正常关闭流程，不强制终止正在写入状态的窗口。直接运行原构建产物的实例仍会锁住该文件，构建前确认要覆盖的二进制已退出，或使用上述独立构建目录。

2026-10-03 使用最终 Release 实际完成五语深浅功能区、真实历史打开、空历史及首次原生对话框导入。语言、主题、历史和视图写入后，日常四个文件的 SHA-256 均与事前基线一致。两项配置根目录回归覆盖绝对覆盖路径不重复追加子目录，以及无效覆盖路径不回退到日常目录。具体窗口范围和未验项见 [欢迎页布局验证](welcome-layout-validation.md)。
