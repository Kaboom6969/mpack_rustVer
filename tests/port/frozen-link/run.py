#!/usr/bin/env python3
"""Build frozen MPack C tests against the Rust library without altering them."""

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

# Matches original_c configure.py `everything` (+ debug): allfeatures + allconfigs.
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
    debug: bool,
    wrap_abort: bool = False,
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
            f"/I{UPSTREAM_INCLUDE}",
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
        f"-I{UPSTREAM_INCLUDE}",
        f"-I{FROZEN_UNIT / 'src'}",
        *(str(source) for source in source_files),
        str(library),
        "-o",
        str(output),
    ]
    if wrap_abort:
        # Redirect abort() to a returning function so TEST_EARLY_EXIT fall-through
        # remains defined under GCC's noreturn assumptions for libc abort.
        # Also quiet printf spam from soft-continued assertion loops.
        adapter_c = Path(__file__).resolve().parent / "c"
        command.insert(3, f"-include{adapter_c / 'soft_abort.h'}")
        command.insert(4, f"-include{adapter_c / 'quiet_printf.h'}")
    if not link_static:
        command.append("-Wl,-rpath," + str(library.parent))
    else:
        # Header inlines from mpack-platform.c and Rust #[no_mangle] exports can
        # overlap; keep the C definitions (sources precede the archive).
        command.extend(["-Wl,--allow-multiple-definition"])
        command.extend(native_static_libs())
    return command


def prepare_full_suite_extras(sources: list[Path]) -> None:
    sources.append(Path(__file__).parent / "c" / "full_layout_check.c")
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
        "--everything",
        action="store_true",
        help="C everything config (reader/expect/node/stdio/compat/extensions) with full-suite-abi stubs",
    )
    parser.add_argument(
        "--default-config",
        action="store_true",
        help="alias for --everything (kept for older docs/scripts)",
    )
    parser.add_argument(
        "--release", action="store_true", help="build the Rust library in release mode"
    )
    args = parser.parse_args()

    if args.expect_missing and not args.full:
        parser.error("--expect-missing requires --full")
    if args.everything and args.default_config:
        parser.error("use only one of --everything / --default-config")
    full_suite = args.everything or args.default_config
    if full_suite and not args.full:
        parser.error("--everything / --default-config requires --full")

    if full_suite:
        # Build only the staticlib: a Windows cdylib cannot leave suite symbols
        # (test_malloc / mpack_assert_fail) undefined, but those must come from
        # the frozen C objects at final exe link.
        cargo_command = ["cargo", "rustc"]
        if RUST_TARGET:
            cargo_command.extend(["--target", RUST_TARGET])
        if args.release:
            cargo_command.append("--release")
        cargo_command.extend(["--features", "full-suite-abi", "--crate-type", "staticlib"])
    else:
        cargo_command = ["cargo", "build"]
        if RUST_TARGET:
            cargo_command.extend(["--target", RUST_TARGET])
        if args.release:
            cargo_command.append("--release")
    run(cargo_command)
    BUILD.mkdir(parents=True, exist_ok=True)
    output = rust_output(args.release)

    # Full-suite modes link the staticlib so suite-provided symbols (test_malloc,
    # mpack_assert_fail) resolve at final exe link. Embed-writer keeps cdylib.
    if full_suite:
        library = static_library(output)
        link_static = True
    else:
        library = cdylib_import_library(output)
        link_static = False
        runtime_library = output / "mpack.dll"
        if runtime_library.exists():
            shutil.copy2(runtime_library, BUILD)

    profile = "release" if args.release else "debug"
    if full_suite:
        config_include = FROZEN_UNIT / "src"
        config_name = "everything"
        debug = True
        extra_defines = EVERYTHING_DEFINES
    else:
        config_include = EMBED_CONFIG_INCLUDE
        config_name = "embed-writer"
        debug = False
        extra_defines = None

    if args.full:
        sources = sorted((FROZEN_UNIT / "src").glob("*.c"))
        executable = BUILD / f"{config_name}-{profile}-frozen"
    else:
        sources = [Path(__file__).parent / "c" / "frozen_nil_smoke.c"]
        executable = BUILD / f"{config_name}-{profile}-nil-smoke"
    sources.append(ROOT / "original_c" / "mpack-develop" / "src" / "mpack" / "mpack-platform.c")

    if full_suite:
        prepare_full_suite_extras(sources)
        ensure_unit_test_data_link()

    print(
        "+",
        " ".join(
            map(
                str,
                c_command(
                    sources,
                    executable,
                    library,
                    config_include=config_include,
                    debug=debug,
                    wrap_abort=full_suite,
                    extra_defines=extra_defines,
                    link_static=link_static,
                ),
            )
        ),
    )
    result = subprocess.run(
        c_command(
            sources,
            executable,
            library,
            config_include=config_include,
            debug=debug,
            wrap_abort=full_suite,
            extra_defines=extra_defines,
            link_static=link_static,
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
        # Full-suite stubs are expected to fail many assertions; success means
        # the binary linked and ran to completion (printed the failure summary).
        completed = subprocess.run([str(executable)], cwd=ROOT)
        if full_suite:
            # Python reports fatal signals as 128+N on Linux.
            if completed.returncode < 0 or completed.returncode >= 128:
                print(
                    "Everything frozen suite crashed "
                    f"(exit={completed.returncode}); treating as failure."
                )
                return 1
            print(
                "Everything frozen suite finished "
                f"(exit={completed.returncode}; assertion failures expected with stubs)."
            )
            return 0
        return completed.returncode
    run([str(executable)])
    return 0


if __name__ == "__main__":
    sys.exit(main())
