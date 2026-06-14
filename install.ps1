# Bobric installer (Windows PowerShell)
#
# Usage:
#   iwr -useb https://raw.githubusercontent.com/sheirla/bobric/main/install.ps1 | iex
#
# Env vars:
#   $env:BOBRIC_VERSION   pin a specific git tag/branch (default: main)
#   $env:BOBRIC_REPO      override the git URL
#
$ErrorActionPreference = 'Stop'

$RepoUrl = if ($env:BOBRIC_REPO) { $env:BOBRIC_REPO } else { 'https://github.com/sheirla/bobric' }
$Version = if ($env:BOBRIC_VERSION) { $env:BOBRIC_VERSION } else { 'main' }
$Binary  = 'bobric.exe'

function Step($msg) { Write-Host "==> $msg" -ForegroundColor Green }
function Warn($msg) { Write-Host "[warn] $msg" -ForegroundColor Yellow }
function Die($msg)  { Write-Host "[err] $msg" -ForegroundColor Red; exit 1 }

Step "bobric installer (target: $RepoUrl @ $Version)"

# 1. Ensure Rust toolchain
$cargo = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $cargo) {
    Step "Rust not found -- installing via rustup"
    $rustupInit = Join-Path $env:TEMP 'rustup-init.exe'
    Invoke-WebRequest -Uri 'https://win.rustup.rs/x86_64' -OutFile $rustupInit -UseBasicParsing
    & $rustupInit -y --default-toolchain stable --profile minimal
    Remove-Item $rustupInit -Force -ErrorAction SilentlyContinue
    # Add cargo to current session PATH
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    $env:CARGO_HOME = "$env:USERPROFILE\.cargo"
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if (-not $cargo) { Die "cargo not on PATH after rustup install" }
}

Step "Rust: $(rustc --version)  cargo: $(cargo --version)"

# 2. Build + install bobric
Step "Building and installing '$Binary' (this can take a few minutes on first run)..."
if ($Version -eq 'main' -or [string]::IsNullOrEmpty($Version)) {
    cargo install --git $RepoUrl --locked
} else {
    cargo install --git $RepoUrl --tag $Version --locked
}

# 3. PATH hint
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
$pathDirs = $env:PATH -split ';'
if ($pathDirs -notcontains $cargoBin) {
    Warn "$cargoBin is not on your PATH"
    Write-Host "  Add it to your user PATH (System Properties -> Environment Variables), or run:"
    Write-Host "  [Environment]::SetEnvironmentVariable('Path',`"$cargoBin;`$env:Path`",'User')"
}

Write-Host ""
Step "Done! Run: $Binary"
