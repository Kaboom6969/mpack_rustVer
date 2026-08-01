# MPack → Rust (Port Mortem 2026)

C → Rust port of [MPack](https://github.com/ludocode/mpack) (MessagePack C library)
from `original_c/mpack-develop/` into idiomatic Rust under `src/`, with a C-ABI FFI
layer so the frozen unit suite in `tests/original/` can link unchanged.

## Why this migration

- Keep MPack’s MessagePack encode/decode behavior and sticky-error model.
- Express the core in safe Rust (`Result`, enums, owned buffers) while exposing the
  original C ABI for the frozen test suite.
- Document every non-trivial divergence in [`DECISIONS.md`](DECISIONS.md).

## Repository layout

| Path | Role |
| --- | --- |
| `src/` | Idiomatic Rust port (safe core + eventual FFI) |
| `original_c/` | Reference C sources (unchanged) |
| `tests/original/` | Frozen C unit suite (do not edit) |
| `tests/port/` | New Rust-side tests |
| `fuzz/` | Differential fuzzer template |
| `bench/` | Benchmark methodology and results |
| `.port-mortem.toml` | Track, source URL, kickoff hashes |
| `Dockerfile` | One-command buildable artifact |

## Build (Rust port)

```bash
cargo build
cargo test
```

Rust tests live under `tests/port/` only. Do not modify `tests/original/`.

## Build (C reference suite)

From `original_c/mpack-develop/` (use `CC=gcc`):

```bash
CC=gcc python3 test/unit/configure.py
CC=gcc ninja -f .build/unit/build.ninja more
```

A passing run prints `Unit testing complete. 0 failures in <N> checks.` per variant.

## Docker (single command)

```bash
docker build -t mpack-rust .
docker run --rm mpack-rust
```

The image builds the crate and runs `cargo test` as the runnable smoke artifact.

## Status

Scaffold stage: crate modules mirror C (`common` → `writer`/`reader` → `expect` →
`node`). Full encode/decode parity and C-ABI FFI are still to be implemented.
See [`DECISIONS.md`](DECISIONS.md) as divergences appear.
