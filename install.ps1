<#
.SYNOPSIS
Builds Workman in release mode and installs the daemon, CLI, and desktop app for
the current user. The Windows counterpart of install.sh.

.DESCRIPTION
Builds the Svelte frontend and the three release binaries, then installs
wrk.exe, workmand.exe, and workman-desktop.exe under
%LOCALAPPDATA%\Programs\Workman\bin. The directory is added to the user PATH
unless -NoPath is given. Re-run after pulling updates. Running daemons are never
stopped: a binary that is currently running is renamed aside and the updater
removes the retired copy once its process exits. A `workman.exe` convenience
copy of wrk.exe is installed unless WORKMAN_INSTALL_ALIAS=0.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File install.ps1
#>
[CmdletBinding()]
param(
    [switch]$NoPath,
    [switch]$SkipFrontend
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $MyInvocation.MyCommand.Path

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    # A shell opened before rustup finished installing has the old PATH.
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    if (Test-Path (Join-Path $cargoBin 'cargo.exe')) {
        $env:Path = "$cargoBin;$env:Path"
    }
    else {
        throw 'cargo was not found. Install Rust from https://rustup.rs and the Visual Studio Build Tools C++ workload, then re-run.'
    }
}

if (-not $SkipFrontend) {
    if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
        throw 'npm was not found on PATH. Install Node.js (https://nodejs.org), or pass -SkipFrontend if apps/desktop/dist is already built.'
    }
    Write-Host '==> Building desktop frontend'
    Push-Location (Join-Path $repo 'apps\desktop')
    try {
        npm install --no-fund --no-audit
        if ($LASTEXITCODE -ne 0) { throw 'npm install failed' }
        npm run build
        if ($LASTEXITCODE -ne 0) { throw 'frontend build failed' }
    }
    finally { Pop-Location }
}

Write-Host '==> Building release binaries'
Push-Location $repo
try {
    cargo build --release -p workman-cli -p workmand -p workman-desktop
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
}
finally { Pop-Location }

$binDir = Join-Path $env:LOCALAPPDATA 'Programs\Workman\bin'
New-Item -ItemType Directory -Force $binDir | Out-Null

function Install-Binary([string]$name) {
    $source = Join-Path $repo "target\release\$name"
    $target = Join-Path $binDir $name
    try {
        Copy-Item $source $target -Force
    }
    catch {
        # A running binary cannot be overwritten, but it can be renamed aside.
        # The self-updater removes retired copies once their processes exit.
        $retired = Join-Path $binDir (".{0}.workman-update-retired-{1}-0" -f $name, $PID)
        Move-Item $target $retired -Force
        Copy-Item $source $target -Force
    }
    Write-Host "  installed $target"
}

Write-Host '==> Installing'
Install-Binary 'wrk.exe'
Install-Binary 'workmand.exe'
Install-Binary 'workman-desktop.exe'
if ($env:WORKMAN_INSTALL_ALIAS -ne '0') {
    try { Copy-Item (Join-Path $binDir 'wrk.exe') (Join-Path $binDir 'workman.exe') -Force }
    catch { Write-Host '  skipped workman.exe alias (file busy)' }
}

if (-not $NoPath) {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not $userPath) { $userPath = '' }
    if (($userPath -split ';') -notcontains $binDir) {
        [Environment]::SetEnvironmentVariable('Path', ($userPath.TrimEnd(';') + ';' + $binDir).TrimStart(';'), 'User')
        Write-Host "  added $binDir to the user PATH; new terminals pick it up"
    }
}

Write-Host ''
Write-Host 'Done. Run `wrk` in any project directory, `wrk app` for the desktop'
Write-Host 'workspace, or `wrk mcp-setup` for Claude Code.'
