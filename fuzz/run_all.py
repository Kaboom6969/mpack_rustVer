#!/usr/bin/env python3
"""Run every Port Mortem fuzz target sequentially.

Requires Linux/WSL with nightly Rust and cargo-fuzz (see fuzz/README.md).

Targets (in order):
  fuzz/:       reader_diff, node_diff, total_diff, expect_diff, writer_diff
  fuzz_ffi/:   ffi_crash  (crash-only smoke; LSan on — fuzzing cfg pairs calloc/free)

Exit 0 from a *diff* target means every executed input matched C vs Rust digests
(mismatch panics → non-zero). The summary prints parsed run counts so "ok" is
not just a bare exit code.

Examples:
  python3 fuzz/run_all.py
  python3 fuzz/run_all.py --seconds 10
  python3 fuzz/run_all.py --seconds 0 --runs 100
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / "tools" / "upstream_mpack.py"

# (fuzz_dir, name, kind) — kind is "diff" (C↔Rust) or "crash" (smoke only).
TARGETS = [
    ("fuzz", "reader_diff", "diff"),
    ("fuzz", "node_diff", "diff"),
    ("fuzz", "total_diff", "diff"),
    ("fuzz", "expect_diff", "diff"),
    ("fuzz", "writer_diff", "diff"),
    ("fuzz_ffi", "ffi_crash", "crash"),
]

DONE_RUNS_RE = re.compile(r"Done\s+(\d+)\s+runs\b")


def run_helper(command: str) -> str:
    result = subprocess.run(
        [sys.executable, str(HELPER), command],
        cwd=ROOT,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return result.stdout.strip()


def using_msvc_host() -> bool:
    rustc = subprocess.run(
        ["rustc", "-vV"],
        cwd=ROOT,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return "host: x86_64-pc-windows-msvc" in rustc.stdout


def ensure_supported_host() -> None:
    if using_msvc_host():
        raise SystemExit(
            "fuzz/run_all.py requires Linux or WSL in this workspace; "
            "Windows MSVC cargo-fuzz builds are missing the ASAN runtime "
            "needed by libFuzzer."
        )


def run_streaming(cmd: list[str], env: dict[str, str]) -> tuple[int, str]:
    """Run command, stream stdout/stderr live, return (exit_code, combined text)."""
    proc = subprocess.Popen(
        cmd,
        cwd=ROOT,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        bufsize=1,
    )
    chunks: list[str] = []
    assert proc.stdout is not None
    for line in proc.stdout:
        sys.stdout.write(line)
        sys.stdout.flush()
        chunks.append(line)
    returncode = proc.wait()
    return returncode, "".join(chunks)


def parse_done_runs(output: str) -> int | None:
    matches = DONE_RUNS_RE.findall(output)
    if not matches:
        return None
    return int(matches[-1])


def format_status(kind: str, code: int, runs: int | None) -> str:
    if code != 0:
        runs_note = f", libFuzzer reported {runs} runs" if runs is not None else ""
        return f"FAIL(exit={code}{runs_note})"
    runs_s = str(runs) if runs is not None else "?"
    if kind == "diff":
        # Differential targets panic on digest mismatch; exit 0 ⇒ 0 divergences.
        return (
            f"ok — {runs_s} inputs compared, 0 divergences "
            f"(any C↔Rust mismatch panics → FAIL)"
        )
    return (
        f"ok — {runs_s} runs, 0 crashes "
        f"(crash-smoke only; not C↔Rust parity)"
    )


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

    ensure_supported_host()

    env = os.environ.copy()
    env.setdefault("CXX", "g++")
    rustflags = env.get("RUSTFLAGS", "")
    if "-C linker=g++" not in rustflags:
        env["RUSTFLAGS"] = (rustflags + " -C linker=g++").strip()

    run_helper("ensure")

    results: list[tuple[str, str, str, int, int | None]] = []
    for fuzz_dir, name, kind in TARGETS:
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

        print("=" * 72)
        print(f"+ {' '.join(cmd)}")
        print("=" * 72, flush=True)
        code, output = run_streaming(cmd, env)
        runs = parse_done_runs(output)
        results.append((fuzz_dir, name, kind, code, runs))
        if code != 0:
            print(
                f"FAIL {fuzz_dir}/{name} exit={code}",
                file=sys.stderr,
            )

    print("\nFuzz run-all summary:")
    print(
        "  (diff targets: each input compares C oracle vs Rust digests; "
        "mismatch → panic → FAIL)"
    )
    
    summary_lines = [
        "Fuzz run-all summary:",
        "  (diff targets: each input compares C oracle vs Rust digests; mismatch → panic → FAIL)"
    ]
    
    failed = 0
    for fuzz_dir, name, kind, code, runs in results:
        line = f"  {fuzz_dir}/{name}: {format_status(kind, code, runs)}"
        print(line)
        summary_lines.append(line)
        if code != 0:
            failed += 1
            
    log_path = ROOT / "fuzz" / "fuzz_summary_auto.txt"
    try:
        with open(log_path, "w", encoding="utf-8") as f:
            f.write("\n".join(summary_lines) + "\n")
        print(f"\nWrote summary to {log_path}", flush=True)
    except Exception as e:
        print(f"Warning: failed to write summary to {log_path}: {e}")
        
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
