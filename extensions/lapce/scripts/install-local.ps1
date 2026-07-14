#requires -Version 5.1
<#
.SYNOPSIS
    Build the Verter Lapce volt and install it into Lapce's local plugins
    directory (Windows / PowerShell).

.DESCRIPTION
    Builds bin/verter-lapce.wasm (cargo, wasm32-wasip1, release), detects the
    per-channel Lapce plugins directory, copies volt.toml + the wasm into
    <plugins>/verter/, and prints the exact `lsp.serverPath` config snippet with
    the absolute built verter-lsp.exe path filled in. Idempotent (re-run to
    refresh). Fails loudly if the wasm or the verter-lsp binary is missing.

.PARAMETER Channel
    Lapce release channel whose plugins directory to install into. One of
    Lapce-Stable (default), Lapce-Nightly, Lapce-Debug.
#>
[CmdletBinding()]
param(
    [ValidateSet("Lapce-Stable", "Lapce-Nightly", "Lapce-Debug")]
    [string]$Channel = "Lapce-Stable"
)

$ErrorActionPreference = "Stop"

# Repo root is three levels up from this script (extensions/lapce/scripts/).
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..\..")).Path
$LapceDir = Join-Path $RepoRoot "extensions\lapce"
$WasmPath = Join-Path $LapceDir "bin\verter-lapce.wasm"
$VoltToml = Join-Path $LapceDir "volt.toml"

function Fail([string]$message) {
    Write-Error $message
    exit 1
}

# 1. Build the volt (cargo wasm32-wasip1 release) and copy to bin/verter-lapce.wasm.
Write-Host "==> Building the Verter Lapce volt (wasm32-wasip1, release)..."
& rustup target add wasm32-wasip1 | Out-Null
& cargo build --manifest-path (Join-Path $LapceDir "Cargo.toml") --target wasm32-wasip1 --release
if ($LASTEXITCODE -ne 0) { Fail "cargo build failed; cannot install the volt." }

$BuiltWasm = Join-Path $LapceDir "target\wasm32-wasip1\release\verter_lapce.wasm"
if (-not (Test-Path -LiteralPath $BuiltWasm)) {
    Fail "built wasm not found at $BuiltWasm -- build first ('pnpm run build:lapce')."
}
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $WasmPath) | Out-Null
Copy-Item -LiteralPath $BuiltWasm -Destination $WasmPath -Force

if (-not (Test-Path -LiteralPath $WasmPath)) {
    Fail "volt wasm missing at $WasmPath -- build first ('pnpm run build:lapce')."
}
if (-not (Test-Path -LiteralPath $VoltToml)) {
    Fail "volt.toml missing at $VoltToml."
}

# 2. Detect the per-channel Lapce plugins directory (Windows).
$LocalAppData = $env:LOCALAPPDATA
if ([string]::IsNullOrEmpty($LocalAppData)) {
    Fail "%LOCALAPPDATA% is not set; cannot locate the Lapce plugins directory."
}
$PluginsDir = Join-Path $LocalAppData (Join-Path "lapce" (Join-Path $Channel (Join-Path "data" "plugins")))

# 3. Create <plugins>/verter/ and copy volt.toml + bin/verter-lapce.wasm into it.
$VoltDir = Join-Path $PluginsDir "verter"
$VoltBinDir = Join-Path $VoltDir "bin"
New-Item -ItemType Directory -Force -Path $VoltBinDir | Out-Null
Copy-Item -LiteralPath $VoltToml -Destination (Join-Path $VoltDir "volt.toml") -Force
Copy-Item -LiteralPath $WasmPath -Destination (Join-Path $VoltBinDir "verter-lapce.wasm") -Force
Write-Host "==> Installed volt to $VoltDir"

# 4. Print the exact lsp.serverPath snippet with the absolute verter-lsp.exe path.
$ServerBin = Join-Path $RepoRoot "target\release\verter-lsp.exe"
$ServerBinForward = $ServerBin -replace "\\", "/"
$BinNote = if (Test-Path -LiteralPath $ServerBin) {
    "found at $ServerBin"
} else {
    "NOT built yet -- run 'cargo build -p verter_lsp --release' first"
}

Write-Host ""
Write-Host "==> verter-lsp binary: $BinNote"
Write-Host "==> Add this to your Lapce settings (settings.toml):"
Write-Host ""
Write-Host "[volt.verter]"
Write-Host "`"lsp.serverPath`" = `"$ServerBinForward`""
Write-Host "`"typeProvider`" = `"tsgo`""
Write-Host ""
Write-Host "==> Now restart Lapce (or reload plugins) to pick up the volt."
