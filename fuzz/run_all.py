#!/usr/bin/env python3
"""Run every Port Mortem fuzz target sequentially.

Requires Linux/WSL with nightly Rust and cargo-fuzz (see fuzz/README.md).

Targets (in order):
  fuzz/:       reader_diff, node_diff, total_diff, expect_diff, writer_diff
  fuzz_ffi/:   ffi_crash  (runs with -detect_leaks=0; suite test_free is a noop)

Examples:
  python3 fuzz/run_all.py
  python3 fuzz/run_all.py --seconds 10
  python3 fuzz/run_all.py --seconds 0 --runs 100
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

TARGETS = [
    ("fuzz", "reader_diff"),
    ("fuzz", "node_diff"),
    ("fuzz", "total_diff"),
    ("fuzz", "expect_diff"),
    ("fuzz", "writer_diff"),
    ("fuzz_ffi", "ffi_crash"),
]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--seconds",
        type=int,
        default=60,
        help="libFuzzer -max_total_time per target (default: 60; 0 disables)",
    )
    parser.add_argument(
        "--max-len",
        type=int,
        default=65536,
        help="libFuzzer -max_len (default: 65536)",
    )
    parser.add_argument(
        "--runs",
        type=int,
        default=None,
        help="optional libFuzzer -runs override (in addition to time budget)",
    )
    args = parser.parse_args()

    if shutil.which("cargo") is None:
        print("cargo not found on PATH", file=sys.stderr)
        return 1

    env = os.environ.copy()
    env.setdefault("CXX", "g++")
    rustflags = env.get("RUSTFLAGS", "")
    if "-C linker=g++" not in rustflags:
        env["RUSTFLAGS"] = (rustflags + " -C linker=g++").strip()

    results: list[tuple[str, str, int]] = []
    for fuzz_dir, name in TARGETS:
        fuzz_path = ROOT / fuzz_dir
        cmd = [
            "cargo",
            "+nightly",
            "fuzz",
            "run",
            name,
            "--fuzz-dir",
            str(fuzz_path),
            "--",
            f"-max_len={args.max_len}",
        ]
        if args.seconds > 0:
            cmd.append(f"-max_total_time={args.seconds}")
        if args.runs is not None:
            cmd.append(f"-runs={args.runs}")
        # Prefer env ASAN_OPTIONS for LSan; libFuzzer -detect_leaks=0 alone
        # still prints end-of-run leak summaries under AddressSanitizer.
        if name == "ffi_crash":
            cmd.append("-detect_leaks=0")
            asan = env.get("ASAN_OPTIONS", "")
            if "detect_leaks=" not in asan:
                env["ASAN_OPTIONS"] = (
                    (asan + ":detect_leaks=0").strip(':') if asan else "detect_leaks=0"
                )

        print("=" * 72)
        print(f"+ {' '.join(cmd)}")
        print("=" * 72, flush=True)
        completed = subprocess.run(cmd, cwd=ROOT, env=env)
        results.append((fuzz_dir, name, completed.returncode))
        if completed.returncode != 0:
            print(
                f"FAIL {fuzz_dir}/{name} exit={completed.returncode}",
                file=sys.stderr,
            )

    print("\nFuzz run-all summary:")
    failed = 0
    for fuzz_dir, name, code in results:
        status = "ok" if code == 0 else f"FAIL({code})"
        print(f"  {fuzz_dir}/{name}: {status}")
        if code != 0:
            failed += 1
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
