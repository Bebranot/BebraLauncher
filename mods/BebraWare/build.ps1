param(
    [string]$GameDir = "C:\LocalDutyAVS\bin\Content.Client",
    [switch]$Install
)

$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$project = Join-Path $here "BebraWare.csproj"

Write-Host "Building BebraWare..."
dotnet build $project -c Release -p:GameDir="$GameDir" -nologo
if ($LASTEXITCODE -ne 0) {
    throw "Build failed (exit $LASTEXITCODE)"
}

$dll = Join-Path $here "bin\Release\net10.0\BebraWare.dll"
if (!(Test-Path -LiteralPath $dll)) {
    throw "Build output not found: $dll"
}

Write-Host "Built: $dll"

if ($Install) {
    $patches = Join-Path $env:LOCALAPPDATA "BebraLauncher\patches"
    New-Item -ItemType Directory -Force -Path $patches | Out-Null
    $target = Join-Path $patches "BebraWare.dll"
    Copy-Item -LiteralPath $dll -Destination $target -Force

    $hash = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant()
    $size = (Get-Item -LiteralPath $target).Length
    $manifestPath = Join-Path $patches "manifest.json"
    $patchesMap = [ordered]@{}

    if (Test-Path -LiteralPath $manifestPath) {
        try {
            $existing = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
            if ($existing.patches) {
                foreach ($property in $existing.patches.PSObject.Properties) {
                    $patchesMap[$property.Name] = [ordered]@{
                        sha256 = [string]$property.Value.sha256
                        size = [int64]$property.Value.size
                    }
                }
            }
        }
        catch {
            Write-Host "Existing manifest is unreadable, rewriting it."
        }
    }

    $patchesMap["BebraWare.dll"] = [ordered]@{ sha256 = $hash; size = $size }
    $json = @{ patches = $patchesMap } | ConvertTo-Json -Depth 5
    [IO.File]::WriteAllText($manifestPath, $json, [Text.UTF8Encoding]::new($false))
    Write-Host "Installed to $target"
    Write-Host "Enable BebraWare on the launcher's Mods tab."
}
