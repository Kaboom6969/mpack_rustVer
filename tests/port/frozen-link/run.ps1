param(
    [switch]$Full,
    [switch]$ExpectMissing
)

$ErrorActionPreference = "Stop"

if ($ExpectMissing -and -not $Full) {
    throw "-ExpectMissing requires -Full."
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..\..")).Path
$UpstreamInclude = Join-Path $Root "original_c\mpack-develop\src"
$FrozenUnit = Join-Path $Root "tests\original\test\unit"
$ConfigInclude = Join-Path $Root "tests\port\ffi-harness\include"
$Build = Join-Path $Root "target\frozen-link"
$CargoTarget = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root "target" }
$Cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
$Compiler = if ($env:CC) { $env:CC } else { "C:\Strawberry\c\bin\gcc.exe" }

if (-not (Test-Path $Cargo)) {
    throw "Cargo was not found at $Cargo. Set up Cargo or update this adapter."
}
if (-not (Get-Command $Compiler -ErrorAction SilentlyContinue) -and -not (Test-Path $Compiler)) {
    throw "C compiler '$Compiler' was not found. Set CC to a GCC-compatible compiler."
}

& $Cargo build
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

New-Item -ItemType Directory -Force -Path $Build | Out-Null
$Library = @(
    (Join-Path $CargoTarget "debug\libmpack.dll.a"),
    (Join-Path $CargoTarget "debug\mpack.lib"),
    (Join-Path $CargoTarget "debug\mpack.dll.lib")
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $Library) {
    throw "Cargo did not produce a linkable mpack cdylib import library."
}
$RuntimeLibrary = Join-Path $CargoTarget "debug\mpack.dll"
if (Test-Path $RuntimeLibrary) {
    Copy-Item -Force $RuntimeLibrary $Build
}

if ($Full) {
    $Sources = Get-ChildItem (Join-Path $FrozenUnit "src") -Filter "*.c" | Sort-Object Name | ForEach-Object FullName
    $Output = Join-Path $Build "embed-writer-frozen.exe"
} else {
    $Sources = @(Join-Path $PSScriptRoot "c\frozen_nil_smoke.c")
    $Output = Join-Path $Build "embed-writer-nil-smoke.exe"
}
$Sources += Join-Path $Root "original_c\mpack-develop\src\mpack\mpack-platform.c"

$Arguments = @(
    "-std=c11",
    "-DMPACK_HAS_CONFIG=1",
    "-I$ConfigInclude",
    "-I$UpstreamInclude",
    "-I$(Join-Path $FrozenUnit 'src')"
) + $Sources + @($Library, "-o", $Output)

& $Compiler @Arguments
if ($LASTEXITCODE -ne 0) {
    if ($Full -and $ExpectMissing) {
        Write-Host "Full frozen-suite link is incomplete as expected: Rust writer symbols remain to be implemented."
        exit 0
    }
    exit $LASTEXITCODE
}

& $Output
exit $LASTEXITCODE
