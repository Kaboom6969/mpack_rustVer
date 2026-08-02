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
| `fuzz/` | Differential fuzz (C oracle vs Rust safe core; WSL/Linux + cargo-fuzz) |
| `bench/` | Benchmark methodology and results |
| `.port-mortem.toml` | Track, source URL, kickoff hashes |
| `Dockerfile` | One-command buildable artifact |

## Build (Rust port)

```bash
cargo build
cargo test
```

Rust tests live under `tests/port/` only. Do not modify `tests/original/`.

### C-to-Rust FFI slice

The first vertical slice implements fixed-buffer nil encoding for the upstream
`embed-writer` configuration:

```bash
cargo test --manifest-path tests/port/ffi-harness/Cargo.toml
```

This command uses the platform C compiler through Cargo's `cc` build dependency.
The test path is:

```text
Rust test runner -> C harness -> MPack C ABI -> safe Rust writer
```

The harness compiles the complete upstream MPack header chain with its own
explicit `mpack-config.h`. It checks the C/Rust writer layout, header-inline
accessors, `nil -> 0xc0`, sticky capacity errors, null-pointer hardening, and
panic containment. The root build only compiles the C harness when the
`ffi-harness` feature is enabled.

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

## Differential fuzz (WSL / Linux)

Compare original C MPack against the Rust safe core with cargo-fuzz:

```bash
rustup toolchain install nightly
cargo +nightly install cargo-fuzz
# If default c++ is Clang, prefer: CXX=g++ RUSTFLAGS="-C linker=g++"
cargo +nightly fuzz run reader_diff --fuzz-dir fuzz -- -max_len=65536
cargo +nightly fuzz run node_diff --fuzz-dir fuzz -- -max_len=65536
```

See [`fuzz/README.md`](fuzz/README.md) for oracle details, symbol policy,
linker pitfalls, and corpus layout.

## Status

The first safe-core-to-C-ABI vertical slice is complete for fixed-buffer nil
encoding under `embed-writer`. Other writer operations, callbacks, allocation,
reader/expect/node, and full frozen-suite parity remain to be implemented. See
[`DECISIONS.md`](DECISIONS.md) for the exact ABI boundary and documented
divergences.
