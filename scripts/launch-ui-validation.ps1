param(
    [ValidateSet('en', 'zh-CN', 'zh-TW', 'ja', 'ko')]
    [string]$Locale = 'en',
    [ValidateSet('dark', 'light')]
    [string]$Theme = 'dark',
    [ValidateSet('debug', 'release')]
    [string]$Configuration = 'debug',
    [string]$TargetDirectory = 'target',
    [string]$BoardPath,
    [switch]$CopyRecentHistory
)

$ErrorActionPreference = 'Stop'
$workspaceDirectory = Split-Path -Parent $PSScriptRoot
$resolvedBoardPath = if ($BoardPath) {
    if (-not (Test-Path -LiteralPath $BoardPath -PathType Leaf)) {
        throw 'The validation board must be an existing local file.'
    }
    (Get-Item -LiteralPath $BoardPath).FullName
}
$buildDirectory = if ([IO.Path]::IsPathRooted($TargetDirectory)) {
    [IO.Path]::GetFullPath($TargetDirectory)
} else {
    [IO.Path]::GetFullPath((Join-Path $workspaceDirectory $TargetDirectory))
}
$sourceBinaryPath = Join-Path $buildDirectory "$Configuration\pomelo.exe"
if (-not (Test-Path -LiteralPath $sourceBinaryPath -PathType Leaf)) {
    throw "Build $sourceBinaryPath before launching UI validation."
}

# Each launch owns a fresh profile; theme/language/history/view writes stay here.
$profileDirectory = Join-Path $workspaceDirectory (
    '.cache\ui-validation\profiles\' + [guid]::NewGuid().ToString('N')
)
New-Item -ItemType Directory -Path $profileDirectory | Out-Null
# A running validation instance must not lock Cargo's next output binary.
$binaryPath = Join-Path $profileDirectory 'pomelo.exe'
$sourceBinaryHash = (Get-FileHash -LiteralPath $sourceBinaryPath -Algorithm SHA256).Hash
Copy-Item -LiteralPath $sourceBinaryPath -Destination $binaryPath
if ((Get-FileHash -LiteralPath $binaryPath -Algorithm SHA256).Hash -ne $sourceBinaryHash -or
    (Get-FileHash -LiteralPath $sourceBinaryPath -Algorithm SHA256).Hash -ne $sourceBinaryHash) {
    throw 'The build binary changed while copying it. Finish the build before launching validation.'
}
$profileEncoding = [Text.UTF8Encoding]::new($false)
[IO.File]::WriteAllText(
    (Join-Path $profileDirectory 'theme.json'),
    (@{ schema_version = 1; theme = $Theme } | ConvertTo-Json),
    $profileEncoding
)
[IO.File]::WriteAllText(
    (Join-Path $profileDirectory 'language.json'),
    (@{ schema_version = 1; language = $Locale } | ConvertTo-Json),
    $profileEncoding
)

if ($CopyRecentHistory) {
    $sourceHistory = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'Pomelo\recent.json'
    if (Test-Path -LiteralPath $sourceHistory -PathType Leaf) {
        Copy-Item -LiteralPath $sourceHistory -Destination (Join-Path $profileDirectory 'recent.json')
    }
}

$previousOverride = [Environment]::GetEnvironmentVariable('POMELO_CONFIG_DIR', 'Process')
try {
    [Environment]::SetEnvironmentVariable('POMELO_CONFIG_DIR', $profileDirectory, 'Process')
    $launchParameters = @{
        FilePath = $binaryPath
        WorkingDirectory = $workspaceDirectory
        PassThru = $true
    }
    if ($resolvedBoardPath) {
        # Start-Process joins ArgumentList items. Quote this one literal file path,
        # including whitespace; Windows file names cannot contain a double quote.
        $launchParameters.ArgumentList = '"' + $resolvedBoardPath + '"'
    }
    $validationProcess = Start-Process @launchParameters
} finally {
    [Environment]::SetEnvironmentVariable('POMELO_CONFIG_DIR', $previousOverride, 'Process')
}

[pscustomobject]@{
    ProcessId = $validationProcess.Id
    BinaryPath = $binaryPath
    SourceBinaryPath = $sourceBinaryPath
    BinarySha256 = $sourceBinaryHash
    ConfigDirectory = $profileDirectory
    Locale = $Locale
    Theme = $Theme
    Configuration = $Configuration
}
