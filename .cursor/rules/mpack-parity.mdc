---
description: Behavioral parity hotspots vs C MPack
globs: src/**/*
alwaysApply: false
---
# Behavioral Parity (match C MPack)
- **Stateful errors**: once `mpack_error_*` is set, later calls no-op / return nil-zero; check error only at key points (same as C).
- **Protocol choices** (see `original_c/docs/protocol.md`): shortest encode; allow overlong decode; non-negative ints encoded unsigned; float width preserved on write; expect/node signedness conversion without loss.
- **Compile-time features**: respect `MPACK_READER` / `WRITER` / `EXPECT` / `NODE` / `STDLIB` / `STDIO` / `COMPATIBILITY` / `EXTENSIONS` / tracking / debug asserts. Default test config enables nearly everything + custom `MPACK_MALLOC`/`FREE`.
- **Edge cases**: allocation failure → `mpack_error_memory`; UTF-8 checks when enabled; compound size tracking in debug; timestamps/ext when `MPACK_EXTENSIONS`; v4 compat when enabled.
- **Allocators**: C-visible buffers/nodes must use the same malloc/free contract as tests (not Rust global allocator for pointers the suite frees).