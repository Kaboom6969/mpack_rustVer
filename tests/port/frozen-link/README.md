# Frozen C suite link checkpoint

This directory provides a build path that leaves `tests/original/` unchanged
while compiling its sources against the Rust `mpack` `cdylib`.

Run the first ABI checkpoint:

```powershell
rustup target add x86_64-pc-windows-gnu
powershell -ExecutionPolicy Bypass -File tests/port/frozen-link/run.ps1
```

The Windows adapter builds Rust for the GNU target because the frozen C suite
is compiled with GCC. This is required for aggregate-by-value ABI calls such as
`mpack_tag_cmp()`. Set `MPACK_RUST_TARGET` to override the target when using a
matching C compiler/toolchain.

It compiles `c/frozen_nil_smoke.c` through the complete upstream MPack header
chain and links it to the Rust library. The probe calls the same writer ABI
used by the frozen suite and currently validates the implemented nil slice.

Use the full-link checkpoint while expanding writer parity:

```powershell
powershell -ExecutionPolicy Bypass -File tests/port/frozen-link/run.ps1 -Full -ExpectMissing
```

The latter command compiles every frozen unit source with the explicit
`embed-writer` configuration from `tests/port/ffi-harness/include/`. Writer
parity is complete when the command runs without `-ExpectMissing` and reports
zero failures.

Run the same gate against the optimized Rust artifact with `-Release` (or
`--release` when using `run.py`.)
