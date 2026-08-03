# MPack → Rust (Port Mortem 2026)

C → Rust port of [MPack](https://github.com/ludocode/mpack). **The library
implementation is Rust** under `src/` (safe core + C-ABI FFI). The frozen C unit
suite in `tests/original/` links against that FFI unchanged.

GitHub language bars overweight C because of the frozen suite and vendored ABI
headers (`include/upstream/`), not because codecs ship as C sources. Non-trivial
divergences live in [`DECISIONS.md`](DECISIONS.md).

## Repository layout

| Path | Role |
| --- | --- |
| `src/` | Safe core (`common` / `writer` / `reader` / `expect` / `node`) + `src/ffi/` |
| `tests/original/` | Frozen C unit suite (**do not edit**) |
| `tests/port/` | Rust tests; `frozen-link/` links the suite to the Rust library |
| `include/upstream/` | Vendored MPack headers + `mpack-platform.c` for frozen-link |
| `fuzz/` | Differential fuzz (C oracle vs safe core; WSL/Linux + cargo-fuzz) |
| `bench/` | Fair C↔Rust FFI benchmarks |
| `tools/upstream_mpack.py` | Fetch / path / cleanup for the pinned upstream checkout |
| `.port-mortem.toml` | Source URL, version, kickoff hashes |
| `Dockerfile` | Build + `cargo test` smoke image |

Differential fuzz and fair benchmarks resolve upstream from `.port-mortem.toml`
into `target/upstream/mpack/pinned/` (`kickoff_hash`, with `source_version` tag
fallback). There is no tracked `original_c/` tree.

## Acceptance gates

Green means the C harness prints `0 failures` and exits 0. The runner forwards
that result; it does not rewrite failures into success.

```bash
# Writer lane (embed-writer layout)
python3 tests/port/frozen-link/run.py --embed-writer

# Reader / Expect / Node (+ related) under everything + full-suite-abi
python3 tests/port/frozen-link/run.py --everything
```

Running frozen-link **without** a suite flag only builds the nil smoke probe —
that is **not** a run of `tests/original/`. Details:
[`tests/port/frozen-link/README.md`](tests/port/frozen-link/README.md).

## Build and test

```bash
cargo build
cargo test
```

New Rust tests go under `tests/port/` only. Optional layout / ABI harness:

```bash
cargo test --manifest-path tests/port/ffi-harness/Cargo.toml
```

```bash
docker build -t mpack-rust .
docker run --rm mpack-rust
```

## Differential fuzz (WSL / Linux)

Requires nightly + cargo-fuzz and a C++ toolchain (`g++`).

```bash
rustup toolchain install nightly
cargo +nightly install cargo-fuzz
python3 fuzz/run_all.py --seconds 60
# optional cleanup after review:
py -3 tools/upstream_mpack.py cleanup
```

See [`fuzz/README.md`](fuzz/README.md).

## Fair benchmarks

```bash
python3 bench/run.py
```

Methodology and results: [`bench/methodology.md`](bench/methodology.md),
[`bench/results.json`](bench/results.json).

## Status

Writer, reader, expect, and node safe core plus C-ABI FFI are in place. Frozen
parity gates (`--embed-writer` / `--everything`) are the acceptance proof.
