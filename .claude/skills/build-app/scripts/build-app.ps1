# Builds ListeningApp into .gen\dist, then mirrors it into dist\ListeningApp and checks the bundled assets.
# Stops before touching anything while ListeningApp.exe runs (it may be recording a meeting).
# Usage (from anywhere):  & .claude\skills\build-app\scripts\build-app.ps1
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..')).Path
$staged = Join-Path $root '.gen\dist\ListeningApp'
$app = Join-Path $root 'dist\ListeningApp'

$running = Get-Process ListeningApp -ErrorAction SilentlyContinue
if ($running) {
    throw "ListeningApp.exe is running (PID $($running.Id -join ', ')). Close it, then build again."
}

& (Join-Path $root 'build.ps1') -DistRoot '.gen\dist'

robocopy $staged $app /MIR /NFL /NDL /NJH /NP /R:1 /W:1 | Out-Null
if ($LASTEXITCODE -ge 8) { throw "Copying the build into $app failed (robocopy exit code $LASTEXITCODE)" }

$assets = Join-Path $root 'listening_app\assets'
$bundled = Join-Path $app '_internal\listening_app\assets'
$stale = Get-ChildItem $assets -Recurse -File | Where-Object {
    $copy = Join-Path $bundled $_.FullName.Substring($assets.Length + 1)
    -not (Test-Path $copy) -or (Get-FileHash $copy).Hash -ne (Get-FileHash $_.FullName).Hash
}
if ($stale) { throw "Bundled assets differ from the source: $($stale.Name -join ', ')" }
Write-Host "Built $(Join-Path $app 'ListeningApp.exe') - assets match the source"
