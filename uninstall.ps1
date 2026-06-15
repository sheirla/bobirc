# Bobric uninstaller (Windows PowerShell)
#
# Removes the binary AND all per-user data so the machine is left
# clean. Confirm-before-delete unless -Yes is passed.
#
# Usage:
#   iwr -useb https://raw.githubusercontent.com/sheirla/bobirc/main/uninstall.ps1 | iex
#
[CmdletBinding()]
param(
    [switch]$Yes = $false
)

$ErrorActionPreference = 'Stop'

$DataDir  = Join-Path $env:USERPROFILE '.config\bobirc'
$CargoBin = Join-Path $env:USERPROFILE '.cargo\bin\bobirc.exe'

function Step($msg) { Write-Host "==> $msg" -ForegroundColor Green }
function Warn($msg) { Write-Host "[warn] $msg" -ForegroundColor Yellow }
function Die($msg)  { Write-Host "[err] $msg" -ForegroundColor Red; exit 1 }

$targets = @()
if (Test-Path $DataDir)  { $targets += $DataDir }
if (Test-Path $CargoBin) { $targets += $CargoBin }

if (-not $Yes) {
    if ($targets.Count -eq 0) {
        Write-Host "Nothing to remove -- bobirc may already be uninstalled."
        exit 0
    }
    Write-Host "This will REMOVE:"
    foreach ($t in $targets) { Write-Host "  - $t" }
    Write-Host ""
    $ans = Read-Host "Continue? [y/N]"
    if ($ans -notin @('y','Y','yes')) {
        Write-Host "Aborted."
        exit 0
    }
}

$removed = $false
if (Test-Path $DataDir) {
    Remove-Item -LiteralPath $DataDir -Recurse -Force
    Step "removed $DataDir"
    $removed = $true
}
if (Test-Path $CargoBin) {
    Remove-Item -LiteralPath $CargoBin -Force
    Step "removed $CargoBin"
    $removed = $true
}

# Also drop the cargo package registry entry so a future
# `cargo install --git ...` starts from a clean slate. Best-effort.
if (Get-Command cargo -ErrorAction SilentlyContinue) {
    $null = & cargo uninstall bobirc 2>&1
}

if (-not $removed) {
    Warn "nothing to remove"
}

Write-Host ""
Step "Done. bobirc is fully uninstalled."
Step 'Reinstall: iwr -useb https://raw.githubusercontent.com/sheirla/bobirc/main/install.ps1 | iex'
