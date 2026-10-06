# UI 验证配置目录

## 配置规则

`POMELO_CONFIG_DIR` 指定独立的应用配置根目录，必须为绝对路径。语言、主题、最近文件、查看状态及面板偏好共用此根目录；不额外追加应用目录。

显式空值或相对路径应报告配置目录错误，不能回退写入日常配置。覆盖变量不改变源文件位置、编码或渲染后端。

## Windows 启动示例

从仓库根目录的 PowerShell 启动独立验证实例：

```powershell
cargo build -p pomelo --locked
$validationRoot = Join-Path ([IO.Path]::GetTempPath()) ('pomelo-validation-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $validationRoot | Out-Null
$previousConfigDir = $env:POMELO_CONFIG_DIR
try {
    $env:POMELO_CONFIG_DIR = $validationRoot
    & .\target\debug\pomelo.exe --locale zh-CN
} finally {
    $env:POMELO_CONFIG_DIR = $previousConfigDir
}
```

其他平台同样通过环境变量指定绝对目录，并使用对应构建产物。空配置用于首次启动验证；多实例分别使用不同目录。不要复制实际历史或设计缩略图到公开测试资源中。

## 检查顺序

1. 确认使用预期构建产物，并在实际桌面看到窗口。
2. 检查语言、主题、菜单、键盘导航及焦点。
3. 用合成样本检查打开、多文档、视图修改和重载。
4. 正常关闭，再用相同目录启动，检查状态恢复。
5. 检查失败诊断和日常配置是否保持原样。

进程启动成功不代表界面验收通过。旧实例不会随重新构建自动升级，应重新启动待验版本。验证目录可能保存本地路径和缩略图，应留在私有环境中。
