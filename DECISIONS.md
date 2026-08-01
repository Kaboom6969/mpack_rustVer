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

`mpack_discard` under stubs formerly forced `mpack_error_eof` even when init
already set `mpack_error_unsupported`, so EOF-wait loops such as
`test_file_read_eof` could terminate after soft-continued assertions. That hack
is obsolete now that Reader FFI provides real discard plus stdfile/filename
init (see “Reader FFI (fixed-buffer first slice)” below).

The default (feature-off) build keeps the embed-writer ABI used by the existing
green frozen-link gate. A single library build cannot satisfy both layouts at once.

### Reader FFI (fixed-buffer first slice)

Under `full-suite-abi`, [`src/ffi/reader.rs`](src/ffi/reader.rs) replaces the
former `stubs/reader.rs` exports. Pattern matches Writer FFI: C owns
`mpack_reader_t` storage; each decode builds a temporary safe-core
`reader::Reader` over `data..end` (after `ensure` / fill when needed), advances
`data` by `used()`, and maps sticky errors through `flag_error` (including
C’s `end = data` truncation).

This slice targets `test-reader.c` green under
`python3 tests/port/frozen-link/run.py --full --everything`:

- Real `init` / `init_data` / `init_error` / `destroy` / `flag_error` /
  `remaining` / `set_fill` / `set_skip`
- `read_tag` / `peek_tag` / C-style recursive `discard` / `read_bytes` /
  `skip_bytes` / inplace / UTF-8 / cstr / alloc / timestamp helpers
- `ensure_straddle` / `read_native_straddle` with minimal fill refill
- `mpack_print_data_to_buffer` via safe-core (JSON-ish / bin hexdump)
- Minimal `init_stdfile` / `init_filename` (owned 4KiB buffer + fread fill /
  optional fseek skip + teardown) so file EOF loops can reach `mpack_error_eof`
  without hanging the everything suite

Frozen-link scaffolding: `tests/port/frozen-link/run.py` creates a repo-root
`test` symlink to `tests/original/test` before running the everything suite so
relative fixture paths (`test/messagepack/...`, `test/pseudojson/...`) resolve.
Without that link, `test_compare_print` soft-continues on a missing expected
file and then `memcmp`s a NULL pointer (SIGSEGV) once `print_data_to_file`
writes a non-empty actual file.

Deferred (Expect / buffer / fuller file parity):

- Real `mpack_track_*` stack: `mpack_done_type` is intentionally a **no-op** so
  tracking-enabled header inlines do not poison the reader. `remaining` /
  `destroy` do not call `track_check_empty` / `track_destroy` yet.
- Expect FFI still stubs; most behavioral weight for “reader” in the frozen
  suite still lives under `test-expect.c`.
- Full streaming buffer edge cases in `test-buffer.c` / `test-file.c` are not
  claimed green by this slice.

### Safe-core API freeze (Node, minimal)

Public items in `src/node.rs` are a **minimal** frozen contract for teammate
tree/DOM work and later FFI wrapping. Signature or public type changes require
lead approval.

**Bodies are intentional stubs** (`Tree::parse` starts as `Error::Unsupported`,
accessors no-op / flag unsupported). Teammate A owns filling implementations
until `tests/port/node_api.rs` acceptance tests (currently `#[ignore]`) pass.

Locked surface:

- `Tree<'data>::parse(&[u8])`, `error`, `flag_error`, `root`
- `Node<'tree, 'data>`: `tag`, `type_`, `is_nil`, `as_bool`, `as_u64`, `as_i64`,
  `as_f32`, `as_f64`, `str_bytes`, `bin_bytes`, `ext`, `array_len`, `array_at`,
  `map_count`, `map_key_at`, `map_value_at`, `map_uint`, `map_str`
- Sticky errors use `common::Error` on the tree. `Node` accessors may flag
  errors through `&Tree` (`Cell<Error>`), matching C `mpack_node_*` behavior.
- Payload views are `&[u8]` borrowed from the input slice (no allocator-owned
  returns). `type_` avoids the `type` keyword.
- Out of safe-core scope (FFI / lead): stream/file/stdfile init, C node pools,
  `*_alloc`, `copy_*` into C `char*`, print-to-file, and optional/contains/
  enum helpers beyond this minimal list.
- Teammates may fill bodies and add `tests/port/node_*.rs` only; do not grow
  the public surface without lead approval. When acceptance tests pass, remove
  their `#[ignore]` attributes.

Intentional minimal divergences vs full C `mpack-node` (once implemented):

- `as_f32` accepts only `Tag::Float` (no integer/double widen yet).
- Required map lookups (`map_uint` / `map_str`) flag `Error::Data` when missing;
  optional/contains variants are not locked yet.
- Duplicate map keys are not diagnosed in this freeze.

### Safe-core API freeze (Reader + Expect)

Public items in `src/reader.rs` and `src/expect.rs` are a frozen contract for
teammate safe-core work and later FFI wrapping:

- Teammates may fill or fix function bodies and add tests under `tests/port/`
  only. Signature or public type changes require lead approval.
- These modules stay under `forbid(unsafe_code)`: no raw pointers, no C
  callbacks, and no APIs that return allocator-owned `*mut` pointers.
- Sticky errors use `common::Error`; a failed operation leaves
  `reader.error()` set (same model as `Reader::read_tag`).
- Expect is free functions taking `&mut Reader<'_>` (mirrors
  `mpack_expect_*(reader)`). It must not grow a second parser.
- Out of safe-core scope (FFI / lead): `init_filename` / `init_stdfile`,
  fill+skip callbacks, `malloc` / `*_alloc`, and copying into C `char*` at the
  ABI boundary. Safe core may expose `&[u8]` / `&mut [u8]` helpers; allocation
  and pointer conversion stay in `src/ffi/`.
- `*_or_nil` results use `expect::ExpectCompound { is_nil, count }` so FFI can
  map to `(bool, *count)` without inventing another shape.
- Rust keywords force raw identifiers for two Expect exports: `expect::r#bool`
  and `expect::r#str` (still the locked names for C `mpack_expect_bool` /
  `mpack_expect_str`). `true_` / `false_` avoid the `true` / `false` keywords.

### Reader cursor atomicity on validation failure

The safe-core Reader treats UTF-8 validation failures (`read_bytes_utf8`) and
timestamp validation failures (`read_timestamp`) as atomic with respect to the
input cursor: on failure, the reader flags a sticky error and does not advance
`Reader::used()`.

This may diverge from the C reader's cursor semantics for the same failure
conditions. If strict C parity is required at the ABI boundary, the FFI layer
may need to emulate C cursor consumption behavior while still mapping to the
safe-core Reader helpers.

