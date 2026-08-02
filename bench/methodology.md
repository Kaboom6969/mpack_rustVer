# Benchmark methodology

Fair comparison of upstream C MPack (`original_c/mpack-develop/`) against the
Rust port’s **C ABI** (`full-suite-abi` staticlib) on identical MessagePack
workloads.

## Locked fairness contract (1A + 2B)

| Rule | Choice |
| --- | --- |
| Comparison surface | **1A**: one C driver (`bench/c/bench_main.c`); link **A** = upstream C `.c` objects, **B** = Rust `staticlib` FFI |
| Feature surface | **2B**: reader + writer + expect + node + compatibility + extensions + stdlib + stdio |
| Tracking | Forced **ON** both sides (`MPACK_READ_TRACKING=1`, `MPACK_WRITE_TRACKING=1`) even under `NDEBUG` (upstream `everything-release` would disable tracking) |
| Allocator | libc `malloc` / `free`. Rust `full-suite-abi` still resolves symbols named `test_malloc` / `test_free`; the harness provides **identity wrappers** over libc (not suite fail-injection hooks) |
| Buffer / track constants | `MPACK_BUFFER_SIZE=4096`, `MPACK_TRACKING_INITIAL_CAPACITY=3` (matches Rust `full-suite-abi` without `mpack_frozen_link`) |
| Opt | C: `-O2 -DNDEBUG -std=gnu11`; Rust: `cargo rustc --release --features full-suite-abi --crate-type staticlib` |
| Not measured | Safe-core Rust API, embed-writer layout, frozen-link DEBUG builds, Valgrind |

Sanity gate before timing: both binaries must emit **byte-identical** encode
fixtures.

## Workloads

1. **Encode throughput** — growable writer; fixed nested map/array (ints, strings, bins); docs/s and MB/s after warm-up.
2. **Decode throughput** — same fixture via **reader** (`mpack_discard`) and via **node/tree**; docs/s and MB/s.
3. **p99 latency** — ≥10k single-document encode / decode-reader / decode-node; report p50 / p99 / max.
4. **RSS** — peak resident set while decoding a large (~16 MiB) MessagePack blob.
5. **Startup** — cold process: wall time to first successful nil encode (no warm-up; measured by `bench/run.py`).

## Procedure

```bash
python3 bench/run.py
```

- Prefer `taskset -c 0` when available (recorded if unavailable).
- Throughput: warm-up then timed region; each metric ≥5 trials; store **median** and raw trials.
- Record environment in `results.json` (`cpu`, `os`, `rustc`, `cc`, commit, define lockstring).
- Set `"status": "measured"` only after a successful run. Do not treat null / placeholder metrics as parity.

## Out of scope

Safe-core-only timings, production-no-tracking matrix, vendoring
`schemaless-benchmarks`, CI perf jobs.
