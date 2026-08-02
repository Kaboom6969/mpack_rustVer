#!/usr/bin/env python3
"""Fair C-vs-Rust FFI benchmark runner (1A + 2B).

Builds the same bench/c/bench_main.c against:
  A) upstream C MPack sources
  B) Rust full-suite-abi release staticlib
then runs the locked workload protocol and writes bench/results.json.
"""

from __future__ import annotations

import json
import os
import platform
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


def dump_fixture(binary: Path, path: Path) -> None:
    wrapped, _ = maybe_taskset([str(binary), "dump-fixture"])
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


def run_metric_trials(
    binary: Path,
    workload: str,
    *,
    iters: int,
    warmup: int,
) -> list[dict]:
    trials: list[dict] = []
    for trial in range(TRIALS):
        result = capture(
            [
                str(binary),
                workload,
                "--json",
                "--iters",
                str(iters),
                "--warmup",
                str(warmup),
            ]
        )
        payload = parse_json_line(result.stdout)
        payload["trial"] = trial
        trials.append(payload)
        print(f"  trial {trial}: {payload}", flush=True)
    return trials


def run_rss_trials(binary: Path) -> list[dict]:
    trials: list[dict] = []
    for trial in range(TRIALS):
        result = capture([str(binary), "rss", "--json"])
        payload = parse_json_line(result.stdout)
        payload["trial"] = trial
        trials.append(payload)
        print(f"  trial {trial}: {payload}", flush=True)
    return trials


def run_startup_trials(binary: Path) -> list[float]:
    samples: list[float] = []
    for trial in range(TRIALS):
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
        samples.append(ms)
        print(f"  trial {trial}: startup_ms={ms:.4f}", flush=True)
    return samples


def summarize_side(
    name: str,
    binary: Path,
) -> dict:
    print(f"\n=== {name} ({binary.name}) ===", flush=True)
    out: dict = {"binary": str(binary.relative_to(ROOT))}

    encode = run_metric_trials(
        binary, "encode", iters=THROUGHPUT_ITERS, warmup=WARMUP
    )
    out["encode_throughput_docs_per_s"] = median(
        [t["docs_per_s"] for t in encode]
    )
    out["encode_throughput_mb_per_s"] = median([t["mb_per_s"] for t in encode])
    out["encode_throughput_trials"] = encode

    dec_r = run_metric_trials(
        binary, "decode-reader", iters=THROUGHPUT_ITERS, warmup=WARMUP
    )
    out["decode_reader_throughput_docs_per_s"] = median(
        [t["docs_per_s"] for t in dec_r]
    )
    out["decode_reader_throughput_mb_per_s"] = median(
        [t["mb_per_s"] for t in dec_r]
    )
    out["decode_reader_throughput_trials"] = dec_r

    dec_n = run_metric_trials(
        binary, "decode-node", iters=THROUGHPUT_ITERS, warmup=WARMUP
    )
    out["decode_node_throughput_docs_per_s"] = median(
        [t["docs_per_s"] for t in dec_n]
    )
    out["decode_node_throughput_mb_per_s"] = median(
        [t["mb_per_s"] for t in dec_n]
    )
    out["decode_node_throughput_trials"] = dec_n

    enc_lat = run_metric_trials(
        binary, "encode-latency", iters=LATENCY_ITERS, warmup=0
    )
    out["encode_p50_ns"] = int(median([t["p50_ns"] for t in enc_lat]))
    out["encode_p99_ns"] = int(median([t["p99_ns"] for t in enc_lat]))
    out["encode_max_ns"] = int(median([t["max_ns"] for t in enc_lat]))
    out["encode_latency_trials"] = enc_lat

    dec_r_lat = run_metric_trials(
        binary, "decode-reader-latency", iters=LATENCY_ITERS, warmup=0
    )
    out["decode_reader_p50_ns"] = int(median([t["p50_ns"] for t in dec_r_lat]))
    out["decode_reader_p99_ns"] = int(median([t["p99_ns"] for t in dec_r_lat]))
    out["decode_reader_max_ns"] = int(median([t["max_ns"] for t in dec_r_lat]))
    out["decode_reader_latency_trials"] = dec_r_lat

    dec_n_lat = run_metric_trials(
        binary, "decode-node-latency", iters=LATENCY_ITERS, warmup=0
    )
    out["decode_node_p50_ns"] = int(median([t["p50_ns"] for t in dec_n_lat]))
    out["decode_node_p99_ns"] = int(median([t["p99_ns"] for t in dec_n_lat]))
    out["decode_node_max_ns"] = int(median([t["max_ns"] for t in dec_n_lat]))
    out["decode_node_latency_trials"] = dec_n_lat

    rss = run_rss_trials(binary)
    out["rss_peak_bytes"] = int(median([t["peak_bytes"] for t in rss]))
    out["rss_fixture_bytes"] = int(rss[0]["fixture_bytes"])
    out["rss_trials"] = rss

    startup = run_startup_trials(binary)
    out["startup_ms"] = median(startup)
    out["startup_trials_ms"] = startup

    return out


def collect_environment(taskset_used: bool) -> dict:
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
    }


def main() -> int:
    c_bin = build_c_binary()
    rust_lib = build_rust_staticlib()
    rust_bin = build_rust_binary(rust_lib)

    fixture_bytes = verify_fixtures(c_bin, rust_bin)
    _, taskset_used = maybe_taskset(["true"])

    results = {
        "status": "measured",
        "note": (
            "C ABI fair compare: identical driver; everything features; "
            "forced tracking; libc malloc (Rust test_* identity wrappers); "
            "release opts. See bench/methodology.md."
        ),
        "environment": collect_environment(taskset_used),
        "fixture_bytes": fixture_bytes,
        "c_reference": summarize_side("c_reference", c_bin),
        "rust_port": summarize_side("rust_port", rust_bin),
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
            sys.stderr.write(exc.stderr if isinstance(exc.stderr, str) else exc.stderr.decode())
        raise
