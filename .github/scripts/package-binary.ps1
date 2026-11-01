#requires -Version 7.0
param(
    [switch]$SkipBuild,
    [Nullable[long]]$SourceDateEpoch
)

$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
Push-Location -LiteralPath $projectRoot
try {
    if ($null -eq $SourceDateEpoch) {
        $epoch = & git show -s --format=%ct HEAD
        if ($LASTEXITCODE -ne 0 -or $epoch -notmatch '^\d+$') { throw '无法读取提交时间，请从 Git 检出构建，或明确传入 SourceDateEpoch' }
        $SourceDateEpoch = [long]$epoch
    }
    $archiveTime = [DateTimeOffset]::FromUnixTimeSeconds([long]$SourceDateEpoch).ToUniversalTime()
    if ($archiveTime.Year -lt 1980 -or $archiveTime.Year -gt 2107) { throw '提交时间超出 ZIP 支持范围' }

    if (-not $SkipBuild) {
        $previousFlags = $env:RUSTFLAGS
        try {
            # 去除 PE 链接时间戳，避免相同源码因构建时间不同而变化。
            if ($env:RUSTFLAGS -notlike '*--no-insert-timestamp*') {
                $env:RUSTFLAGS = "$env:RUSTFLAGS -C link-arg=-Wl,--no-insert-timestamp".Trim()
            }
            & ./scripts/build.ps1 -Action release
        } finally { $env:RUSTFLAGS = $previousFlags }
    }

    $packageName = 'opensesame-windows-amd64'
    $payload = [ordered]@{
        'OpenSesame.exe' = 'dist/OpenSesame.exe'
        'README.md' = 'docs/binary.md'
        'licenses/fonts.txt' = 'assets/fonts/LICENSE.txt'
    }
    foreach ($source in $payload.Values) {
        if (-not (Test-Path -LiteralPath $source -PathType Leaf) -or (Get-Item -LiteralPath $source).Length -eq 0) {
            throw "缺少打包文件：$source"
        }
    }
    $output = Join-Path $projectRoot 'dist/release'
    New-Item -ItemType Directory -Path $output -Force | Out-Null
    $archivePath = Join-Path $output "$packageName.zip"
    $temporaryPath = Join-Path $output "$([guid]::NewGuid()).zip.tmp"
    try {
        $stream = [IO.File]::Create($temporaryPath)
        try {
            $zip = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create, $true)
            try {
                # 固定条目顺序与时间；压缩包只包含明确列出的发行文件。
                foreach ($name in $payload.Keys) {
                    $entry = $zip.CreateEntry("$packageName/$name", [IO.Compression.CompressionLevel]::Optimal)
                    $entry.LastWriteTime = $archiveTime
                    $inputStream = [IO.File]::OpenRead((Join-Path $projectRoot $payload[$name]))
                    $entryStream = $entry.Open()
                    try { $inputStream.CopyTo($entryStream) }
                    finally { $entryStream.Dispose(); $inputStream.Dispose() }
                }
            } finally { $zip.Dispose() }
        } finally { $stream.Dispose() }
        [IO.File]::Move($temporaryPath, $archivePath, $true)
    } finally {
        if (Test-Path -LiteralPath $temporaryPath) { Remove-Item -LiteralPath $temporaryPath -Force }
    }
    $hash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText("$archivePath.sha256", "$hash  $packageName.zip`n", [Text.UTF8Encoding]::new($false))
    Get-Item -LiteralPath $archivePath, "$archivePath.sha256" | Select-Object Name, Length
} finally { Pop-Location }
