#!/usr/bin/env python3
"""Build frozen MPack C tests against the Rust library without altering them.

Parity gate: acceptance is the frozen suite binary's own exit code and its
printed ``Unit testing complete. N failures`` line. This runner only builds,
links, and forwards — it must not rewrite a failing suite into success.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
RUST_TARGET = os.environ.get(
    "MPACK_RUST_TARGET", "x86_64-pc-windows-gnu" if os.name == "nt" else ""
)
VENDORED_UPSTREAM_INCLUDE = ROOT / "include" / "upstream"
FROZEN_UNIT = ROOT / "tests" / "original" / "test" / "unit"
EMBED_CONFIG_INCLUDE = ROOT / "tests" / "port" / "ffi-harness" / "include"
BUILD = ROOT / "target" / "frozen-link"

# Matches upstream configure.py `everything` (+ debug): allfeatures + allconfigs.
EVERYTHING_DEFINES = [
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
    "MPACK_STDIO=1",
]

SUMMARY_RE = re.compile(
    r"Unit testing complete\.\s+(\d+)\s+failures\s+in\s+(\d+)\s+checks\."
)
TEST_FAILED_RE = re.compile(r"^TEST FAILED AT\b")
# Incidental stdout from frozen print tests (e.g. mpack_print("testing...")).
INCIDENTAL_PRINT_RE = re.compile(r'^\s*"testing\.\.\."\s*$')


def resolve_upstream_include() -> Path:
    override = os.environ.get("MPACK_UPSTREAM_SRC")
    if override:
        include_root = Path(override).expanduser().resolve()
    else:
        include_root = VENDORED_UPSTREAM_INCLUDE

    header = include_root / "mpack" / "mpack.h"
    platform = include_root / "mpack" / "mpack-platform.c"
    if not header.is_file() or not platform.is_file():
        raise SystemExit(
            f"Invalid MPACK_UPSTREAM_SRC/include root: {include_root} "
            f"(expected mpack/mpack.h and mpack/mpack-platform.c)"
        )
    return include_root


def run(command: list[str], *, env: dict[str, str] | None = None) -> None:
    print("+", " ".join(map(str, command)))
    subprocess.run(command, cwd=ROOT, check=True, env=env)


def rust_output(release: bool) -> Path:
    profile = "release" if release else "debug"
    return TARGET / RUST_TARGET / profile if RUST_TARGET else TARGET / profile


def cdylib_import_library(output: Path) -> Path:
    candidates = (
        output / name
        for name in (
            "mpack.lib",
            "mpack.dll.lib",
            "libmpack.dll.a",
            "libmpack.so",
            "libmpack.dylib",
        )
    )
    for candidate in candidates:
        if candidate.exists():
            return candidate
    raise FileNotFoundError("Cargo did not produce a linkable mpack cdylib import library.")


def static_library(output: Path) -> Path:
    candidates = (output / name for name in ("libmpack.a", "mpack.lib"))
    for candidate in candidates:
        if candidate.exists():
            return candidate
    raise FileNotFoundError("Cargo did not produce a linkable mpack static library.")


def native_static_libs() -> list[str]:
    """System libraries required when linking a Rust staticlib from C."""
    target = RUST_TARGET or ("x86_64-pc-windows-gnu" if os.name == "nt" else "")
    if "windows-gnu" in target or (os.name == "nt" and not target):
        return [
            "-lkernel32",
            "-lntdll",
            "-luserenv",
            "-lws2_32",
            "-ldbghelp",
            "-lgcc_eh",
            "-lpthread",
            "-luser32",
        ]
    if "apple" in target or sys.platform == "darwin":
        return ["-framework", "Security", "-lSystem"]
    return ["-ldl", "-lpthread", "-lm"]


def c_command(
    source_files: list[Path],
    output: Path,
    library: Path,
    *,
    config_include: Path,
    upstream_include: Path,
    debug: bool,
    soft_continue: bool = False,
    extra_defines: list[str] | None = None,
    link_static: bool = False,
) -> list[str]:
    compiler = os.environ.get("CC") or shutil.which("gcc") or shutil.which("cc") or shutil.which("clang")
    if compiler is None:
        compiler = "cl" if os.name == "nt" else "gcc"

    defines = ["MPACK_HAS_CONFIG=1", "MPACK_FROZEN_TESTS=1"]
    if extra_defines:
        defines.extend(extra_defines)
    if debug:
        defines.append("DEBUG")

    if Path(compiler).name.lower() in {"cl", "cl.exe"}:
        command = [
            compiler,
            "/nologo",
            "/std:c11",
            "/Zi",
            *[f"/D{define}" for define in defines],
            f"/I{config_include}",
            f"/I{upstream_include}",
            f"/I{FROZEN_UNIT / 'src'}",
            *(str(source) for source in source_files),
            str(library),
            f"/Fe:{output}",
        ]
        return command

    command = [
        compiler,
        "-std=c11",
        "-g",
        *[f"-D{define}" for define in defines],
        f"-I{config_include}",
        f"-I{upstream_include}",
        f"-I{FROZEN_UNIT / 'src'}",
        *(str(source) for source in source_files),
        str(library),
        "-o",
        str(output),
    ]
    if soft_continue:
        # Debug-only: redirect abort() so TEST_EARLY_EXIT fall-through remains
        # defined under GCC noreturn assumptions, and quiet printf spam.
        # Must not be used as the parity acceptance path.
        adapter_c = Path(__file__).resolve().parent / "c"
        command.insert(3, f"-include{adapter_c / 'soft_abort.h'}")
        command.insert(4, f"-include{adapter_c / 'quiet_printf.h'}")
    if not link_static:
        command.append("-Wl,-rpath," + str(library.parent))
    else:
        # Retained only for staticlib + mpack-platform.c vs Rust #[no_mangle]
        # overlap. Prefer fixing duplicate exports over widening this flag.
        # Documented risk: silent wrong-definition selection.
        command.extend(["-Wl,--allow-multiple-definition"])
        command.extend(native_static_libs())
    return command


def prepare_full_suite_extras(sources: list[Path], *, soft_continue: bool) -> None:
    sources.append(Path(__file__).parent / "c" / "full_layout_check.c")
    if soft_continue:
        sources.append(Path(__file__).parent / "c" / "soft_abort.c")
        sources.append(Path(__file__).parent / "c" / "quiet_printf.c")
    ctor = BUILD / "full_layout_ctor.c"
    ctor.write_text(
        "int mpack_full_layout_check(void);\n"
        "static void __attribute__((constructor)) mpack_run_layout_check(void) {\n"
        "    int failures = mpack_full_layout_check();\n"
        "    if (failures != 0) {\n"
        "        __builtin_trap();\n"
        "    }\n"
        "}\n"
    )
    sources.append(ctor)


def ensure_unit_test_data_link() -> None:
    """Ensure `test/` resolves MessagePack fixtures relative to the repo root.

    Upstream unit tests open paths like `test/messagepack/...` with cwd at the
    MPack project root. Frozen-link runs from the Rust repo root, so expose the
    frozen fixture tree via a `test` symlink (or junction on Windows).
    """
    link = ROOT / "test"
    target = ROOT / "tests" / "original" / "test"
    if not target.is_dir():
        raise SystemExit(f"missing frozen unit fixtures at {target}")
    if link.is_symlink() or link.exists():
        if link.is_dir() and (link / "messagepack").is_dir():
            return
        if link.is_symlink() or link.is_file():
            link.unlink()
        else:
            raise SystemExit(f"refusing to replace unexpected path {link}")
    link.symlink_to(target, target_is_directory=True)


def suite_verdict(returncode: int, stdout: str, *, soft_continue: bool) -> int:
    """Map suite process output to a runner exit code.

    Acceptance data comes from the C suite itself: its exit status and the
    ``Unit testing complete`` line it prints. Soft-continue never turns a
    failing suite into success.
    """
    match = SUMMARY_RE.search(stdout)
    if match:
        failures = int(match.group(1))
        checks = int(match.group(2))
        if failures != 0:
            # Suite already returns EXIT_FAILURE; force non-zero if somehow 0.
            return returncode if returncode != 0 else 1
        if returncode != 0:
            print(
                "Summary reports 0 failures but process exit is non-zero; "
                "forwarding suite exit.",
                file=sys.stderr,
            )
            return returncode
        return 0

    # No summary: typical when TEST_EARLY_EXIT aborts before main returns.
    if returncode < 0 or returncode >= 128:
        print(
            "Frozen suite aborted/crashed before summary "
            f"(exit={returncode}); treating as failure."
        )
        return 1 if returncode == 0 else returncode
    if returncode != 0:
        print(
            "Frozen suite exited without a Unit testing complete summary "
            f"(exit={returncode}); treating as failure."
        )
        return returncode
    print(
        "Frozen suite exit 0 but missing Unit testing complete summary; "
        "treating as failure.",
        file=sys.stderr,
    )
    return 1


def extract_suite_signal_lines(stdout: str, stderr: str) -> tuple[list[str], list[str], str | None]:
    """Keep harness signal lines; drop incidental C print-test stdout."""
    failures: list[str] = []
    other: list[str] = []
    summary: str | None = None

    for raw in (stdout or "").splitlines() + (stderr or "").splitlines():
        line = raw.rstrip()
        if not line.strip():
            continue
        if INCIDENTAL_PRINT_RE.match(line):
            continue
        if SUMMARY_RE.search(line):
            summary = line.strip()
            continue
        if TEST_FAILED_RE.match(line.strip()):
            failures.append(line.strip())
            continue
        # Keep unexpected residual lines (rare) for debugging.
        other.append(line)

    return failures, other, summary


def collect_env_info(config_name: str, profile: str, soft_continue: bool) -> list[str]:
    lines = [
        "=== Suite Log ===",
        f"Timestamp: {datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M:%S UTC')}",
        f"Platform: {sys.platform} (os.name={os.name})",
    ]

    try:
        git_commit = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            capture_output=True,
            text=True,
            cwd=ROOT,
        )
        if git_commit.returncode == 0:
            lines.append(f"Git Commit: {git_commit.stdout.strip()}")
        git_branch = subprocess.run(
            ["git", "rev-parse", "--abbrev-ref", "HEAD"],
            capture_output=True,
            text=True,
            cwd=ROOT,
        )
        if git_branch.returncode == 0:
            lines.append(f"Git Branch: {git_branch.stdout.strip()}")
    except Exception:
        pass

    try:
        rustc_proc = subprocess.run(
            ["rustc", "-vV"],
            capture_output=True,
            text=True,
            cwd=ROOT,
        )
        if rustc_proc.returncode == 0:
            lines.append("Rustc Info:")
            for line in rustc_proc.stdout.strip().splitlines():
                lines.append(f"  {line}")
    except Exception:
        pass

    compiler = (
        os.environ.get("CC")
        or shutil.which("gcc")
        or shutil.which("cc")
        or shutil.which("clang")
        or "gcc"
    )
    lines.append("Compiler Environment:")
    lines.append(f"  CC: {compiler}")

    lines.append("Suite Configuration:")
    lines.append(f"  Config Name: {config_name}")
    lines.append(f"  Profile: {profile}")
    lines.append(f"  Soft Continue: {soft_continue}")
    lines.append("")
    return lines


def format_suite_log(
    *,
    config_name: str,
    profile: str,
    soft_continue: bool,
    returncode: int,
    stdout: str,
    stderr: str,
) -> str:
    """Structured suite log aligned with fuzz_log style (no incidental print noise)."""
    chunks = collect_env_info(config_name, profile, soft_continue)
    failures, other, summary = extract_suite_signal_lines(stdout, stderr)
    match = SUMMARY_RE.search(stdout or "")
    failures_n = int(match.group(1)) if match else None
    checks_n = int(match.group(2)) if match else None

    chunks.append("=" * 72)
    chunks.append("Suite Result:")
    if match and failures_n is not None and checks_n is not None:
        status = "ok" if failures_n == 0 and returncode == 0 else "FAIL"
        chunks.append(
            f"  Status: {status} - {failures_n} failures in {checks_n} checks "
            f"(process exit={returncode}"
            f"{'; soft-continue' if soft_continue else ''})"
        )
        if summary:
            chunks.append(f"  Harness: {summary}")
    elif returncode < 0 or returncode >= 128:
        chunks.append(
            f"  Status: FAIL - aborted/crashed before summary (exit={returncode})"
        )
    else:
        chunks.append(
            f"  Status: FAIL - missing Unit testing complete summary "
            f"(exit={returncode})"
        )

    chunks.append("")
    chunks.append("Failures:")
    if failures:
        for line in failures:
            chunks.append(f"  {line}")
    else:
        chunks.append("  (none)")

    if other:
        chunks.append("")
        chunks.append("Other harness output (filtered):")
        for line in other:
            chunks.append(f"  {line}")

    chunks.append("")
    chunks.append(
        "Notes: incidental C print-test stdout (e.g. quoted \"testing...\") "
        "is omitted; pass/fail comes only from the harness summary / "
        "TEST FAILED lines."
    )
    chunks.append("")
    return "\n".join(chunks)


def run_suite_executable(
    executable: Path,
    *,
    soft_continue: bool,
    config_name: str = "everything",
    profile: str = "debug",
) -> int:
    completed = subprocess.run(
        [str(executable)],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )

    log_text = format_suite_log(
        config_name=config_name,
        profile=profile,
        soft_continue=soft_continue,
        returncode=completed.returncode,
        stdout=completed.stdout or "",
        stderr=completed.stderr or "",
    )

    # Console: same structured view (no raw "testing..." spam).
    sys.stdout.write(log_text)
    if not log_text.endswith("\n"):
        sys.stdout.write("\n")
    sys.stdout.flush()

    log_path = ROOT / "suite_log.txt"
    try:
        log_path.write_text(log_text, encoding="utf-8")
    except Exception as e:
        print(f"Warning: failed to write log to {log_path}: {e}")

    verdict = suite_verdict(
        completed.returncode,
        completed.stdout or "",
        soft_continue=soft_continue,
    )
    print(f"\nWrote {log_path}", flush=True)
    return verdict


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Link and run frozen MPack C tests against the Rust library. "
            "Parity success = suite exit 0 and '0 failures' in the C summary."
        )
    )
    parser.add_argument(
        "--embed-writer",
        action="store_true",
        help=(
            "run the frozen C unit suite under the embed-writer config "
            "(writer gate; without a suite flag only frozen_nil_smoke.c runs)"
        ),
    )
    parser.add_argument(
        "--full",
        action="store_true",
        help=argparse.SUPPRESS,  # deprecated alias for --embed-writer
    )
    parser.add_argument(
        "--expect-missing",
        action="store_true",
        help=argparse.SUPPRESS,
    )
    parser.add_argument(
        "--everything",
        action="store_true",
        help=(
            "run the frozen C unit suite under the C everything config "
            "(reader/expect/node/stdio/compat/extensions + full-suite-abi); "
            "parity gate requires 0 failures"
        ),
    )
    parser.add_argument(
        "--default-config",
        action="store_true",
        help="alias for --everything (kept for older docs/scripts)",
    )
    parser.add_argument(
        "--soft-continue",
        action="store_true",
        help=(
            "DEBUG ONLY: soft-abort + quiet printf so the suite continues past "
            "TEST_EARLY_EXIT and prints a full failure summary. Still forwards "
            "the suite exit / failure count — never a fake green. Not parity."
        ),
    )
    parser.add_argument(
        "--release", action="store_true", help="build the Rust library in release mode"
    )
    args = parser.parse_args()

    if args.expect_missing:
        parser.error(
            "--expect-missing is removed: an incomplete link must fail. "
            "Unresolved symbols are not a successful checkpoint."
        )
    if args.everything and args.default_config:
        parser.error("use only one of --everything / --default-config")
    everything = args.everything or args.default_config
    if args.full:
        # Old docs used `--full` (suite) and `--full --everything`.
        # `--full` alone → embed-writer; with --everything → ignore --full.
        if everything:
            print(
                "warning: --full is deprecated; `--everything` alone is enough.",
                file=sys.stderr,
            )
        else:
            print(
                "warning: --full is deprecated and misleading; use --embed-writer.",
                file=sys.stderr,
            )
            args.embed_writer = True
    if args.embed_writer and everything:
        parser.error("use only one of --embed-writer / --everything")
    run_frozen_suite = args.embed_writer or everything
    if args.soft_continue and not everything:
        parser.error("--soft-continue requires --everything (or --default-config)")

    if everything:
        # Build only the staticlib: a Windows cdylib cannot leave suite symbols
        # (test_malloc / mpack_assert_fail) undefined, but those must come from
        # the frozen C objects at final exe link.
        cargo_command = ["cargo", "rustc"]
        if RUST_TARGET:
            cargo_command.extend(["--target", RUST_TARGET])
        if args.release:
            cargo_command.append("--release")
        cargo_command.extend(
            [
                "--features",
                "full-suite-abi",
                "--crate-type",
                "staticlib",
                "--",
                "--cfg",
                "mpack_frozen_link",
            ]
        )
    else:
        cargo_command = ["cargo", "rustc"]
        if RUST_TARGET:
            cargo_command.extend(["--target", RUST_TARGET])
        if args.release:
            cargo_command.append("--release")
        cargo_command.extend(["--features", "ffi", "--crate-type", "cdylib"])
    run(cargo_command)
    BUILD.mkdir(parents=True, exist_ok=True)
    output = rust_output(args.release)

    # Everything mode links the staticlib so suite-provided symbols (test_malloc,
    # mpack_assert_fail) resolve at final exe link. Embed-writer / smoke keep cdylib.
    if everything:
        library = static_library(output)
        link_static = True
    else:
        library = cdylib_import_library(output)
        link_static = False
        runtime_library = output / "mpack.dll"
        if runtime_library.exists():
            shutil.copy2(runtime_library, BUILD)

    profile = "release" if args.release else "debug"
    if everything:
        config_include = FROZEN_UNIT / "src"
        config_name = "everything"
        debug = True
        extra_defines = EVERYTHING_DEFINES
    else:
        config_include = EMBED_CONFIG_INCLUDE
        config_name = "embed-writer"
        debug = False
        extra_defines = None

    upstream_include = resolve_upstream_include()

    if run_frozen_suite:
        sources = sorted((FROZEN_UNIT / "src").glob("*.c"))
        executable = BUILD / f"{config_name}-{profile}-frozen"
    else:
        sources = [Path(__file__).parent / "c" / "frozen_nil_smoke.c"]
        executable = BUILD / f"{config_name}-{profile}-nil-smoke"
    sources.append(upstream_include / "mpack" / "mpack-platform.c")

    soft_continue = bool(args.soft_continue)
    if everything:
        prepare_full_suite_extras(sources, soft_continue=soft_continue)
        ensure_unit_test_data_link()

    cmd = c_command(
        sources,
        executable,
        library,
        config_include=config_include,
        upstream_include=upstream_include,
        debug=debug,
        soft_continue=soft_continue,
        extra_defines=extra_defines,
        link_static=link_static,
    )
    print("+", " ".join(map(str, cmd)))
    result = subprocess.run(cmd, cwd=ROOT)
    if result.returncode:
        return result.returncode

    if run_frozen_suite:
        return run_suite_executable(
            executable,
            soft_continue=soft_continue,
            config_name=config_name,
            profile=profile,
        )
    run([str(executable)])
    return 0


if __name__ == "__main__":
    sys.exit(main())
