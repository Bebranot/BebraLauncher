# Builds the Windows installer: BebraLauncher_<version>_x64-setup.exe
#
#   1. publishes the game Loader together with the .NET runtime (no .NET install needed),
#   2. builds the UI and the app,
#   3. packs everything with NSIS (Tauri downloads NSIS on first use).
#
# Needs: Rust, Node + pnpm, .NET 10 SDK. Run from anywhere:  pwsh scripts/build-release.ps1

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

Write-Host '==> Loader (self-contained)'
dotnet publish loader/SS14.Loader/SS14.Loader.csproj -c Release -r win-x64 --self-contained true `
    -p:PublishSingleFile=false -o loader/out-sc
if ($LASTEXITCODE -ne 0) { throw 'loader publish failed' }

Write-Host '==> UI dependencies'
Set-Location "$root/app"
pnpm install --frozen-lockfile
if ($LASTEXITCODE -ne 0) { throw 'pnpm install failed' }

Write-Host '==> App + installer'
pnpm tauri build --config src-tauri/tauri.release.json
if ($LASTEXITCODE -ne 0) { throw 'tauri build failed' }

$setup = Get-ChildItem "$root/target/release/bundle/nsis/*-setup.exe" | Select-Object -First 1
Write-Host "==> Done: $($setup.FullName) ($([math]::Round($setup.Length / 1MB, 1)) MB)"
