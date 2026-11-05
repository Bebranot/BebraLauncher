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
# Publish into an empty directory: tauri packs loader/out-sc as the app's `loader` resource, so
# files left from an older build would silently end up in the installer.
$loaderOut = "$root/loader/out-sc"
if (Test-Path $loaderOut) {
    Remove-Item $loaderOut -Recurse -Force
}
# Locked restore: the release must use exactly the dependency graph recorded in
# loader/**/packages.lock.json, not whatever the feed serves today.
dotnet restore loader/SS14.Loader/SS14.Loader.csproj -r win-x64 --locked-mode
if ($LASTEXITCODE -ne 0) { throw 'loader restore failed (packages.lock.json is out of date)' }
# DebugType=None keeps .pdb files out of the installer: they are debug data, not needed to run.
dotnet publish loader/SS14.Loader/SS14.Loader.csproj -c Release -r win-x64 --self-contained true --no-restore `
    -p:PublishSingleFile=false -p:DebugType=None -o $loaderOut
if ($LASTEXITCODE -ne 0) { throw 'loader publish failed' }
if (-not (Test-Path (Join-Path $loaderOut 'SS14.Loader.exe'))) {
    throw "loader publish produced no SS14.Loader.exe in $loaderOut"
}

Write-Host '==> HWID patch'
# A class library the Loader loads into the client, not a program: it must sit next to
# SS14.Loader.exe, because that is where the launcher looks for it. Locked restore keeps the
# shipped binary on the dependency graph recorded in packages.lock.json.
dotnet restore loader/Bebra.HwidPatch/Bebra.HwidPatch.csproj --locked-mode
if ($LASTEXITCODE -ne 0) { throw 'HWID patch restore failed (packages.lock.json is out of date)' }
dotnet publish loader/Bebra.HwidPatch/Bebra.HwidPatch.csproj -c Release --no-restore `
    -p:DebugType=None -o $loaderOut
if ($LASTEXITCODE -ne 0) { throw 'HWID patch publish failed' }
if (-not (Test-Path (Join-Path $loaderOut 'Bebra.HwidPatch.dll'))) {
    throw "HWID patch publish produced no Bebra.HwidPatch.dll in $loaderOut"
}

Write-Host '==> Performance patch'
# Same as the HWID patch: loaded into the client from next to SS14.Loader.exe.
dotnet restore loader/Bebra.PerfPatch/Bebra.PerfPatch.csproj --locked-mode
if ($LASTEXITCODE -ne 0) { throw 'performance patch restore failed (packages.lock.json is out of date)' }
dotnet publish loader/Bebra.PerfPatch/Bebra.PerfPatch.csproj -c Release --no-restore `
    -p:DebugType=None -o $loaderOut
if ($LASTEXITCODE -ne 0) { throw 'performance patch publish failed' }
if (-not (Test-Path (Join-Path $loaderOut 'Bebra.PerfPatch.dll'))) {
    throw "performance patch publish produced no Bebra.PerfPatch.dll in $loaderOut"
}

Write-Host '==> Presence patch'
# Same as the HWID patch: loaded into the client from next to SS14.Loader.exe.
dotnet restore loader/Bebra.PresencePatch/Bebra.PresencePatch.csproj --locked-mode
if ($LASTEXITCODE -ne 0) { throw 'presence patch restore failed (packages.lock.json is out of date)' }
dotnet publish loader/Bebra.PresencePatch/Bebra.PresencePatch.csproj -c Release --no-restore `
    -p:DebugType=None -o $loaderOut
if ($LASTEXITCODE -ne 0) { throw 'presence patch publish failed' }
if (-not (Test-Path (Join-Path $loaderOut 'Bebra.PresencePatch.dll'))) {
    throw "presence patch publish produced no Bebra.PresencePatch.dll in $loaderOut"
}

# Licences of everything shipped inside loader/: tauri packs loader/out-sc as the `loader`
# resource, so the notice file lands next to the Loader in the installer.
$notices = Join-Path $root 'loader/THIRD-PARTY-NOTICES.txt'
if (-not (Test-Path $notices)) { throw "missing $notices" }
Copy-Item $notices -Destination $loaderOut -Force

# Include VC++ CRT runtime DLLs so the client and native libraries (SDL3, OpenAL, etc.)
# run cleanly on fresh Windows installations without requiring separate VC++ redist install.
# The DLLs are taken only from the Visual Studio redistributable (never from System32: that
# copies whatever the build machine happens to have). Missing redist means missing CRT in the
# installer, which breaks the game for users, so it is an error, not a warning.
$vcDirs = @()
if ($env:BEBRA_VCRT_DIR) {
    $vcDirs = @(Get-Item $env:BEBRA_VCRT_DIR -ErrorAction SilentlyContinue)
} else {
    $vcDirs = @(Get-ChildItem "C:\Program Files\Microsoft Visual Studio\*\*\VC\Redist\MSVC\*\x64\Microsoft.VC*.CRT" -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending)
}
if ($vcDirs.Count -eq 0 -or -not $vcDirs[0]) {
    throw 'VC++ CRT redistributable not found (install the "Desktop development with C++" workload or set BEBRA_VCRT_DIR to the x64 CRT folder)'
}
$vcDir = $vcDirs[0].FullName
foreach ($dll in @('vcruntime140.dll', 'msvcp140.dll')) {
    if (-not (Test-Path (Join-Path $vcDir $dll))) {
        throw "VC++ CRT folder $vcDir has no $dll"
    }
}
Write-Host "==> CRT from $vcDir"
Copy-Item "$vcDir\*.dll" -Destination $loaderOut -Force

Write-Host '==> UI dependencies'
Set-Location "$root/app"
pnpm.cmd install --frozen-lockfile
if ($LASTEXITCODE -ne 0) { throw 'pnpm install failed' }

Write-Host '==> App + installer'
# Old bundles are removed first: otherwise an installer from a previous version could be found
# (and later signed/published) instead of the one just built.
$bundleDir = "$root/target/release/bundle"
if (Test-Path $bundleDir) {
    Remove-Item $bundleDir -Recurse -Force
}
pnpm.cmd tauri build --config src-tauri/tauri.release.json
if ($LASTEXITCODE -ne 0) { throw 'tauri build failed' }

$setups = @(Get-ChildItem "$root/target/release/bundle/nsis/*-setup.exe" -ErrorAction SilentlyContinue)
if ($setups.Count -ne 1) {
    throw "expected exactly one installer in target/release/bundle/nsis, found $($setups.Count)"
}
$setup = $setups[0]
Write-Host "==> Done: $($setup.FullName) ($([math]::Round($setup.Length / 1MB, 1)) MB)"
