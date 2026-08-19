<#
.SYNOPSIS
Builds the Windows release archive the self-updater expects:
release/v<version>/workman-windows-x86_64.zip plus its SHA256.

.DESCRIPTION
Mirrors the Linux archives' layout: bin/wrk.exe, bin/workmand.exe, and
bin/workman-desktop.exe, together with install.ps1 as the human install path.
Builds the frontend and the locked dist-profile binaries from the current
checkout, so run it from the repository root at the tagged release commit.
Archive entries are written with forward slashes explicitly; the updater's
zip reader resolves bin/wrk.exe literally and Compress-Archive would emit
backslash entry names it cannot match.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File scripts/release-windows.ps1
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

$versionLine = Select-String -Path (Join-Path $repo 'Cargo.toml') -Pattern '^version = "(.+)"' | Select-Object -First 1
if (-not $versionLine) { throw 'workspace version was not found in Cargo.toml' }
$version = $versionLine.Matches[0].Groups[1].Value
Write-Host "==> Packaging Workman $version for windows-x86_64"

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

Push-Location $repo
try {
    cargo build --locked --profile dist -p workman-cli -p workmand
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
    cargo build --locked --profile dist -p workman-desktop --features tauri/custom-protocol
    if ($LASTEXITCODE -ne 0) { throw 'desktop build failed' }
}
finally { Pop-Location }

$outputDir = Join-Path $repo "release\v$version"
New-Item -ItemType Directory -Force $outputDir | Out-Null
$archivePath = Join-Path $outputDir 'workman-windows-x86_64.zip'
if (Test-Path $archivePath) { Remove-Item $archivePath -Force }

$distDir = Join-Path $repo 'target\dist'
$entries = @(
    @{ source = Join-Path $distDir 'wrk.exe'; entry = 'bin/wrk.exe' },
    @{ source = Join-Path $distDir 'workmand.exe'; entry = 'bin/workmand.exe' },
    @{ source = Join-Path $distDir 'workman-desktop.exe'; entry = 'bin/workman-desktop.exe' },
    @{ source = Join-Path $repo 'install.ps1'; entry = 'install.ps1' }
)
foreach ($item in $entries) {
    if (-not (Test-Path $item.source)) { throw "release input is missing: $($item.source)" }
}

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::Open($archivePath, [System.IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($item in $entries) {
        [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
            $archive,
            $item.source,
            $item.entry,
            [System.IO.Compression.CompressionLevel]::Optimal
        ) | Out-Null
        Write-Host "  packed $($item.entry)"
    }
}
finally { $archive.Dispose() }

$hash = (Get-FileHash -Algorithm SHA256 $archivePath).Hash.ToLowerInvariant()
$hashLine = "$hash  workman-windows-x86_64.zip"
[System.IO.File]::WriteAllText("$archivePath.sha256", "$hashLine`n", [System.Text.UTF8Encoding]::new($false))

Write-Host ''
Write-Host "Done: $archivePath"
Write-Host "      $hashLine"
