# Frozen C suite link checkpoint

This directory provides a build path that leaves `tests/original/` unchanged
while compiling its sources against the Rust `mpack` `cdylib`.

Run the first ABI checkpoint:

```powershell
powershell -ExecutionPolicy Bypass -File tests/port/frozen-link/run.ps1
```

It compiles `c/frozen_nil_smoke.c` through the complete upstream MPack header
chain and links it to the Rust library. The probe calls the same writer ABI
used by the frozen suite and currently validates the implemented nil slice.

Use the full-link checkpoint while expanding writer parity:

```powershell
powershell -ExecutionPolicy Bypass -File tests/port/frozen-link/run.ps1 -Full -ExpectMissing
```

The latter command compiles every frozen unit source with the explicit
`embed-writer` configuration from `tests/port/ffi-harness/include/`. It exits
successfully while unresolved Rust symbols are expected. Once writer parity is
complete, omit `--expect-missing`; a zero exit status then means the linked
frozen runner passed.
