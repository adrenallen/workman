<#
.SYNOPSIS
Side-by-side development install for Windows: wrk-dev, workmand-dev, and
workman-desktop-dev from the current checkout, never touching the release
install. The Windows counterpart of scripts/dev-install.sh.

.DESCRIPTION
Builds the frontend and release binaries, then installs them under the same
%LOCALAPPDATA%\Programs\Workman\bin directory with -dev names. Workman derives
the development identity from the executable name, so the dev stack keeps its
own data directory, daemon discovery, and MCP registration. Re-run after
source changes; running dev binaries are renamed aside like the self-updater.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File scripts/dev-install.ps1
#>
[CmdletBinding()]
param(
    [switch]$SkipFrontend
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    if (Test-Path (Join-Path $cargoBin 'cargo.exe')) { $env:Path = "$cargoBin;$env:Path" }
    else { throw 'cargo was not found. Install Rust from https://rustup.rs, then re-run.' }
}

if (-not $SkipFrontend) {
    Push-Location (Join-Path $repo 'apps\desktop')
    try {
        npm install --no-fund --no-audit
        if ($LASTEXITCODE -ne 0) { throw 'npm install failed' }
        npm run build
        if ($LASTEXITCODE -ne 0) { throw 'frontend build failed' }
    }
    finally { Pop-Location }
}

Write-Host '==> Building the development identity from the current checkout'
Push-Location $repo
try {
    cargo build --release -p workman-cli -p workmand
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
    cargo build --release -p workman-desktop --features tauri/custom-protocol
    if ($LASTEXITCODE -ne 0) { throw 'desktop build failed' }
}
finally { Pop-Location }

$binDir = Join-Path $env:LOCALAPPDATA 'Programs\Workman\bin'
New-Item -ItemType Directory -Force $binDir | Out-Null

function Install-DevBinary([string]$source, [string]$target) {
    $sourcePath = Join-Path $repo "target\release\$source"
    $targetPath = Join-Path $binDir $target
    try {
        Copy-Item $sourcePath $targetPath -Force
    }
    catch {
        # A running dev binary cannot be overwritten, but it can be renamed
        # aside; the self-updater removes retired copies once they exit.
        $retired = Join-Path $binDir (".{0}.workman-update-retired-{1}-0" -f $target, $PID)
        Move-Item $targetPath $retired -Force
        Copy-Item $sourcePath $targetPath -Force
    }
    Write-Host "  installed $targetPath"
}

Write-Host '==> Installing the dev identity'
Install-DevBinary 'wrk.exe' 'wrk-dev.exe'
Install-DevBinary 'workmand.exe' 'workmand-dev.exe'
Install-DevBinary 'workman-desktop.exe' 'workman-desktop-dev.exe'

try {
    $startMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Workman Dev.lnk'
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($startMenu)
    $shortcut.TargetPath = Join-Path $binDir 'workman-desktop-dev.exe'
    $shortcut.WorkingDirectory = $binDir
    $shortcut.Description = 'Workman development identity'
    $shortcut.Save()
    Write-Host '  added the Workman Dev Start Menu entry'
}
catch { Write-Host "  skipped the Start Menu entry: $($_.Exception.Message)" }

Write-Host ''
Write-Host 'Done. Run `wrk-dev` or `wrk-dev app`; the dev identity keeps its own'
Write-Host 'data directory and daemon, so the stable install stays untouched.'
