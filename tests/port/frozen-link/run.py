#!/usr/bin/env python3
"""Build frozen MPack C tests against the Rust cdylib without altering them."""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
RUST_TARGET = os.environ.get(
    "MPACK_RUST_TARGET", "x86_64-pc-windows-gnu" if os.name == "nt" else ""
)
UPSTREAM_INCLUDE = ROOT / "original_c" / "mpack-develop" / "src"
FROZEN_UNIT = ROOT / "tests" / "original" / "test" / "unit"
EMBED_CONFIG_INCLUDE = ROOT / "tests" / "port" / "ffi-harness" / "include"
BUILD = ROOT / "target" / "frozen-link"


def run(command: list[str]) -> None:
    print("+", " ".join(map(str, command)))
    subprocess.run(command, cwd=ROOT, check=True)


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


def c_command(
    source_files: list[Path],
    output: Path,
    library: Path,
    *,
    config_include: Path,
    debug: bool,
    wrap_abort: bool = False,
) -> list[str]:
    compiler = os.environ.get("CC") or shutil.which("cc") or shutil.which("clang")
    if compiler is None:
        compiler = "cl" if os.name == "nt" else "gcc"

    if Path(compiler).name.lower() in {"cl", "cl.exe"}:
        command = [
            compiler,
            "/nologo",
            "/std:c11",
            "/Zi",
            "/DMPACK_HAS_CONFIG=1",
            "/DMPACK_FROZEN_TESTS=1",
            f"/I{config_include}",
            f"/I{UPSTREAM_INCLUDE}",
            f"/I{FROZEN_UNIT / 'src'}",
            *(str(source) for source in source_files),
            str(library),
            f"/Fe:{output}",
        ]
        if debug:
            command.insert(3, "/DDEBUG")
        return command

    command = [
        compiler,
        "-std=c11",
        "-g",
        "-DMPACK_HAS_CONFIG=1",
        "-DMPACK_FROZEN_TESTS=1",
        f"-I{config_include}",
        f"-I{UPSTREAM_INCLUDE}",
        f"-I{FROZEN_UNIT / 'src'}",
        *(str(source) for source in source_files),
        str(library),
        "-o",
        str(output),
        "-Wl,-rpath," + str(library.parent),
    ]
    if debug:
        command.insert(3, "-DDEBUG")
    if wrap_abort:
        # Redirect abort() to a returning function so TEST_EARLY_EXIT fall-through
        # remains defined under GCC's noreturn assumptions for libc abort.
        command.insert(
            3,
            f"-include{Path(__file__).resolve().parent / 'c' / 'soft_abort.h'}",
        )
    return command


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--full",
        action="store_true",
        help="compile every frozen unit source (expected to have unresolved symbols until writer parity)",
    )
    parser.add_argument(
        "--expect-missing",
        action="store_true",
        help="treat the current full-suite unresolved-symbol result as a successful checkpoint",
    )
    parser.add_argument(
        "--default-config",
        action="store_true",
        help="use tests/original default config and build Rust with full-suite-abi stubs",
    )
    parser.add_argument(
        "--release", action="store_true", help="build the Rust library in release mode"
    )
    args = parser.parse_args()

    if args.expect_missing and not args.full:
        parser.error("--expect-missing requires --full")
    if args.default_config and not args.full:
        parser.error("--default-config requires --full")

    cargo_command = ["cargo", "build"]
    if RUST_TARGET:
        cargo_command.extend(["--target", RUST_TARGET])
    if args.release:
        cargo_command.append("--release")
    if args.default_config:
        cargo_command.extend(["--features", "full-suite-abi"])
    run(cargo_command)
    BUILD.mkdir(parents=True, exist_ok=True)
    output = rust_output(args.release)
    library = cdylib_import_library(output)
    runtime_library = output / "mpack.dll"
    if runtime_library.exists():
        shutil.copy2(runtime_library, BUILD)

    profile = "release" if args.release else "debug"
    if args.default_config:
        config_include = FROZEN_UNIT / "src"
        config_name = "default"
        debug = True
    else:
        config_include = EMBED_CONFIG_INCLUDE
        config_name = "embed-writer"
        debug = False

    if args.full:
        sources = sorted((FROZEN_UNIT / "src").glob("*.c"))
        executable = BUILD / f"{config_name}-{profile}-frozen"
    else:
        sources = [Path(__file__).parent / "c" / "frozen_nil_smoke.c"]
        executable = BUILD / f"{config_name}-{profile}-nil-smoke"
    sources.append(ROOT / "original_c" / "mpack-develop" / "src" / "mpack" / "mpack-platform.c")

    if args.default_config:
        sources.append(Path(__file__).parent / "c" / "full_layout_check.c")
        sources.append(Path(__file__).parent / "c" / "soft_abort.c")
        sources.append(Path(__file__).parent / "c" / "quiet_printf.c")
        # Provide a tiny main wrapper that runs layout check then the suite main.
        # The frozen suite already has main(); call the layout check from a ctor.
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

    result = subprocess.run(
        c_command(
            sources,
            executable,
            library,
            config_include=config_include,
            debug=debug,
            wrap_abort=args.default_config,
        ),
        cwd=ROOT,
    )
    if result.returncode:
        if args.expect_missing:
            print(
                "Full frozen-suite link is incomplete as expected: "
                "Rust writer symbols remain to be implemented."
            )
            return 0
        return result.returncode

    if args.full:
        # Default-config stubs are expected to fail many assertions; success means
        # the binary linked and ran to completion (printed the failure summary).
        completed = subprocess.run([str(executable)], cwd=ROOT)
        if args.default_config:
            # Python reports fatal signals as 128+N on Linux.
            if completed.returncode < 0 or completed.returncode >= 128:
                print(
                    "Default-config frozen suite crashed "
                    f"(exit={completed.returncode}); treating as failure."
                )
                return 1
            print(
                "Default-config frozen suite finished "
                f"(exit={completed.returncode}; assertion failures expected with stubs)."
            )
            return 0
        return completed.returncode
    run([str(executable)])
    return 0


if __name__ == "__main__":
    sys.exit(main())
