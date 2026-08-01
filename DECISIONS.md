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
initialization but does not invoke user callbacks. Allocator-backed writers,
flush and teardown callbacks, tracking, builder support, and frozen-suite
integration are deferred to later vertical slices.
