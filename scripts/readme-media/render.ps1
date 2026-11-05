# Rebuilds docs/media from the live interface. Start `pnpm dev` in app/ first.
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
$out = Resolve-Path '..\..\docs' | Join-Path -ChildPath 'media'
New-Item -ItemType Directory -Force $out | Out-Null

node capture.mjs

foreach ($lang in 'ru', 'en') {
    npx remotion still "hero-$lang" "$out\hero-$lang.png" --scale=1.5
    npx remotion render "themes-$lang" "$out\themes-$lang.gif" --codec=gif
    foreach ($shot in 'servers', 'appearance', 'optimization', 'connecting') {
        Copy-Item "public\shots\$shot-$lang.png" "$out\$shot-$lang.png"
    }
}
# GitHub's social preview: 1280x640, under 1 MB.
npx remotion still 'hero-en' "$out\social-preview.png" --scale=1
