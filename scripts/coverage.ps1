<#
.SYNOPSIS
  Coverage gate: 100% line coverage for the pure (platform-free) crates.

.DESCRIPTION
  Reads the crate list from scripts/coverage-crates.txt (one name per line,
  '#' comments) and runs cargo-llvm-cov on exactly those crates with
  --fail-under-lines 100. Exits non-zero when coverage is below 100% or a
  test fails.

  On aarch64-pc-windows-msvc the MSVC linker produces profiles that
  llvm-profdata rejects ("malformed instrumentation profile data: symbol name
  is empty"), so the instrumented build links with rust-lld. This only affects
  the separate llvm-cov target directory.

.PARAMETER Html
  Also write an HTML report to target/llvm-cov/html.

.EXAMPLE
  pwsh scripts/coverage.ps1
#>
param(
    [switch]$Html
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$crates = Get-Content (Join-Path $PSScriptRoot 'coverage-crates.txt') |
    ForEach-Object { $_.Trim() } |
    Where-Object { $_ -and -not $_.StartsWith('#') }

if (-not $crates) {
    Write-Error 'coverage-crates.txt lists no crates'
    exit 1
}

$cargoArgs = @('llvm-cov')
foreach ($c in $crates) { $cargoArgs += @('-p', $c) }
$cargoArgs += @('--fail-under-lines', '100', '--show-missing-lines')

$env:RUSTFLAGS = (@($env:RUSTFLAGS, '-C linker=rust-lld') | Where-Object { $_ }) -join ' '

Write-Host "coverage gate (100% lines): $($crates -join ', ')"
& cargo @cargoArgs
$code = $LASTEXITCODE
if ($code -ne 0) {
    Write-Host "coverage gate FAILED (exit $code)"
    exit $code
}

if ($Html) {
    $htmlArgs = @('llvm-cov', 'report', '--html')
    & cargo @htmlArgs
}
Write-Host 'coverage gate passed'
exit 0
