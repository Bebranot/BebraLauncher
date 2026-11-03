param(
    [string]$GameDir = "C:\LocalDutyAVS\bin\Content.Client",
    [switch]$Install
)

$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$project = Join-Path $here "NoBlackBars.csproj"

Write-Host "Building NoBlackBars..."
dotnet build $project -c Release -p:GameDir="$GameDir" -nologo
if ($LASTEXITCODE -ne 0) {
    throw "Build failed (exit $LASTEXITCODE)"
}

$dll = Join-Path $here "bin\Release\net10.0\NoBlackBars.dll"
if (!(Test-Path $dll)) {
    throw "Build output not found: $dll"
}

Write-Host "Built: $dll"

if ($Install) {
    $patches = Join-Path $env:LOCALAPPDATA "BebraLauncher\patches"
    New-Item -ItemType Directory -Force $patches | Out-Null
    $target = Join-Path $patches "NoBlackBars.dll"
    Copy-Item $dll $target -Force

    # The launcher loads only files whose SHA-256 matches manifest.json, so refresh the record.
    $hash = (Get-FileHash $target -Algorithm SHA256).Hash
    $size = (Get-Item $target).Length
    $manifestPath = Join-Path $patches "manifest.json"
    $patchesMap = @{}
    if (Test-Path $manifestPath) {
        try {
            $existing = Get-Content $manifestPath -Raw | ConvertFrom-Json
            if ($existing.patches) {
                foreach ($prop in $existing.patches.PSObject.Properties) {
                    $patchesMap[$prop.Name] = @{ sha256 = $prop.Value.sha256; size = $prop.Value.size }
                }
            }
        } catch {
            Write-Host "Existing manifest is unreadable, rewriting it."
        }
    }
    $patchesMap["NoBlackBars.dll"] = @{ sha256 = $hash; size = $size }
    $json = @{ patches = $patchesMap } | ConvertTo-Json -Depth 5
    [System.IO.File]::WriteAllText($manifestPath, $json, (New-Object System.Text.UTF8Encoding($false)))

    Write-Host "Installed to $target"
    Write-Host "Enable the mod on the Mods tab in the launcher."
}
