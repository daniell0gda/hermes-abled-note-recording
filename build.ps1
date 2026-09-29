# Builds dist\ListeningApp.exe: one file, no console window.
# Usage (from the project folder):  .\build.ps1
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$python = Join-Path $root '.venv\Scripts\python.exe'
$build = Join-Path $root 'build'

& $python -m PyInstaller `
    --noconfirm --clean --onefile --noconsole `
    --name ListeningApp `
    --distpath (Join-Path $root 'dist') `
    --workpath $build `
    --specpath $build `
    --add-data "$root\listening_app\assets;listening_app\assets" `
    --add-data "$root\config.example.yaml;." `
    --collect-data litellm `
    --copy-metadata litellm `
    --hidden-import tiktoken_ext.openai_public `
    (Join-Path $root 'listening_app\__main__.py')
if ($LASTEXITCODE -ne 0) { throw "PyInstaller failed with exit code $LASTEXITCODE" }

Copy-Item (Join-Path $root 'config.example.yaml') (Join-Path $root 'dist\config.example.yaml') -Force
Write-Host "Built $(Join-Path $root 'dist\ListeningApp.exe')"
