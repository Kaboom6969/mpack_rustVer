# AGENTS.md

## Cursor Cloud specific instructions

This repo is a **Port Mortem** project: porting the C **MPack** MessagePack library
from the pinned upstream kickoff commit recorded in `.port-mortem.toml` to Rust
(`src/`, `Cargo.toml`) via a C-ABI FFI layer.
The frozen C unit suite lives under `tests/original/` (do not edit it; see
`.cursor/rules/port-mortem-core.mdc`). Divergences go in `DECISIONS.md`.

### Toolchain (pre-installed in the VM snapshot)
- Rust `cargo`/`rustc` (for the Rust port), C toolchain (`gcc`, `g++`, `clang`),
  `python3`, and `ninja` are already installed. The update script runs
  `cargo fetch` when `Cargo.toml` exists.

### Building / running the reference C library + unit suite
- Differential fuzzing and fair benchmarks fetch the pinned upstream MPack
  checkout into `target/upstream/mpack/pinned/` via
  `tools/upstream_mpack.py`.
- **Use `CC=gcc`.** The default `cc` is Clang 18, which compiles the pure-C variants
  fine but cannot locate the libstdc++ C++ headers (`<limits>` "file not found"), so
  the `c++11` variant in the `more`/`all` targets fails under Clang. `gcc` builds
  every variant.
- The `tools/*.sh` helper scripts are not marked executable; invoke them via an
  interpreter (e.g. `python3 test/unit/configure.py`, `sh tools/unit.sh ...`) or call
  the tools directly.
- Configure + run (from the fetched pinned upstream checkout):
  - `python3 tools/upstream_mpack.py ensure`  → prints the fetched `src/` path
  - from `target/upstream/mpack/pinned/`, run:
    - `CC=gcc python3 test/unit/configure.py`  → generates `.build/unit/build.ninja`
    - `CC=gcc ninja -f .build/unit/build.ninja more`  → builds + runs the CI "more" set
    (default/everything/embed/no-float/gnu89/c++11/lto). Other targets: a single
    `run-<config>` (e.g. `run-everything-debug`), `all`, or `help`.
- A passing run prints `Unit testing complete. 0 failures in <N> checks.` per variant.

### Rust port
- Standard `cargo build` / `cargo test` / `cargo clippy` at the repo root. New Rust
  tests go in `tests/port/` only (never edit the frozen suite in `tests/original/`).
- Safe core modules stay `forbid(unsafe_code)`; raw pointers live only under
  `src/ffi/`. Dual ABI: default embed-writer vs Cargo feature `full-suite-abi`
  (mutually exclusive) — see `DECISIONS.md`.
- The FFI layer must match the MPack C ABI so the frozen C suite links unchanged;
  see `.cursor/rules/mpack-architecture.mdc` and `mpack-parity.mdc`.
- Parallel lanes and **per-module frozen-suite gates** are in
  `.cursor/rules/team-ownership.mdc` (also mirrored under `.trae/rules/`).
  Teammate safe-core “done” ≠ lane “done”: a lane is done only when its matching
  `test-*.c` has 0 failures under the documented `frozen-link` command—not from
  `tests/port/` alone. How-to: `tests/port/frozen-link/README.md`.
