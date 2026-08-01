# Frozen C suite link checkpoint

This directory provides a build path that leaves `tests/original/` unchanged
while compiling its sources against the Rust `mpack` `cdylib`.

Run the first ABI checkpoint:

```powershell
rustup target add x86_64-pc-windows-gnu
powershell -ExecutionPolicy Bypass -File tests/port/frozen-link/run.ps1
```

```bash
python3 tests/port/frozen-link/run.py
```

The Windows adapter builds Rust for the GNU target because the frozen C suite
is compiled with GCC. This is required for aggregate-by-value ABI calls such as
`mpack_tag_cmp()`. Set `MPACK_RUST_TARGET` to override the target when using a
matching C compiler/toolchain.

It compiles `c/frozen_nil_smoke.c` through the complete upstream MPack header
chain and links it to the Rust library. The probe calls the same writer ABI
used by the frozen suite and currently validates the implemented nil slice.

Use the full-link checkpoint while expanding writer parity (embed-writer config):

```bash
python3 tests/port/frozen-link/run.py --full
```

```powershell
powershell -ExecutionPolicy Bypass -File tests/port/frozen-link/run.ps1 -Full -ExpectMissing
```

The latter command compiles every frozen unit source with the explicit
`embed-writer` configuration from `tests/port/ffi-harness/include/`. Writer
parity is complete when the command runs without `-ExpectMissing` and reports
zero failures.

## Default full-suite config (stub scaffolding)

To link and run the frozen suite under its upstream default configuration
(reader/expect/node/stdio/extensions/tracking/builder), build Rust with
`full-suite-abi` and point includes at `tests/original/test/unit/src`:

```bash
python3 tests/port/frozen-link/run.py --full --default-config
```

Unimplemented APIs are thin FFI stubs that set sticky `mpack_error_unsupported`
and return zero/nil/`NULL`. The gate succeeds when the binary links, passes the
C/Rust layout constructor check, and runs to completion. Assertion failures are
expected until stubs are replaced with safe-core implementations.

Because the frozen suite hardcodes `TEST_EARLY_EXIT=1`, the default-config
adapter force-includes `c/soft_abort.h` to redirect `abort` to a returning
`mpack_soft_abort()` so the first failed assertion does not kill the process
(and so GCC does not treat post-abort loop bodies as unreachable). It also
links `c/quiet_printf.c` to swallow per-assertion `printf` spam from huge
compound-size loops while keeping the final `Unit testing complete` summary.

Run the same gate against the optimized Rust artifact with `-Release` (or
`--release` when using `run.py`.)
