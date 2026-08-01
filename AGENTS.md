# AGENTS.md

## Cursor Cloud specific instructions

This repo is a **Port Mortem** project: porting the C **MPack** MessagePack library
(`original_c/mpack-develop/`) to Rust. The Rust port (`src/`, `Cargo.toml`) does not
exist yet — it is the work to be done. The frozen C unit suite the port must eventually
pass via a C-ABI FFI layer lives under `tests/original/` (do not edit it; see
`.cursor/rules/port-mortem-core.mdc`).

### Toolchain (pre-installed in the VM snapshot)
- Rust `cargo`/`rustc` (for the Rust port), C toolchain (`gcc`, `g++`, `clang`),
  `python3`, and `ninja` are already installed. The update script only runs
  `cargo fetch` (guarded on `Cargo.toml` existing), since there is no dependency
  manifest until the port is started.

### Building / running the reference C library + unit suite
- The C reference implementation and its unit suite are the runnable "application"
  today. Build/run from `original_c/mpack-develop/`.
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

### Rust port (once it exists)
- Standard `cargo build` / `cargo test` / `cargo clippy` at the repo root. New Rust
  tests go in `tests/port/` only (never edit the frozen suite in `tests/original/`).
- The FFI layer must match the MPack C ABI so the frozen C suite links unchanged;
  see `.cursor/rules/mpack-architecture.mdc` and `mpack-parity.mdc`.
- Parallel lanes and **per-module frozen-suite gates** are in
  `.cursor/rules/team-ownership.mdc` (also mirrored under `.trae/rules/`).
  A lane is done only when its matching `test-*.c` has 0 failures under the
  documented `frozen-link` command—not from `tests/port/` alone.
