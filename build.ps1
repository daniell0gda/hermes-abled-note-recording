# Builds dist\ListeningApp\ListeningApp.exe: one folder, no console window.
# Usage (from the project folder):  .\build.ps1
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$python = Join-Path $root '.venv\Scripts\python.exe'
$build = Join-Path $root 'build'
$app = Join-Path $root 'dist\ListeningApp'

& $python -m PyInstaller `
    --noconfirm --clean --onedir --noconsole `
    --name ListeningApp `
    --distpath (Join-Path $root 'dist') `
    --workpath $build `
    --specpath $build `
    --add-data "$root\listening_app\assets;listening_app\assets" `
    --add-data "$root\config.example.yaml;." `
    --collect-data litellm `
    --copy-metadata litellm `
    --hidden-import tiktoken_ext.openai_public `
    --hidden-import webview.platforms.winforms `
    --hidden-import webview.platforms.edgechromium `
    (Join-Path $root 'listening_app\__main__.py')
if ($LASTEXITCODE -ne 0) { throw "PyInstaller failed with exit code $LASTEXITCODE" }

Copy-Item (Join-Path $root 'config.example.yaml') (Join-Path $app 'config.example.yaml') -Force
Write-Host "Built $(Join-Path $app 'ListeningApp.exe')"
