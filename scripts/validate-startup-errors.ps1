param(
    [ValidateSet('en', 'zh-CN', 'zh-TW', 'ja', 'ko')]
    [string]$Locale = 'zh-CN',
    [ValidateSet('en', 'zh-CN', 'zh-TW', 'ja', 'ko')]
    [string]$OverrideLocale,
    [ValidateSet('invalid-option', 'missing-value', 'unsupported-encoding', 'unsupported-locale', 'log-unavailable', 'long-option')]
    [string]$Scenario = 'invalid-option',
    [ValidateSet('debug', 'release')]
    [string]$Configuration = 'release',
    [string]$TargetDirectory = 'target'
)

# This deliberately opens a startup failure dialog. It is not a daily launcher.
$ErrorActionPreference = 'Stop'
if ($OverrideLocale -and $Scenario -eq 'unsupported-locale') {
    throw 'Unsupported-locale validation uses the saved preference, without an override.'
}
$workspaceDirectory = Split-Path -Parent $PSScriptRoot
$buildDirectory = if ([IO.Path]::IsPathRooted($TargetDirectory)) {
    [IO.Path]::GetFullPath($TargetDirectory)
} else {
    [IO.Path]::GetFullPath((Join-Path $workspaceDirectory $TargetDirectory))
}
$sourceBinary = Join-Path $buildDirectory "$Configuration\pomelo.exe"
if (-not (Test-Path -LiteralPath $sourceBinary -PathType Leaf)) {
    throw "Build $sourceBinary first."
}
$profileDirectory = Join-Path $workspaceDirectory (
    '.cache\ui-validation\startup\' + [guid]::NewGuid().ToString('N')
)
New-Item -ItemType Directory -Path $profileDirectory | Out-Null
$binaryPath = Join-Path $profileDirectory 'pomelo.exe'
$hash = (Get-FileHash -LiteralPath $sourceBinary).Hash
Copy-Item -LiteralPath $sourceBinary -Destination $binaryPath
if ((Get-FileHash -LiteralPath $binaryPath).Hash -ne $hash -or
    (Get-FileHash -LiteralPath $sourceBinary).Hash -ne $hash) {
    throw 'The binary changed during validation setup.'
}
$encoding = [Text.UTF8Encoding]::new($false)
[IO.File]::WriteAllText((Join-Path $profileDirectory 'language.json'),
    (@{ schema_version = 1; language = $Locale } | ConvertTo-Json), $encoding)
$configurationDirectory = $profileDirectory
if ($Scenario -eq 'log-unavailable') {
    $configurationDirectory = Join-Path $profileDirectory 'blocked-config'
    [IO.File]::WriteAllText($configurationDirectory, 'preserve', $encoding)
}
$arguments = @()
if ($OverrideLocale) { $arguments += @('--locale', $OverrideLocale) }
$arguments += switch ($Scenario) {
    'missing-value' { '--encoding' }
    'unsupported-encoding' { @('--encoding', 'unsupported-test') }
    'unsupported-locale' { @('--locale', 'unsupported-test') }
    'long-option' { '--' + ('启动参数' * 200) }
    default { '--startup-validation-invalid' }
}
$previousOverride = [Environment]::GetEnvironmentVariable('POMELO_CONFIG_DIR', 'Process')
try {
    [Environment]::SetEnvironmentVariable('POMELO_CONFIG_DIR', $configurationDirectory, 'Process')
    $process = Start-Process -FilePath $binaryPath -WorkingDirectory $workspaceDirectory `
        -ArgumentList $arguments -PassThru
} finally {
    [Environment]::SetEnvironmentVariable('POMELO_CONFIG_DIR', $previousOverride, 'Process')
}
$record = [ordered]@{
    ProcessId = $process.Id
    BinaryPath = $binaryPath
    SourceBinaryPath = $sourceBinary
    BinarySha256 = $hash
    ConfigDirectory = $configurationDirectory
    Locale = $Locale
    OverrideLocale = $OverrideLocale
    Scenario = $Scenario
    Arguments = $arguments
    Configuration = $Configuration
}
$recordPath = Join-Path $profileDirectory 'result.json'
$record | ConvertTo-Json
[IO.File]::WriteAllText($recordPath, ($record | ConvertTo-Json), $encoding)
$process.WaitForExit()
$record.ExitCode = $process.ExitCode
[IO.File]::WriteAllText($recordPath, ($record | ConvertTo-Json), $encoding)
$record | ConvertTo-Json
if ($process.ExitCode -ne 2) { throw 'Expected startup argument failure exit code 2.' }
