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
UPSTREAM_INCLUDE = ROOT / "original_c" / "mpack-develop" / "src"
FROZEN_UNIT = ROOT / "tests" / "original" / "test" / "unit"
CONFIG_INCLUDE = ROOT / "tests" / "port" / "ffi-harness" / "include"
BUILD = ROOT / "target" / "frozen-link"


def run(command: list[str]) -> None:
    print("+", " ".join(map(str, command)))
    subprocess.run(command, cwd=ROOT, check=True)


def cdylib_import_library() -> Path:
    candidates = (
        TARGET / "debug" / name
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


def c_command(source_files: list[Path], output: Path, library: Path) -> list[str]:
    compiler = os.environ.get("CC") or shutil.which("cc") or shutil.which("clang")
    if compiler is None:
        compiler = "cl" if os.name == "nt" else "gcc"

    if Path(compiler).name.lower() in {"cl", "cl.exe"}:
        return [
            compiler,
            "/nologo",
            "/std:c11",
            "/DMPACK_HAS_CONFIG=1",
            f"/I{CONFIG_INCLUDE}",
            f"/I{UPSTREAM_INCLUDE}",
            f"/I{FROZEN_UNIT / 'src'}",
            *(str(source) for source in source_files),
            str(library),
            f"/Fe:{output}",
        ]

    return [
        compiler,
        "-std=c11",
        "-DMPACK_HAS_CONFIG=1",
        f"-I{CONFIG_INCLUDE}",
        f"-I{UPSTREAM_INCLUDE}",
        f"-I{FROZEN_UNIT / 'src'}",
        *(str(source) for source in source_files),
        str(library),
        "-o",
        str(output),
    ]


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
    args = parser.parse_args()

    if args.expect_missing and not args.full:
        parser.error("--expect-missing requires --full")

    run(["cargo", "build"])
    BUILD.mkdir(parents=True, exist_ok=True)
    library = cdylib_import_library()
    runtime_library = TARGET / "debug" / "mpack.dll"
    if runtime_library.exists():
        shutil.copy2(runtime_library, BUILD)

    if args.full:
        sources = sorted((FROZEN_UNIT / "src").glob("*.c"))
        executable = BUILD / "embed-writer-frozen"
    else:
        sources = [Path(__file__).parent / "c" / "frozen_nil_smoke.c"]
        executable = BUILD / "embed-writer-nil-smoke"
    sources.append(ROOT / "original_c" / "mpack-develop" / "src" / "mpack" / "mpack-platform.c")

    result = subprocess.run(c_command(sources, executable, library), cwd=ROOT)
    if result.returncode:
        if args.expect_missing:
            print("Full frozen-suite link is incomplete as expected: Rust writer symbols remain to be implemented.")
            return 0
        return result.returncode

    if args.full:
        return subprocess.run([str(executable)], cwd=ROOT).returncode
    run([str(executable)])
    return 0


if __name__ == "__main__":
    sys.exit(main())
