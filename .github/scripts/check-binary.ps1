#requires -Version 7.0
param([switch]$SmokeTest)

$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$executable = Join-Path $projectRoot 'dist/OpenSesame.exe'
$headers = & objdump.exe -p $executable
if ($LASTEXITCODE -ne 0) { throw '无法读取发行程序' }
if (($headers -join "`n") -notmatch 'Subsystem\s+00000002\s+\(Windows GUI\)') { throw '程序没有使用 Windows GUI 子系统' }
$file = [IO.File]::ReadAllBytes($executable)
$peOffset = [BitConverter]::ToInt32($file, 0x3c)
if ([BitConverter]::ToUInt16($file, 0) -ne 0x5a4d -or [BitConverter]::ToUInt32($file, $peOffset) -ne 0x4550 -or
    [BitConverter]::ToUInt16($file, $peOffset + 4) -ne 0x8664) { throw '程序不是有效的 Windows x64 EXE' }
$systemLibraries = @('advapi32.dll', 'bcrypt.dll', 'bcryptprimitives.dll', 'combase.dll', 'comctl32.dll', 'comdlg32.dll', 'crypt32.dll', 'd3d11.dll', 'd3d12.dll',
    'dcomp.dll', 'dwmapi.dll', 'dwrite.dll', 'dxgi.dll', 'gdi32.dll', 'imm32.dll', 'iphlpapi.dll', 'kernel32.dll',
    'msvcrt.dll', 'ntdll.dll', 'ole32.dll', 'oleaut32.dll', 'opengl32.dll', 'powrprof.dll', 'propsys.dll', 'psapi.dll',
    'rpcrt4.dll', 'secur32.dll', 'setupapi.dll', 'shell32.dll', 'shlwapi.dll', 'user32.dll', 'userenv.dll',
    'usp10.dll', 'uxtheme.dll', 'version.dll', 'winmm.dll', 'ws2_32.dll', 'wtsapi32.dll')
foreach ($line in $headers) {
    if ($line -match 'DLL Name:\s*(\S+)') {
        $library = $Matches[1].ToLowerInvariant()
        if ($library -notin $systemLibraries -and $library -notmatch '^(api|ext)-ms-win-[a-z0-9-]+\.dll$') {
            throw "发现额外运行库依赖：$library"
        }
    }
}
if (-not ('OpenSesameReleaseIcons' -as [type])) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class OpenSesameReleaseIcons {
    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    public static extern uint ExtractIconExW(string file, int index, out IntPtr large, out IntPtr small, uint count);
    [DllImport("user32.dll")]
    public static extern bool DestroyIcon(IntPtr icon);
}
'@
}
$large = [IntPtr]::Zero
$small = [IntPtr]::Zero
try {
    $count = [OpenSesameReleaseIcons]::ExtractIconExW($executable, 0, [ref]$large, [ref]$small, 1)
    if ($count -eq 0 -or $large -eq [IntPtr]::Zero -or $small -eq [IntPtr]::Zero) { throw '程序缺少 Windows 图标资源' }
} finally {
    if ($large -ne [IntPtr]::Zero) { [OpenSesameReleaseIcons]::DestroyIcon($large) | Out-Null }
    if ($small -ne [IntPtr]::Zero) { [OpenSesameReleaseIcons]::DestroyIcon($small) | Out-Null }
}
Write-Output '发行程序结构、运行库及图标检查通过'
if (-not $SmokeTest) { return }

$temporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$testRoot = Join-Path $temporaryRoot "opensesame-release-$([guid]::NewGuid())"
$previousAppData = $env:APPDATA
$previousBackend = $env:SLINT_BACKEND
try {
    New-Item -ItemType Directory -Path (Join-Path $testRoot 'working') -Force | Out-Null
    $env:SLINT_BACKEND = 'winit-software'
    foreach ($language in @('zh-CN', 'en')) {
        $env:APPDATA = Join-Path $testRoot $language
        $configuration = Join-Path $env:APPDATA 'OpenSesame'
        New-Item -ItemType Directory -Path $configuration -Force | Out-Null
        @{ version = 1; servers = @(); settings = @{ language = $language; auto_knock = $false; auto_check = $false } } |
            ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $configuration 'config.json') -Encoding utf8NoBOM
        $errorLog = Join-Path $testRoot "$language.stderr.log"
        $process = $null
        try {
            # 隔离配置与工作目录，启动检查不会连接用户配置的服务器。
            $process = Start-Process -FilePath $executable -WorkingDirectory (Join-Path $testRoot 'working') -WindowStyle Hidden -PassThru -RedirectStandardError $errorLog
            Start-Sleep -Seconds 4
            $process.Refresh()
            if ($process.HasExited) { throw "$language 发行程序提前退出：$($process.ExitCode)" }
            if ($process.MainWindowHandle -eq 0) { throw "$language 发行程序没有创建窗口" }
            if ((Get-Item -LiteralPath $errorLog).Length -ne 0) { throw (Get-Content -LiteralPath $errorLog -Raw) }
        } finally {
            if ($null -ne $process) {
                if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
                $process.Dispose()
            }
        }
    }
    Write-Output '中英文发行程序启动检查通过'
} finally {
    $env:APPDATA = $previousAppData
    $env:SLINT_BACKEND = $previousBackend
    # 仅删除本脚本在临时目录中创建的唯一测试目录。
    $resolvedRoot = [IO.Path]::GetFullPath($testRoot)
    if (-not $resolvedRoot.StartsWith($temporaryRoot, [StringComparison]::OrdinalIgnoreCase) -or
        [IO.Path]::GetFileName($resolvedRoot) -notmatch '^opensesame-release-[0-9a-f-]{36}$') { throw '测试目录越界' }
    if (Test-Path -LiteralPath $resolvedRoot) { Remove-Item -LiteralPath $resolvedRoot -Recurse -Force }
}
