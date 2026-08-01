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

_None recorded yet (scaffold only)._
