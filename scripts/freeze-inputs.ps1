param(
    [string]$WebRoot = 'C:\Users\Zen\Desktop\gitrepo\pomelo',
    [string]$Output = '.cache\web-inputs.json'
)
$ErrorActionPreference = 'Stop'
$webPath = (Resolve-Path -LiteralPath $WebRoot).Path
$files = @('package.json', 'bun.lock', 'tsconfig.json') | ForEach-Object { Join-Path $webPath $_ }
foreach ($directory in @('src', 'scripts', 'tests', 'docs')) {
    $files += Get-ChildItem -LiteralPath (Join-Path $webPath $directory) -File -Recurse | Select-Object -ExpandProperty FullName
}
$files += Get-ChildItem -LiteralPath (Join-Path $webPath 'public\fonts\stroke') -File -Recurse | Select-Object -ExpandProperty FullName
$entries = foreach ($file in ($files | Sort-Object -Unique)) {
    $item = Get-Item -LiteralPath $file
    [ordered]@{ path = [IO.Path]::GetRelativePath($webPath, $file).Replace('\', '/'); bytes = $item.Length; sha256 = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() }
}
$outputPath = [IO.Path]::GetFullPath($Output)
[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($outputPath)) | Out-Null
[ordered]@{ schema = 'pomelo-input-manifest-v1'; source = $webPath; files = @($entries) } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $outputPath -Encoding utf8
Write-Output "Frozen $($entries.Count) source entries to $outputPath"
