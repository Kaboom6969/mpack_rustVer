# Frozen C suite link checkpoint

This directory provides a build path that leaves `tests/original/` unchanged
while compiling its sources against the Rust `mpack` library.

**Parity rule:** when a suite flag runs the frozen unit sources, acceptance is
whatever the C harness itself reports:

1. stdout line `Unit testing complete. N failures in M checks.`
2. process exit from `tests/original/.../test.c` (`0` only if `N == 0`)

The runner builds, links, and **forwards** that result. It must not rewrite a
failing suite into success.

## Smoke (not the frozen suite)

```powershell
rustup target add x86_64-pc-windows-gnu
powershell -ExecutionPolicy Bypass -File tests/port/frozen-link/run.ps1
```

```bash
python3 tests/port/frozen-link/run.py
```

Without a suite flag, this only compiles `c/frozen_nil_smoke.c`. That is an ABI
smoke probe, **not** a run of `tests/original/`.

The Windows adapter builds Rust for the GNU target because the frozen C suite
is compiled with GCC. This is required for aggregate-by-value ABI calls such as
`mpack_tag_cmp()`. Set `MPACK_RUST_TARGET` to override the target when using a
matching C compiler/toolchain.

## Embed-writer gate (`--embed-writer`)

```bash
python3 tests/port/frozen-link/run.py --embed-writer
```

```powershell
powershell -ExecutionPolicy Bypass -File tests/port/frozen-link/run.ps1 -EmbedWriter
```

Compiles every frozen unit source with the explicit `embed-writer`
configuration from `tests/port/ffi-harness/include/`. Green when the C harness
prints `0 failures` and the process exits 0 (matching C `embed-writer-release`).

## Everything parity gate (`--everything`)

To link and run the frozen suite under the upstream **everything** configuration
(reader/expect/node/stdio/compatibility/extensions/tracking/builder), build
Rust with `full-suite-abi` and pass the same feature macros as C
`run-everything-debug`:

```bash
python3 tests/port/frozen-link/run.py --everything
```

```powershell
powershell -ExecutionPolicy Bypass -File tests/port/frozen-link/run.ps1 -Everything
```

`--default-config` is kept as an alias for `--everything`.

**Green means:** C harness `0 failures` and process exit 0. Any non-zero
failure count or non-zero exit makes the runner fail.

Everything mode invokes `cargo rustc --crate-type staticlib` so suite symbols
such as `test_malloc` / `mpack_assert_fail` resolve at final executable link
(required on Windows, where a `cdylib` cannot leave those undefined).

Default everything builds keep native `TEST_EARLY_EXIT` / `abort()` behavior
(first failed assertion may kill the process before a summary). That matches
upstream C semantics.

### Optional debug: `--soft-continue`

```bash
python3 tests/port/frozen-link/run.py --everything --soft-continue
```

```powershell
powershell -ExecutionPolicy Bypass -File tests/port/frozen-link/run.ps1 -Everything -SoftContinue
```

Force-includes `c/soft_abort.h` and `c/quiet_printf.c` so the suite can continue
past `TEST_EARLY_EXIT` and print a full failure list. The runner **still**
forwards the suite exit / failure count — soft-continue is never a fake green
and is **not** the parity acceptance path.

### Removed / deprecated

- `--expect-missing`: rejected (incomplete link must fail).
- `--full` / `-Full`: deprecated alias for `--embed-writer` / `-EmbedWriter`.
  Old `--full --everything` still works but prints a warning; prefer
  `--everything` alone.

Staticlib link may still pass `-Wl,--allow-multiple-definition` so
`mpack-platform.c` header inlines can coexist with Rust `#[no_mangle]` exports.
Prefer eliminating duplicates over widening that flag.

Run the same gate against the optimized Rust artifact with `-Release` (or
`--release` when using `run.py`.)
