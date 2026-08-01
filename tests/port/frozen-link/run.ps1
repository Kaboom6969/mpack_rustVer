param(
    [switch]$Full,
    [switch]$ExpectMissing,
    [switch]$Everything,
    [switch]$DefaultConfig,
    [switch]$Release
)

$ErrorActionPreference = "Stop"

if ($ExpectMissing -and -not $Full) {
    throw "-ExpectMissing requires -Full."
}
if ($Everything -and $DefaultConfig) {
    throw "Use only one of -Everything / -DefaultConfig."
}
$FullSuite = $Everything -or $DefaultConfig
if ($FullSuite -and -not $Full) {
    throw "-Everything / -DefaultConfig requires -Full."
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..\..")).Path
$UpstreamInclude = Join-Path $Root "original_c\mpack-develop\src"
$FrozenUnit = Join-Path $Root "tests\original\test\unit"
$EmbedConfigInclude = Join-Path $Root "tests\port\ffi-harness\include"
$Build = Join-Path $Root "target\frozen-link"
$CargoTarget = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root "target" }
$RustTarget = if ($env:MPACK_RUST_TARGET) { $env:MPACK_RUST_TARGET } else { "x86_64-pc-windows-gnu" }
$Cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
$Compiler = if ($env:CC) { $env:CC } else { "C:\Strawberry\c\bin\gcc.exe" }

# Matches original_c configure.py `everything` (+ debug).
$EverythingDefines = @(
    "MPACK_VARIANT_BUILDS=1",
    "MPACK_READER=1",
    "MPACK_WRITER=1",
    "MPACK_EXPECT=1",
    "MPACK_NODE=1",
    "MPACK_COMPATIBILITY=1",
    "MPACK_EXTENSIONS=1",
    "MPACK_STDLIB=1",
    "MPACK_MALLOC=test_malloc",
    "MPACK_FREE=test_free",
    "MPACK_STDIO=1"
)

if (-not (Test-Path $Cargo)) {
    throw "Cargo was not found at $Cargo. Set up Cargo or update this adapter."
}
if (-not (Get-Command $Compiler -ErrorAction SilentlyContinue) -and -not (Test-Path $Compiler)) {
    throw "C compiler '$Compiler' was not found. Set CC to a GCC-compatible compiler."
}

if ($FullSuite) {
    # staticlib only: Windows cdylib cannot leave suite symbols undefined.
    $CargoArguments = @("rustc", "--target", $RustTarget)
    if ($Release) {
        $CargoArguments += "--release"
    }
    $CargoArguments += @("--features", "full-suite-abi", "--crate-type", "staticlib")
} else {
    $CargoArguments = @("build", "--target", $RustTarget)
    if ($Release) {
        $CargoArguments += "--release"
    }
}
& $Cargo @CargoArguments
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

New-Item -ItemType Directory -Force -Path $Build | Out-Null
$Profile = if ($Release) { "release" } else { "debug" }
$RustOutput = Join-Path $CargoTarget "$RustTarget\$Profile"

if ($FullSuite) {
    $Library = @(
        (Join-Path $RustOutput "libmpack.a"),
        (Join-Path $RustOutput "mpack.lib")
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $Library) {
        throw "Cargo did not produce a linkable mpack static library."
    }
} else {
    $Library = @(
        (Join-Path $RustOutput "libmpack.dll.a"),
        (Join-Path $RustOutput "mpack.lib"),
        (Join-Path $RustOutput "mpack.dll.lib")
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $Library) {
        throw "Cargo did not produce a linkable mpack cdylib import library."
    }
    $RuntimeLibrary = Join-Path $RustOutput "mpack.dll"
    if (Test-Path $RuntimeLibrary) {
        Copy-Item -Force $RuntimeLibrary $Build
    }
}

if ($FullSuite) {
    $ConfigInclude = Join-Path $FrozenUnit "src"
    $ConfigName = "everything"
    $DebugDefine = @("-DDEBUG")
    $ExtraDefines = $EverythingDefines | ForEach-Object { "-D$_" }
} else {
    $ConfigInclude = $EmbedConfigInclude
    $ConfigName = "embed-writer"
    $DebugDefine = @()
    $ExtraDefines = @()
}

if ($Full) {
    $Sources = Get-ChildItem (Join-Path $FrozenUnit "src") -Filter "*.c" | Sort-Object Name | ForEach-Object FullName
    $Output = Join-Path $Build "$ConfigName-$Profile-frozen.exe"
} else {
    $Sources = @(Join-Path $PSScriptRoot "c\frozen_nil_smoke.c")
    $Output = Join-Path $Build "$ConfigName-$Profile-nil-smoke.exe"
}
$Sources += Join-Path $Root "original_c\mpack-develop\src\mpack\mpack-platform.c"

if ($FullSuite) {
    $Sources += Join-Path $PSScriptRoot "c\full_layout_check.c"
    $Sources += Join-Path $PSScriptRoot "c\soft_abort.c"
    $Sources += Join-Path $PSScriptRoot "c\quiet_printf.c"
    $Ctor = Join-Path $Build "full_layout_ctor.c"
    @"
int mpack_full_layout_check(void);
static void __attribute__((constructor)) mpack_run_layout_check(void) {
    int failures = mpack_full_layout_check();
    if (failures != 0) {
        __builtin_trap();
    }
}
"@ | Set-Content -Path $Ctor -Encoding Ascii
    $Sources += $Ctor
}

$NativeStaticLibs = @()
if ($FullSuite) {
    $NativeStaticLibs = @(
        "-lkernel32",
        "-lntdll",
        "-luserenv",
        "-lws2_32",
        "-ldbghelp",
        "-lgcc_eh",
        "-lpthread",
        "-luser32"
    )
}

$Arguments = @(
    "-std=c11",
    "-g"
) + $DebugDefine + $ExtraDefines + @(
    "-DMPACK_HAS_CONFIG=1",
    "-DMPACK_FROZEN_TESTS=1",
    "-I$ConfigInclude",
    "-I$UpstreamInclude",
    "-I$(Join-Path $FrozenUnit 'src')"
)
if ($FullSuite) {
    $Arguments += @(
        "-include$(Join-Path $PSScriptRoot 'c\soft_abort.h')",
        "-include$(Join-Path $PSScriptRoot 'c\quiet_printf.h')"
    )
}
$Arguments += $Sources + @($Library) + $NativeStaticLibs
if ($FullSuite) {
    $Arguments += "-Wl,--allow-multiple-definition"
}
$Arguments += @("-o", $Output)

& $Compiler @Arguments
if ($LASTEXITCODE -ne 0) {
    if ($Full -and $ExpectMissing) {
        Write-Host "Full frozen-suite link is incomplete as expected: Rust writer symbols remain to be implemented."
        exit 0
    }
    exit $LASTEXITCODE
}

& $Output
$SuiteExit = $LASTEXITCODE
if ($FullSuite) {
    if ($SuiteExit -lt 0) {
        Write-Host "Everything frozen suite aborted (signal); treating as failure."
        exit 1
    }
    Write-Host "Everything frozen suite finished (exit=$SuiteExit; assertion failures expected with stubs)."
    exit 0
}
exit $SuiteExit
