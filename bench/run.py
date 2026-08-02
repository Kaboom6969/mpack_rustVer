#!/usr/bin/env python3
"""Fair C-vs-Rust FFI benchmark runner (1A + 2B).

Builds the same bench/c/bench_main.c against:
  A) upstream C MPack sources
  B) Rust full-suite-abi release staticlib
then runs the locked workload protocol and writes bench/results.json.

Hard gates before status=measured:
  - fixture byte-identical
  - post-link: Rust binary test_malloc/test_free resolve to libc (not noop shim)
"""

from __future__ import annotations

import json
import os
import platform
import random
import re
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
BENCH = ROOT / "bench"
BENCH_C = BENCH / "c"
BUILD = ROOT / "target" / "bench"
UPSTREAM_SRC = ROOT / "original_c" / "mpack-develop" / "src"
UPSTREAM_MPACK = UPSTREAM_SRC / "mpack"
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))

TRIALS = 5
WARMUP = 100
THROUGHPUT_ITERS = 5000
LATENCY_ITERS = 10000
# Fixed seed so interleaved C/Rust order is reproducible across runs.
INTERLEAVE_SEED = 20260802

DEFINE_LOCK = (
    "MPACK_READER=1 MPACK_WRITER=1 MPACK_EXPECT=1 MPACK_NODE=1 "
    "MPACK_COMPATIBILITY=1 MPACK_EXTENSIONS=1 MPACK_STDLIB=1 MPACK_STDIO=1 "
    "MPACK_READ_TRACKING=1 MPACK_WRITE_TRACKING=1 "
    "MPACK_BUFFER_SIZE=4096 MPACK_TRACKING_INITIAL_CAPACITY=3 "
    "MPACK_MALLOC=malloc MPACK_FREE=free"
)

OPT_LOCK = "C:-O2 -DNDEBUG -std=gnu11 -D_DEFAULT_SOURCE; Rust:cargo --release --features full-suite-abi"

MPACK_C_SOURCES = [
    UPSTREAM_MPACK / "mpack-common.c",
    UPSTREAM_MPACK / "mpack-writer.c",
    UPSTREAM_MPACK / "mpack-reader.c",
    UPSTREAM_MPACK / "mpack-expect.c",
    UPSTREAM_MPACK / "mpack-node.c",
    UPSTREAM_MPACK / "mpack-platform.c",
]

SYMBOL_RE = re.compile(r"^[0-9a-fA-F]+ <(?P<name>[^>]+)>:\s*$")


def run(command: list[str], *, env: dict[str, str] | None = None) -> None:
    print("+", " ".join(map(str, command)), flush=True)
    subprocess.run(command, cwd=ROOT, check=True, env=env)


def compiler() -> str:
    return os.environ.get("CC") or shutil.which("gcc") or shutil.which("cc") or "gcc"


def cflags() -> list[str]:
    return [
        "-std=gnu11",
        "-O2",
        "-DNDEBUG",
        "-D_DEFAULT_SOURCE",
        "-DMPACK_HAS_CONFIG=1",
        f"-I{BENCH_C}",
        f"-I{UPSTREAM_SRC}",
    ]


def native_libs() -> list[str]:
    return ["-ldl", "-lpthread", "-lm"]


def build_c_binary() -> Path:
    BUILD.mkdir(parents=True, exist_ok=True)
    out = BUILD / "mpack_bench_c"
    cmd = [
        compiler(),
        *cflags(),
        str(BENCH_C / "bench_main.c"),
        *[str(p) for p in MPACK_C_SOURCES],
        "-o",
        str(out),
        *native_libs(),
    ]
    run(cmd)
    return out


def build_rust_staticlib() -> Path:
    # No mpack_frozen_link: OWNED_BUFFER_CAPACITY stays 4096 (matches C).
    # Suite shims' leaky test_free is overridden at final link by bench_shims.c.
    # assert_rust_identity_allocators() hard-fails if the override did not win.
    run(
        [
            "cargo",
            "rustc",
            "--release",
            "--features",
            "full-suite-abi",
            "--crate-type",
            "staticlib",
        ]
    )
    lib = TARGET / "release" / "libmpack.a"
    if not lib.exists():
        raise SystemExit(f"missing staticlib at {lib}")
    return lib


def build_rust_binary(lib: Path) -> Path:
    BUILD.mkdir(parents=True, exist_ok=True)
    out = BUILD / "mpack_bench_rust"
    cmd = [
        compiler(),
        *cflags(),
        # Object files first so --allow-multiple-definition keeps our libc wrappers.
        str(BENCH_C / "bench_main.c"),
        str(BENCH_C / "bench_shims.c"),
        str(UPSTREAM_MPACK / "mpack-platform.c"),
        str(lib),
        "-o",
        str(out),
        "-Wl,--allow-multiple-definition",
        *native_libs(),
    ]
    run(cmd)
    return out


