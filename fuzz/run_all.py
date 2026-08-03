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
import json
import os
import re
import shutil
import subprocess
import sys
from datetime import datetime, timezone
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


def collect_env_info(args: argparse.Namespace, env: dict[str, str]) -> list[str]:
    lines = [
        "=== Fuzz Log ===",
        f"Timestamp: {datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M:%S UTC')}",
        f"Platform: {sys.platform} (os.name={os.name})",
    ]

    try:
        git_commit = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True, cwd=ROOT)
        if git_commit.returncode == 0:
            lines.append(f"Git Commit: {git_commit.stdout.strip()}")
        git_branch = subprocess.run(["git", "rev-parse", "--abbrev-ref", "HEAD"], capture_output=True, text=True, cwd=ROOT)
        if git_branch.returncode == 0:
            lines.append(f"Git Branch: {git_branch.stdout.strip()}")
    except Exception:
        pass

    try:
        rustc_proc = subprocess.run(["rustc", "+nightly", "-vV"], capture_output=True, text=True, cwd=ROOT)
        if rustc_proc.returncode == 0:
            lines.append("Rustc Nightly Info:")
            for line in rustc_proc.stdout.strip().splitlines():
                lines.append(f"  {line}")
    except Exception:
        pass

    try:
        cf_proc = subprocess.run(["cargo", "+nightly", "fuzz", "--version"], capture_output=True, text=True, cwd=ROOT)
        if cf_proc.returncode == 0:
            lines.append(f"Cargo Fuzz Version: {cf_proc.stdout.strip()}")
    except Exception:
        pass

    lines.append("Compiler & Linker Environment:")
    lines.append(f"  CXX: {env.get('CXX', '')}")
    lines.append(f"  CC: {env.get('CC', '')}")
    lines.append(f"  RUSTFLAGS: {env.get('RUSTFLAGS', '')}")

    meta_path = ROOT / "target" / "upstream" / "mpack" / "pinned" / ".resolved.json"
    if meta_path.exists():
        try:
            meta = json.loads(meta_path.read_text(encoding="utf-8"))
            lines.append("Upstream MPack C Oracle (Pinned):")
            lines.append(f"  Source URL: {meta.get('source_url', '')}")
            lines.append(f"  Source Version: {meta.get('source_version', '')}")
            lines.append(f"  Kickoff Hash: {meta.get('kickoff_hash', '')}")
            lines.append(f"  Resolution Kind: {meta.get('resolution_kind', '')}")
            lines.append(f"  Resolved Commit: {meta.get('resolved_commit', '')}")
        except Exception:
            pass

    lines.append("Fuzz Configuration Parameters:")
    lines.append(f"  --seconds (max_total_time): {args.seconds}")
    lines.append(f"  --max-len (max_len): {args.max_len}")
    lines.append(f"  --runs: {args.runs if args.runs is not None else 'None (unlimited / time-based)'}")
    lines.append("Targets To Run:")
    for fuzz_dir, name, kind in TARGETS:
        lines.append(f"  - {fuzz_dir}/{name} (mode: {kind})")

    lines.append("")
    return lines


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

    env_info = collect_env_info(args, env)
    log_chunks: list[str] = ["\n".join(env_info) + "\n"]

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

        divider = "=" * 72 + "\n" + f"+ {' '.join(cmd)}\n" + "=" * 72 + "\n"
        print(divider, end="", flush=True)
        log_chunks.append(divider)

        code, output = run_streaming(cmd, env)
        log_chunks.append(output)
        if not output.endswith("\n"):
            log_chunks.append("\n")

        runs = parse_done_runs(output)
        results.append((fuzz_dir, name, kind, code, runs))
        if code != 0:
            print(
                f"FAIL {fuzz_dir}/{name} exit={code}",
                file=sys.stderr,
            )

    summary_header = "\n========================================================================\n"
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

    summary_text = summary_header + "\n".join(summary_lines) + "\n"
    log_chunks.append(summary_text)

    full_log = "".join(log_chunks)

    log_paths = [ROOT / "fuzz" / "fuzz_summary_auto.txt", ROOT / "fuzz_log.txt"]
    for path in log_paths:
        try:
            with open(path, "w", encoding="utf-8") as f:
                f.write(full_log)
            print(f"\nWrote full fuzz log and summary to {path}", flush=True)
        except Exception as e:
            print(f"Warning: failed to write log to {path}: {e}")

    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
