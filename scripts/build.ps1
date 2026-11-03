param(
    [ValidateSet('check', 'test', 'build', 'release', 'qa', 'fmt', 'lint', 'check-web', 'test-web', 'release-web')]
    [string]$Action = 'release'
)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $projectRoot
$localToolchain = Join-Path $projectRoot '.tools\rustup\toolchains\stable-x86_64-pc-windows-gnu\bin'
if (Test-Path -LiteralPath (Join-Path $localToolchain 'cargo.exe')) {
    $env:CARGO_HOME = Join-Path $projectRoot '.tools\cargo'
    $env:RUSTUP_HOME = Join-Path $projectRoot '.tools\rustup'
    $env:RUSTC = Join-Path $localToolchain 'rustc.exe'
    $env:PATH = "$localToolchain;$env:PATH"
    $cargoCommand = Join-Path $localToolchain 'cargo.exe'
    $mingwBin = Join-Path $projectRoot '.tools\mingw64\bin'
    if (Test-Path -LiteralPath (Join-Path $mingwBin 'gcc.exe')) {
        $env:PATH = "$mingwBin;$env:PATH"
        $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = Join-Path $mingwBin 'gcc.exe'
        $env:CC = Join-Path $mingwBin 'gcc.exe'
        $env:AR = Join-Path $mingwBin 'ar.exe'
    }
} else {
    $cargoCommand = (Get-Command cargo -ErrorAction Stop).Source
}

switch ($Action) {
    'fmt' {
        $formatter = Join-Path $localToolchain 'cargo-fmt.exe'
        if (Test-Path -LiteralPath $formatter) { & $formatter fmt --all -- --check }
        else { & $cargoCommand fmt --all -- --check }
    }
    'lint' {
        $clippy = Join-Path $localToolchain 'cargo-clippy.exe'
        if (Test-Path -LiteralPath $clippy) { & $clippy clippy --locked --all-targets --all-features -- -D warnings }
        else { & $cargoCommand clippy --locked --all-targets --all-features -- -D warnings }
    }
    'check' { & $cargoCommand check --locked }
    'test' { & $cargoCommand test --locked --lib }
    'build' { & $cargoCommand build --locked }
    'qa' { & $cargoCommand build --locked --features qa }
    'check-web' { & $cargoCommand check --locked --no-default-features --features web --bin opensesame-web }
    'test-web' { & $cargoCommand test --locked --no-default-features --features web --lib }
    'release-web' { throw 'Web 发行构建仅支持 Linux amd64，请在 Linux 上运行 bash scripts/build.sh release-web' }
    'release' {
        & $cargoCommand build --locked --release
        if ($LASTEXITCODE -eq 0) {
            New-Item -ItemType Directory -Path 'dist' -Force | Out-Null
            $destination = 'dist\OpenSesame.exe'
            Copy-Item -LiteralPath 'target\release\opensesame.exe' -Destination $destination -Force
            Get-Item -LiteralPath $destination | Select-Object FullName, Length
        }
    }
}
if ($LASTEXITCODE -ne 0) { throw "构建失败，退出码：$LASTEXITCODE" }

