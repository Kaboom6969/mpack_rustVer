# Decisions

Non-trivial behavioral, API, or layout differences from C MPack belong here.

## Architecture intent

- **Safe Rust core**: idiomatic types (`Vec<u8>`, `Result`, enums for tags/errors).
  Prefer zero `unsafe` in encode/decode algorithms.
- **FFI layer** (planned): `#[repr(C)]` + `#[no_mangle] pub extern "C"` matching the
  MPack C ABI (`mpack_reader_t`, `mpack_writer_t`, `mpack_tree_t`, `mpack_node_t`,
  `mpack_tag_t`, `mpack_error_t`, …) so `tests/original/` links unchanged.
- **Module order**: common → writer/reader → expect → node (mirrors C dependency order).
- **Allocators**: C-visible buffers/nodes must use the same malloc/free contract as the
  frozen suite (not the Rust global allocator for pointers the suite frees).

## Divergences

### Audited unsafe boundary

Safe encoding and decoding modules are compiled with `forbid(unsafe_code)`.
Raw-pointer access is restricted to `src/ffi/`, where each unsafe block states
its safety contract. The FFI layer does not store Rust references, trait
objects, or `Box` values in C-visible structs. It creates a temporary slice
only for the duration of an operation and immediately calls the safe core.

### Initial ABI configuration

The first C ABI slice supports the upstream `embed-writer` configuration only:
writer enabled, with reader, expect, node, stdlib, stdio, compatibility,
extensions, builder, allocation, and write tracking disabled. This keeps the
debug and release layouts identical. It is not ABI-compatible with MPack
configurations that add conditional fields to `mpack_writer_t`.

The C harness supplies an explicit `mpack-config.h` and includes the complete
upstream header chain. It compares C `sizeof`/`offsetof` values with Rust before
testing behavior. Header-inline functions such as
`mpack_writer_buffer_used()` and `mpack_writer_error()` remain C inline
functions and are not exported by Rust.

### ABI error representation

The FFI layer represents `mpack_error_t` as `c_int` constants, including the
intentional gap between `mpack_ok = 0` and `mpack_error_io = 2`. It does not use
a Rust fieldless enum because reading an unknown C integer as a Rust enum would
be undefined behavior. Mapping from the safe `Error` type to ABI codes is
explicit.

### Null pointers and panics

The original C implementation asserts on a null writer during initialization.
The Rust FFI instead avoids dereferencing null: void functions return early and
`mpack_writer_destroy(NULL)` returns `mpack_error_bug`. A null buffer
initializes a non-null writer into `mpack_error_bug`. This is deliberate FFI
hardening; validity and exclusivity of non-null pointers remain caller
requirements.

Every exported function contains unwinding panics with `catch_unwind` and
falls back to `mpack_error_bug` where a writer is available. This cannot catch
process-aborting panics if a consumer builds the library with `panic = "abort"`.

### Deferred writer features

The initial slice clears the flush, error, teardown, and context fields during
initialization. It supports an explicitly requested fixed-buffer
`mpack_writer_flush_message()` callback, but allocator-backed writers, error
and teardown callbacks, tracking, builder support, and full frozen-suite
behavioral parity are deferred to later vertical slices.

### Frozen-suite link adapter

`tests/port/frozen-link/` compiles `mpack-platform.c` only to emit MPack's
header-inline ABI definitions, then links the frozen test objects to the Rust
`cdylib`. It does not compile any C encoder, decoder, expect, or node source.
This adapter is necessary because C11 header inlines such as
`mpack_writer_error()` require one external definition in debug builds.

### Full-suite ABI stubs

The `full-suite-abi` Cargo feature switches `#[repr(C)]` layouts to the upstream
everything unit-test configuration (compatibility, extensions, malloc reserve,
builder, and read/write tracking). Under this feature, missing reader, expect,
node, track, and print exports are thin stubs that set sticky
`mpack_error_unsupported` and return zero/nil/`NULL`.

Stubs are temporary scaffolding for the
`python3 tests/port/frozen-link/run.py --full --everything` feedback loop
(C `everything` macros; `--default-config` remains an alias). They do not change
the final unsafe budget: each export remains a required C ABI entry, safe
encode/decode stays in `forbid(unsafe_code)` modules, and stub bodies are
replaced with safe-core calls rather than grown in place.

The everything adapter force-includes a soft `abort` redirect so the frozen
suite's hardcoded `TEST_EARLY_EXIT` does not stop the process before printing
the failure summary. It also links a quiet `printf` override so soft-continued
assertion spam does not dominate runtime. Both are scaffolding-only and are not
used by the embed-writer gate.

Full-suite frozen-link builds with `cargo rustc --crate-type staticlib` so
suite-provided symbols (`test_malloc`, `mpack_assert_fail`) resolve when linking
the final executable. A Windows `cdylib` cannot leave those undefined at DLL
link time.

Everything-gate crash detection treats exit `< 0` or `>= 128` as failure
(Linux signal encoding and Windows SEH / abnormal statuses such as
`0xC0000005`); normal assertion failure is `EXIT_FAILURE` (`1`). The gate
expects GCC/MinGW (`CC=gcc`); the MSVC `cl` path does not force-include the
soft-abort / quiet-printf adapters.

`soft_abort.h` includes `<stdlib.h>` before `#define abort mpack_soft_abort`
so libc's noreturn `abort` declaration is not rewritten onto
`mpack_soft_abort` (which would omit call-site epilogues and trip stack
canaries when the soft abort returns).

`mpack_discard` under stubs forces `mpack_error_eof` even when init already set
`mpack_error_unsupported`, so EOF-wait loops such as `test_file_read_eof` can
terminate after soft-continued assertions.

The default (feature-off) build keeps the embed-writer ABI used by the existing
green frozen-link gate. A single library build cannot satisfy both layouts at once.

