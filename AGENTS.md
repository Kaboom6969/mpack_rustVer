# AGENTS.md

## Cursor Cloud specific instructions

This repo is a **Port Mortem** project: porting the C **MPack** MessagePack library
(`original_c/mpack-develop/`) to Rust (`src/`, `Cargo.toml`) via a C-ABI FFI layer.
The frozen C unit suite lives under `tests/original/` (do not edit it; see
`.cursor/rules/port-mortem-core.mdc`). Divergences go in `DECISIONS.md`.

### Toolchain (pre-installed in the VM snapshot)
- Rust `cargo`/`rustc` (for the Rust port), C toolchain (`gcc`, `g++`, `clang`),
  `python3`, and `ninja` are already installed. The update script runs
  `cargo fetch` when `Cargo.toml` exists.

### Building / running the reference C library + unit suite
- The C reference implementation and its unit suite live under
  `original_c/mpack-develop/`.
- **Use `CC=gcc`.** The default `cc` is Clang 18, which compiles the pure-C variants
  fine but cannot locate the libstdc++ C++ headers (`<limits>` "file not found"), so
  the `c++11` variant in the `more`/`all` targets fails under Clang. `gcc` builds
  every variant.
- The `tools/*.sh` helper scripts are not marked executable; invoke them via an
  interpreter (e.g. `python3 test/unit/configure.py`, `sh tools/unit.sh ...`) or call
  the tools directly.
- Configure + run (from `original_c/mpack-develop/`):
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
