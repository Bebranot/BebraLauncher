# Publishes a launcher release on GitHub: builds the installer, signs it, creates the release
# (its text is the changelog section of this version) and uploads the installer and its signature.
# Launchers find it on their "Updates" tab.
#
#   $env:GITHUB_TOKEN = '<token with write access to Contents of the repository>'
#   pwsh scripts/release.ps1                 # or: powershell -File scripts/release.ps1
#   pwsh scripts/release.ps1 -DryRun         # everything except talking to GitHub
#
# Needs: the signing key (made once with `bebra update-keygen`; default location
# %USERPROFILE%\.bebra\update-signing.key, or $env:BEBRA_UPDATE_KEY), a clean pushed main,
# and what scripts/build-release.ps1 needs. Rules: docs/CHANGELOG_RULES.md.

param(
    [switch]$DryRun,
    [switch]$SkipTests,
    [string]$Repo = 'Bebranot/BebraLauncher',
    # Goes into the release title only ("BebraLauncher 1.0.000 Beta"). The release is never marked
    # as a GitHub pre-release: launchers skip those and /releases/latest does not point at them.
    [string]$Label = ''
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

function Fail($message) { throw "release: $message" }

# ---- the version: Cargo.toml holds a.b.patch, the release is named a.b.cdd (patch has 3 digits)
$cargo = Get-Content "$root/Cargo.toml" -Raw
if ($cargo -notmatch '(?ms)\[workspace\.package\].*?^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"') {
    Fail 'no version in Cargo.toml'
}
$version = '{0}.{1}.{2:000}' -f $Matches[1], $Matches[2], [int]$Matches[3]
$cargoVersion = "$($Matches[1]).$($Matches[2]).$([int]$Matches[3])"
$tag = "v$version"
Write-Host "==> Release $version (tag $tag, Cargo $cargoVersion)"

# ---- the changelog must have this version, and its text becomes the release text
$changelog = Get-Content "$root/CHANGELOG.md" -Raw -Encoding UTF8

# The section above all version sections is the unreleased one: a "## [...]" heading whose title
# is not a version number. If it still has entries, the version was not bumped: publishing now
# would either reuse an existing tag with old text or ship a release that silently drops those
# entries (docs/CHANGELOG_RULES.md).
$unreleased = [regex]::Match($changelog, '(?ms)^##\s*\[(?!\d+\.\d+)\p{L}[^\]]*\]\s*\r?\n(.*?)(?=^##\s*\[|\z)')
if ($unreleased.Success) {
    $pending = @($unreleased.Groups[1].Value -split '\r?\n' | Where-Object { $_ -match '\S' -and $_ -notmatch '^\s*#{2,3}\s' })
    if ($pending.Count -gt 0) {
        Fail "CHANGELOG.md still has $($pending.Count) line(s) in the unreleased section: rename it to the new version and bump the version first (docs/CHANGELOG_RULES.md)"
    }
}

$escaped = [regex]::Escape($version)
if ($changelog -notmatch "(?ms)^## \[$escaped\][^\r\n]*\r?\n(.*?)(?=^## \[|\z)") {
    Fail "CHANGELOG.md has no entry for $version (docs/CHANGELOG_RULES.md)"
}
$notes = $Matches[1].Trim()
if (-not $notes) { Fail "the changelog entry of $version is empty" }

# ---- git: clean, on main, pushed
$branch = (git rev-parse --abbrev-ref HEAD).Trim()
if ($branch -ne 'main') { Fail "not on main (on $branch)" }
if (git status --porcelain) { Fail 'uncommitted changes: commit them first' }
git fetch origin main --quiet
$head = (git rev-parse HEAD).Trim()
if ($head -ne (git rev-parse origin/main).Trim()) { Fail 'main is not pushed (or origin has newer commits)' }

# ---- the signing key
$keyPath = $env:BEBRA_UPDATE_KEY
if (-not $keyPath) { $keyPath = Join-Path $env:USERPROFILE '.bebra\update-signing.key' }
if (-not (Test-Path $keyPath)) { Fail "no signing key at $keyPath (make one: bebra update-keygen <file>)" }

if (-not $DryRun -and -not $env:GITHUB_TOKEN) { Fail 'set $env:GITHUB_TOKEN first' }

# ---- tests
if (-not $SkipTests) {
    Write-Host '==> Tests'
    cargo test --workspace
    if ($LASTEXITCODE -ne 0) { Fail 'cargo test failed' }
    Push-Location "$root/app"
    pnpm install --frozen-lockfile
    pnpm check
    if ($LASTEXITCODE -ne 0) { Pop-Location; Fail 'pnpm check failed' }
    pnpm test
    if ($LASTEXITCODE -ne 0) { Pop-Location; Fail 'pnpm test failed' }
    Pop-Location
}

# ---- build and sign
Write-Host '==> Installer'
& "$PSScriptRoot/build-release.ps1"
# Exactly one installer for this version, x64, no leftovers from a previous build: signing and
# publishing the wrong file is worse than failing here.
$setups = @(Get-ChildItem "$root/target/release/bundle/nsis/*_${cargoVersion}_x64-setup.exe" -ErrorAction SilentlyContinue)
if ($setups.Count -ne 1) {
    Fail "expected exactly one *_${cargoVersion}_x64-setup.exe in target/release/bundle/nsis, found $($setups.Count)"
}
$setup = $setups[0]

Write-Host '==> Signing'
cargo build -p bebra-cli --release
if ($LASTEXITCODE -ne 0) { Fail 'cannot build the signing tool' }
& "$root/target/release/bebra.exe" update-sign $setup.FullName $version --key $keyPath
if ($LASTEXITCODE -ne 0) { Fail 'signing failed' }
$sig = "$($setup.FullName).sig"

Write-Host "`n--- release text ---`n$notes`n--------------------"
if ($DryRun) {
    Write-Host "==> Dry run: built and signed $($setup.Name) ($([math]::Round($setup.Length / 1MB, 1)) MB); nothing was published."
    return
}

# ---- publish
$headers = @{
    Authorization          = "Bearer $env:GITHUB_TOKEN"
    Accept                 = 'application/vnd.github+json'
    'X-GitHub-Api-Version' = '2022-11-28'
}
# The launcher's "What is new" shows only the changelog groups and bullet items of the text, so the
# plain lines of this header stay on the GitHub page. It must not contain list items.
$header = (Get-Content "$PSScriptRoot/release-page.txt" -Raw -Encoding UTF8).Replace('{setup}', $setup.Name).Replace('{repo}', $Repo)
$title = "BebraLauncher $version"
if ($Label) { $title = "$title $Label" }
$body = @{
    tag_name         = $tag
    target_commitish = $head
    name             = $title
    body             = "$($header.Trim())`n`n$notes"
    draft            = $false
    prerelease       = $false
} | ConvertTo-Json
Write-Host '==> Creating the release'
$release = Invoke-RestMethod -Method Post -Uri "https://api.github.com/repos/$Repo/releases" `
    -Headers $headers -ContentType 'application/json; charset=utf-8' `
    -Body ([System.Text.Encoding]::UTF8.GetBytes($body))

$upload = $release.upload_url -replace '\{.*$', ''
foreach ($file in @($setup.FullName, $sig)) {
    $name = [uri]::EscapeDataString((Split-Path $file -Leaf))
    Write-Host "==> Uploading $name"
    Invoke-RestMethod -Method Post -Uri "${upload}?name=$name" -Headers $headers `
        -ContentType 'application/octet-stream' -InFile $file | Out-Null
}
Write-Host "==> Done: $($release.html_url)"