def _disassemble_symbol(binary: Path, symbol: str) -> str:
    """Return the objdump -d body for `<symbol>:` (empty if missing)."""
    objdump = shutil.which("objdump")
    if objdump is None:
        raise SystemExit("objdump required for post-link allocator gate")
    result = subprocess.run(
        [objdump, "-d", str(binary)],
        cwd=ROOT,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    lines = result.stdout.splitlines()
    body: list[str] = []
    capturing = False
    for line in lines:
        match = SYMBOL_RE.match(line)
        if match:
            if capturing:
                break
            capturing = match.group("name") == symbol
            continue
        if capturing:
            if line.strip() == "":
                # Keep going through padding; stop only on next symbol (above).
                continue
            body.append(line)
    return "\n".join(body)


def assert_rust_identity_allocators(binary: Path) -> dict:
    """Hard gate: test_malloc/test_free must call libc, not the Rust noop shim.

    The full-suite-abi staticlib embeds cargo-test suite_shims whose test_free is
    a no-op. bench_shims.c + --allow-multiple-definition must win at final link.
    If a different linker/order lets the noop win, RSS/throughput are invalid.
    """
    malloc_body = _disassemble_symbol(binary, "test_malloc")
    free_body = _disassemble_symbol(binary, "test_free")
    if not malloc_body:
        raise SystemExit(f"allocator gate: missing test_malloc in {binary}")
    if not free_body:
        raise SystemExit(f"allocator gate: missing test_free in {binary}")

    malloc_ok = ("malloc@plt" in malloc_body) or re.search(
        r"\bmalloc\b", malloc_body
    )
    free_ok = ("free@plt" in free_body) or re.search(r"\bfree\b", free_body)
    # Noop shim is essentially endbr64 + ret (no call/jmp to free).
    free_is_noop = ("ret" in free_body) and ("free" not in free_body)

    if free_is_noop or not free_ok:
        raise SystemExit(
            "allocator gate FAILED: test_free does not resolve to libc free "
            f"(disassembly:\n{free_body}\n). Refusing status=measured."
        )
    if not malloc_ok:
        raise SystemExit(
            "allocator gate FAILED: test_malloc does not resolve to libc malloc "
            f"(disassembly:\n{malloc_body}\n). Refusing status=measured."
        )

    evidence = {
        "tool": "objdump -d",
        "test_malloc_refs_libc": True,
        "test_free_refs_libc": True,
        "test_malloc_snippet": malloc_body.strip().splitlines()[:6],
        "test_free_snippet": free_body.strip().splitlines()[:6],
    }
    print(
        "allocator gate OK: test_malloc/test_free resolve to libc "
        "(not suite noop shim)",
        flush=True,
    )
    return evidence


def maybe_taskset(command: list[str]) -> tuple[list[str], bool]:
    if shutil.which("taskset") is None:
        return command, False
    return ["taskset", "-c", "0", *command], True


def capture(command: list[str]) -> subprocess.CompletedProcess[str]:
    wrapped, _ = maybe_taskset(command)
    print("+", " ".join(map(str, wrapped)), flush=True)
    return subprocess.run(
        wrapped,
        cwd=ROOT,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def dump_fixture(binary: Path, path: Path, workload: str = "dump-fixture") -> None:
    wrapped, _ = maybe_taskset([str(binary), workload])
    print("+", " ".join(map(str, wrapped)), flush=True)
    raw = subprocess.check_output(wrapped, cwd=ROOT)
    path.write_bytes(raw)


def verify_fixtures(c_bin: Path, rust_bin: Path) -> int:
    c_path = BUILD / "fixture_c.bin"
    r_path = BUILD / "fixture_rust.bin"
    dump_fixture(c_bin, c_path)
    dump_fixture(rust_bin, r_path)
    c_bytes = c_path.read_bytes()
    r_bytes = r_path.read_bytes()
    if c_bytes != r_bytes:
        raise SystemExit(
            f"fixture mismatch: C={len(c_bytes)} bytes Rust={len(r_bytes)} bytes"
        )
    print(f"fixture byte-identical ({len(c_bytes)} bytes)", flush=True)
    return len(c_bytes)


def parse_json_line(stdout: str) -> dict:
    line = stdout.strip().splitlines()[-1]
    return json.loads(line)


def median(values: list[float]) -> float:
    return float(statistics.median(values))


def run_one_metric(
    binary: Path,
    workload: str,
    *,
    iters: int,
    warmup: int,
    extra_args: list[str] | None = None,
) -> dict:
    command = [
        str(binary),
        workload,
        "--json",
        "--iters",
        str(iters),
        "--warmup",
        str(warmup),
    ]
    if extra_args:
        command.extend(extra_args)
    result = capture(command)
    return parse_json_line(result.stdout)


def interleaved_metric_trials(
    c_bin: Path,
    rust_bin: Path,
    workload: str,
    *,
    iters: int,
    warmup: int,
    rng: random.Random,
    extra_args: list[str] | None = None,
) -> tuple[list[dict], list[dict]]:
    """Run TRIALS with per-trial shuffled C/Rust order (thermal fairness)."""
    c_trials: list[dict] = []
    rust_trials: list[dict] = []
    print(f"\n=== interleaved {workload} ===", flush=True)
    for trial in range(TRIALS):
        order = ["c", "rust"]
        rng.shuffle(order)
        print(f"  trial {trial} order={order}", flush=True)
        for side in order:
            binary = c_bin if side == "c" else rust_bin
            payload = run_one_metric(
                binary,
                workload,
                iters=iters,
                warmup=warmup,
                extra_args=extra_args,
            )
            payload["trial"] = trial
            payload["side"] = side
            if side == "c":
                c_trials.append(payload)
            else:
                rust_trials.append(payload)
            print(f"    {side}: {payload}", flush=True)
    return c_trials, rust_trials


def interleaved_startup_trials(
    c_bin: Path, rust_bin: Path, rng: random.Random
) -> tuple[list[float], list[float]]:
    c_samples: list[float] = []
    rust_samples: list[float] = []
    print("\n=== interleaved startup ===", flush=True)
    for trial in range(TRIALS):
        order = ["c", "rust"]
        rng.shuffle(order)
        print(f"  trial {trial} order={order}", flush=True)
        for side in order:
            binary = c_bin if side == "c" else rust_bin
            wrapped, _ = maybe_taskset([str(binary), "startup", "--json"])
            t0 = time.perf_counter()
            subprocess.run(
                wrapped,
                cwd=ROOT,
                check=True,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.PIPE,
            )
            t1 = time.perf_counter()
            ms = (t1 - t0) * 1000.0
            if side == "c":
                c_samples.append(ms)
            else:
                rust_samples.append(ms)
            print(f"    {side}: startup_ms={ms:.4f}", flush=True)
    return c_samples, rust_samples


def fill_throughput(out: dict, trials: list[dict], prefix: str) -> None:
    out[f"{prefix}_docs_per_s"] = median([t["docs_per_s"] for t in trials])
    out[f"{prefix}_mb_per_s"] = median([t["mb_per_s"] for t in trials])
    out[f"{prefix}_trials"] = trials


def fill_latency(out: dict, trials: list[dict], prefix: str) -> None:
    out[f"{prefix}_p50_ns"] = int(median([t["p50_ns"] for t in trials]))
    out[f"{prefix}_p99_ns"] = int(median([t["p99_ns"] for t in trials]))
    out[f"{prefix}_max_ns"] = int(median([t["max_ns"] for t in trials]))
    out[f"{prefix}_latency_trials"] = trials


def summarize_interleaved(
    c_bin: Path,
    rust_bin: Path,
    *,
    large_fixture: Path,
) -> tuple[dict, dict]:
    rng = random.Random(INTERLEAVE_SEED)
    c_out: dict = {"binary": str(c_bin.relative_to(ROOT))}
    rust_out: dict = {"binary": str(rust_bin.relative_to(ROOT))}

    c_t, r_t = interleaved_metric_trials(
        c_bin, rust_bin, "encode", iters=THROUGHPUT_ITERS, warmup=WARMUP, rng=rng
    )
    fill_throughput(c_out, c_t, "encode_throughput")
    fill_throughput(rust_out, r_t, "encode_throughput")

    c_t, r_t = interleaved_metric_trials(
        c_bin,
        rust_bin,
        "decode-reader",
        iters=THROUGHPUT_ITERS,
        warmup=WARMUP,
        rng=rng,
    )
    fill_throughput(c_out, c_t, "decode_reader_throughput")
    fill_throughput(rust_out, r_t, "decode_reader_throughput")

    c_t, r_t = interleaved_metric_trials(
        c_bin,
        rust_bin,
        "decode-node",
        iters=THROUGHPUT_ITERS,
        warmup=WARMUP,
        rng=rng,
    )
    fill_throughput(c_out, c_t, "decode_node_throughput")
    fill_throughput(rust_out, r_t, "decode_node_throughput")

    c_t, r_t = interleaved_metric_trials(
        c_bin, rust_bin, "encode-latency", iters=LATENCY_ITERS, warmup=0, rng=rng
    )
    fill_latency(c_out, c_t, "encode")
    fill_latency(rust_out, r_t, "encode")

    c_t, r_t = interleaved_metric_trials(
        c_bin,
        rust_bin,
        "decode-reader-latency",
        iters=LATENCY_ITERS,
        warmup=0,
        rng=rng,
    )
    fill_latency(c_out, c_t, "decode_reader")
    fill_latency(rust_out, r_t, "decode_reader")

    c_t, r_t = interleaved_metric_trials(
        c_bin,
        rust_bin,
        "decode-node-latency",
        iters=LATENCY_ITERS,
        warmup=0,
        rng=rng,
    )
    fill_latency(c_out, c_t, "decode_node")
    fill_latency(rust_out, r_t, "decode_node")

    # Decode-only RSS: fresh process loads fixture from disk (no encode).
    c_t, r_t = interleaved_metric_trials(
        c_bin,
        rust_bin,
        "rss",
        iters=1,
        warmup=0,
        rng=rng,
        extra_args=["--fixture", str(large_fixture)],
    )
    c_out["rss_peak_bytes"] = int(median([t["peak_bytes"] for t in c_t]))
    c_out["rss_fixture_bytes"] = int(c_t[0]["fixture_bytes"])
    c_out["rss_mode"] = "decode_only"
    c_out["rss_trials"] = c_t
    rust_out["rss_peak_bytes"] = int(median([t["peak_bytes"] for t in r_t]))
    rust_out["rss_fixture_bytes"] = int(r_t[0]["fixture_bytes"])
    rust_out["rss_mode"] = "decode_only"
    rust_out["rss_trials"] = r_t

    c_s, r_s = interleaved_startup_trials(c_bin, rust_bin, rng)
    c_out["startup_ms"] = median(c_s)
    c_out["startup_trials_ms"] = c_s
    rust_out["startup_ms"] = median(r_s)
    rust_out["startup_trials_ms"] = r_s

    return c_out, rust_out


def collect_environment(
    taskset_used: bool, allocator_gate: dict
) -> dict:
    rustc = subprocess.check_output(["rustc", "-vV"], text=True).strip()
    cc = compiler()
    cc_ver = subprocess.check_output([cc, "-dumpversion"], text=True).strip()
    commit = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
    ).strip()
    cpu = platform.processor() or platform.machine()
    if Path("/proc/cpuinfo").exists():
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                cpu = line.split(":", 1)[1].strip()
                break
    return {
        "cpu": cpu,
        "os": platform.platform(),
        "rustc": rustc.splitlines()[0] if rustc else None,
        "rustc_verbose": rustc,
        "cc": f"{cc} {cc_ver}",
        "commit": commit,
        "define_lockstring": DEFINE_LOCK,
        "opt_flags": OPT_LOCK,
        "taskset_cpu0": taskset_used,
        "trials": TRIALS,
        "warmup": WARMUP,
        "throughput_iters": THROUGHPUT_ITERS,
        "latency_iters": LATENCY_ITERS,
        "interleave_seed": INTERLEAVE_SEED,
        "trial_order": "per-trial shuffled C/Rust (seeded)",
        "allocator_gate": allocator_gate,
        "rss_mode": "decode_only_fresh_process",
    }


def main() -> int:
    c_bin = build_c_binary()
    rust_lib = build_rust_staticlib()
    rust_bin = build_rust_binary(rust_lib)

    # Hard gate: refuse measured results if noop test_free won the link.
    allocator_gate = assert_rust_identity_allocators(rust_bin)

    fixture_bytes = verify_fixtures(c_bin, rust_bin)

    large_fixture = BUILD / "fixture_large.bin"
    dump_fixture(c_bin, large_fixture, workload="dump-large-fixture")
    print(
        f"large RSS fixture written ({large_fixture.stat().st_size} bytes)",
        flush=True,
    )

    _, taskset_used = maybe_taskset(["true"])

    c_reference, rust_port = summarize_interleaved(
        c_bin, rust_bin, large_fixture=large_fixture
    )

    results = {
        "status": "measured",
        "note": (
            "C ABI fair compare: identical driver; everything features; "
            "forced tracking; libc malloc (Rust test_* identity wrappers, "
            "post-link objdump gate); decode-only RSS in a fresh process "
            "(fixture pre-encoded); per-trial shuffled C/Rust order; "
            "release opts. See bench/methodology.md."
        ),
        "environment": collect_environment(taskset_used, allocator_gate),
        "fixture_bytes": fixture_bytes,
        "c_reference": c_reference,
        "rust_port": rust_port,
    }

    out_path = BENCH / "results.json"
    out_path.write_text(json.dumps(results, indent=2) + "\n")
    print(f"\nWrote {out_path}", flush=True)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except subprocess.CalledProcessError as exc:
        if exc.stderr:
            sys.stderr.write(
                exc.stderr if isinstance(exc.stderr, str) else exc.stderr.decode()
            )
        raise
