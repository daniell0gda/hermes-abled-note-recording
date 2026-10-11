# Builds dist\ListeningApp\ListeningApp.exe: one folder, no console window.
# Usage (from the project folder):  .\build.ps1
#   -DistRoot .gen\dist  builds into that folder instead, leaving dist\ListeningApp alone.
param([string]$DistRoot = 'dist')
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$python = Join-Path $root '.venv\Scripts\python.exe'
$build = Join-Path $root 'build'
$dist = Join-Path $root $DistRoot
$app = Join-Path $dist 'ListeningApp'
$elbow = Join-Path $root 'elbow'
$vcvars = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat'

# elbow, the Rust diagram renderer, is bundled as elbow\elbow.exe; cargo needs the MSVC linker environment.
$cargo = "cargo build --release --manifest-path `"$elbow\Cargo.toml`""
if (Test-Path $vcvars) { cmd /s /c "`"$vcvars`" >nul 2>&1 && $cargo" } else { cmd /s /c $cargo }
if ($LASTEXITCODE -ne 0) { throw "Building elbow failed with exit code $LASTEXITCODE" }

& $python -m PyInstaller `
    --noconfirm --clean --onedir --noconsole `
    --name ListeningApp `
    --distpath $dist `
    --workpath $build `
    --specpath $build `
    --add-data "$root\listening_app\assets;listening_app\assets" `
    --add-data "$root\config.example.yaml;." `
    --add-binary "$elbow\target\release\elbow.exe;elbow" `
    --collect-data litellm `
    --copy-metadata litellm `
    --hidden-import tiktoken_ext.openai_public `
    --hidden-import webview.platforms.winforms `
    --hidden-import webview.platforms.edgechromium `
    (Join-Path $root 'listening_app\__main__.py')
if ($LASTEXITCODE -ne 0) { throw "PyInstaller failed with exit code $LASTEXITCODE" }

Copy-Item (Join-Path $root 'config.example.yaml') (Join-Path $app 'config.example.yaml') -Force
Write-Host "Built $(Join-Path $app 'ListeningApp.exe')"
