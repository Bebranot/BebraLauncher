# Measures client runs against a running local server and writes one JSON report per run.
# How to read and repeat this: scripts/bench/README.md
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $PlanFile,
    [Parameter(Mandatory)] [string] $OutDir,
    [Parameter(Mandatory)] [string] $Loader,
    [string] $Server = 'ss14://localhost:1212',
    [string] $Bebra = (Join-Path $PSScriptRoot '..\..\target\release\bebra.exe'),
    [string] $DataDir = '',
    [int] $Repeats = 1,
    [int] $Warmup = 75,
    [int] $Seconds = 60,
    [string] $Tour = '',
    [string] $Lobby = '',
    [string] $Setup = '',
    [string] $Anchors = '',
    [int] $Width = 1920,
    [int] $Height = 1080,
    [int] $ClientTimeout = 420
)
$ErrorActionPreference = 'Stop'

# A plan is JSON: { "<run name>": { "profile": "...", "switches": [], "cvars": [], "env": [],
# "affinity": <mask or null>, "vsync": true|false|null } }
$plan = Get-Content $PlanFile -Raw -Encoding UTF8 | ConvertFrom-Json
New-Item -ItemType Directory -Force $OutDir | Out-Null

# What the report proves about each profile: every CVar any profile touches.
$reportCvars = @(
    'light.soft_shadows', 'light.blur', 'light.resolution_scale', 'light.max_shadowcasting_lights',
    'light.ambient_occlusion', 'light.ambient_occlusion_resolution_scale', 'parallax.low_quality',
    'parallax.enabled', 'particles.quality', 'particles.auto_lod_emitters',
    'accessibility.disable_heat_distortion', 'accessibility.disable_singulo_warp',
    'accessibility.disable_rainbow_overlay', 'accessibility.disable_nv_noise',
    'accessibility.disable_ai_static', 'render.tile_edges', 'audio.hrtf', 'audio.tick_rate',
    'audio.default_concurrent', 'ambience.cooldown', 'ambience.max_sounds', 'display.vsync',
    'display.max_fps', 'display.width', 'display.height', 'display.compat',
    'viewport.scale_render', 'net.predict', 'duty.low_end_mode'
) -join ','

function Stop-Clients {
    Get-Process SS14.Loader -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
}

function Get-GpuPercent([int] $ProcessId) {
    # Class names are not localized, unlike the same counters through Get-Counter.
    $engines = Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine `
        -Filter "Name LIKE 'pid_${ProcessId}_%engtype_3D'" -ErrorAction SilentlyContinue
    return ($engines | Measure-Object -Property UtilizationPercentage -Sum).Sum
}

# Round robin (A B C A B C): a slow drift of the server or the machine hits every run alike.
foreach ($repeat in 1..$Repeats) {
    foreach ($name in $plan.PSObject.Properties.Name) {
        $run = $plan.$name
        $report = Join-Path $OutDir ("{0}__{1}.json" -f $name, $repeat)
        if (Test-Path $report) { Write-Host "skip $name #$repeat, report exists"; continue }
        Stop-Clients
        Start-Sleep -Seconds 3

        $arguments = @('launch', $Server, '--guest', '--loader', $Loader, '--profile', $run.profile)
        if ($DataDir) { $arguments += @('--data-dir', $DataDir) }
        foreach ($switch in $run.switches) { $arguments += @('--switch', $switch) }
        foreach ($cvar in $run.cvars) { $arguments += @('--cvar', $cvar) }
        $arguments += @('--cvar', "display.width=$Width", '--cvar', "display.height=$Height")
        # null keeps what the profile set, which is the way a player runs it.
        if ($run.vsync -eq $false) {
            $arguments += @('--cvar', 'display.vsync=false', '--cvar', 'display.max_fps=0')
        }
        foreach ($item in $run.env) { $arguments += @('--env', $item) }
        $arguments += @('--patch', (Join-Path (Split-Path $Loader) 'Bebra.BenchPatch.dll'))

        $env:BEBRA_BENCH_OUT = $report
        $env:BEBRA_BENCH_WARMUP = "$Warmup"
        $env:BEBRA_BENCH_SECONDS = "$Seconds"
        $env:BEBRA_BENCH_TOUR = $Tour
        $env:BEBRA_BENCH_LOBBY = $Lobby
        $env:BEBRA_BENCH_SETUP = $Setup
        $env:BEBRA_BENCH_ANCHORS = $Anchors
        $env:BEBRA_BENCH_CVARS = $reportCvars
        $env:BEBRA_BENCH_TAG = $name

        $stdout = Join-Path $OutDir ("{0}__{1}.launcher.log" -f $name, $repeat)
        Write-Host ("[{0:HH:mm:ss}] run {1} #{2}" -f (Get-Date), $name, $repeat)
        # A "weak PC" run: children inherit the mask from the start, so the .NET runtime and the
        # launcher's core count see only these cores, as they would on such a machine.
        $self = [System.Diagnostics.Process]::GetCurrentProcess()
        $allCores = $self.ProcessorAffinity
        if ($run.affinity) { $self.ProcessorAffinity = [IntPtr][int64]$run.affinity }
        try {
            $launcher = Start-Process -FilePath $Bebra -ArgumentList ($arguments | ForEach-Object { '"{0}"' -f $_ }) `
                -RedirectStandardOutput $stdout -RedirectStandardError "$stdout.err" -PassThru -WindowStyle Hidden
        } finally {
            $self.ProcessorAffinity = $allCores
        }

        $deadline = (Get-Date).AddSeconds($ClientTimeout)
        $gpu = New-Object System.Collections.Generic.List[double]
        $client = $null
        while (-not (Test-Path $report) -and (Get-Date) -lt $deadline -and -not $launcher.HasExited) {
            Start-Sleep -Seconds 5
            if ($null -eq $client -or $client.HasExited) {
                $client = Get-Process SS14.Loader -ErrorAction SilentlyContinue | Select-Object -First 1
            }
            if ($client -and -not $client.HasExited) {
                # Something in the client's window start-up widens the mask again, keep it applied.
                if ($run.affinity) { $client.ProcessorAffinity = [IntPtr][int64]$run.affinity }
                $sample = Get-GpuPercent $client.Id
                if ($sample) { $gpu.Add([double]$sample) }
            }
        }
        if (Test-Path $report) {
            # The last samples are the measured phase; the report keeps the patch's numbers as is.
            $tail = $gpu | Select-Object -Last ([Math]::Ceiling($Seconds / 5))
            $average = ($tail | Measure-Object -Average).Average
            Start-Sleep -Seconds 2
            $json = Get-Content $report -Raw -Encoding UTF8 | ConvertFrom-Json
            $json | Add-Member -NotePropertyName gpu_3d_percent -NotePropertyValue ([Math]::Round([double]$average, 1)) -Force
            $json | ConvertTo-Json -Depth 6 | Set-Content $report -Encoding UTF8
            Write-Host ("    {0} FPS, p99 {1} ms, GPU {2}%" -f $json.fps_avg, $json.frame_ms.p99, $json.gpu_3d_percent)
        } else {
            Write-Warning "no report for $name #$repeat (launcher exited: $($launcher.HasExited))"
        }
        # `quit` normally ends the client; never leave one running into the next run.
        $waitUntil = (Get-Date).AddSeconds(15)
        while (-not $launcher.HasExited -and (Get-Date) -lt $waitUntil) { Start-Sleep -Seconds 1 }
        Stop-Clients
    }
}
