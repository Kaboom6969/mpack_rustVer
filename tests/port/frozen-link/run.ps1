param(
    [switch]$EmbedWriter,
    [switch]$Full, # deprecated alias for -EmbedWriter
    [switch]$ExpectMissing,
    [switch]$Everything,
    [switch]$DefaultConfig,
    [switch]$SoftContinue,
    [switch]$Release
)

$ErrorActionPreference = "Stop"

if ($ExpectMissing) {
    throw "-ExpectMissing is removed: an incomplete link must fail. Unresolved symbols are not a successful checkpoint."
}
if ($Everything -and $DefaultConfig) {
    throw "Use only one of -Everything / -DefaultConfig."
}
$EverythingMode = $Everything -or $DefaultConfig
if ($Full) {
    if ($EverythingMode) {
        Write-Warning "-Full is deprecated; -Everything alone is enough."
    } else {
        Write-Warning "-Full is deprecated and misleading; use -EmbedWriter."
        $EmbedWriter = $true
    }
}
if ($EmbedWriter -and $EverythingMode) {
    throw "Use only one of -EmbedWriter / -Everything."
}
$RunFrozenSuite = $EmbedWriter -or $EverythingMode
if ($SoftContinue -and -not $EverythingMode) {
    throw "-SoftContinue requires -Everything (or -DefaultConfig)."
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

if ($EverythingMode) {
    # staticlib only: Windows cdylib cannot leave suite symbols undefined.
    $CargoArguments = @("rustc", "--target", $RustTarget)
    if ($Release) {
        $CargoArguments += "--release"
    }
    $CargoArguments += @(
        "--features", "full-suite-abi",
        "--crate-type", "staticlib",
        "--", "--cfg", "mpack_frozen_link"
    )
} else {
    $CargoArguments = @("rustc", "--target", $RustTarget)
    if ($Release) {
        $CargoArguments += "--release"
    }
    $CargoArguments += @("--features", "ffi", "--crate-type", "cdylib")
}
& $Cargo @CargoArguments
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

New-Item -ItemType Directory -Force -Path $Build | Out-Null
$Profile = if ($Release) { "release" } else { "debug" }
$RustOutput = Join-Path $CargoTarget "$RustTarget\$Profile"

if ($EverythingMode) {
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

if ($EverythingMode) {
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

if ($RunFrozenSuite) {
    $Sources = Get-ChildItem (Join-Path $FrozenUnit "src") -Filter "*.c" | Sort-Object Name | ForEach-Object FullName
    $Output = Join-Path $Build "$ConfigName-$Profile-frozen.exe"
} else {
    $Sources = @(Join-Path $PSScriptRoot "c\frozen_nil_smoke.c")
    $Output = Join-Path $Build "$ConfigName-$Profile-nil-smoke.exe"
}
$Sources += Join-Path $Root "original_c\mpack-develop\src\mpack\mpack-platform.c"

if ($EverythingMode) {
    $Sources += Join-Path $PSScriptRoot "c\full_layout_check.c"
    if ($SoftContinue) {
        $Sources += Join-Path $PSScriptRoot "c\soft_abort.c"
        $Sources += Join-Path $PSScriptRoot "c\quiet_printf.c"
    }
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
if ($EverythingMode) {
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
if ($SoftContinue) {
    # DEBUG ONLY — not parity. Soft-abort / quiet printf for full failure lists.
    $Arguments += @(
        "-include$(Join-Path $PSScriptRoot 'c\soft_abort.h')",
        "-include$(Join-Path $PSScriptRoot 'c\quiet_printf.h')"
    )
}
$Arguments += $Sources + @($Library) + $NativeStaticLibs
if ($EverythingMode) {
    # Retained only for staticlib + mpack-platform.c vs Rust #[no_mangle] overlap.
    $Arguments += "-Wl,--allow-multiple-definition"
}
$Arguments += @("-o", $Output)

& $Compiler @Arguments
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

if (-not $RunFrozenSuite) {
    # Nil smoke only — not the frozen unit suite.
    & $Output
    exit $LASTEXITCODE
}

# Capture suite stdout/stderr so we can parse the C harness summary while still
# printing it. Parity = suite exit + "Unit testing complete. N failures".
$SuiteOut = & $Output 2>&1 | ForEach-Object { $_.ToString() }
$SuiteExit = $LASTEXITCODE
$SuiteText = ($SuiteOut -join "`n")
if ($SuiteText) {
    Write-Host $SuiteText
}

$SummaryMatch = [regex]::Match(
    $SuiteText,
    "Unit testing complete\.\s+(\d+)\s+failures\s+in\s+(\d+)\s+checks\."
)
$SoftNote = if ($SoftContinue) { "; soft-continue" } else { "" }

if ($SummaryMatch.Success) {
    $Failures = [int]$SummaryMatch.Groups[1].Value
    $Checks = [int]$SummaryMatch.Groups[2].Value
    Write-Host "Frozen suite summary (from C harness): $Failures failures in $Checks checks (process exit=$SuiteExit$SoftNote)."
    if ($Failures -ne 0) {
        if ($SuiteExit -ne 0) { exit $SuiteExit }
        exit 1
    }
    if ($SuiteExit -ne 0) {
        Write-Host "Summary reports 0 failures but process exit is non-zero; forwarding suite exit."
        exit $SuiteExit
    }
    exit 0
}

# No summary: typical when TEST_EARLY_EXIT aborts before main returns.
if ($SuiteExit -lt 0 -or $SuiteExit -ge 128) {
    Write-Host "Frozen suite aborted/crashed before summary (exit=$SuiteExit); treating as failure."
    if ($SuiteExit -eq 0) { exit 1 }
    exit $SuiteExit
}
if ($SuiteExit -ne 0) {
    Write-Host "Frozen suite exited without a Unit testing complete summary (exit=$SuiteExit); treating as failure."
    exit $SuiteExit
}
Write-Host "Frozen suite exit 0 but missing Unit testing complete summary; treating as failure."
exit 1
