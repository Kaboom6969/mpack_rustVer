# DECISIONS

Non-trivial divergences from MPack (C) and why. Update this file whenever behavior, API surface, or layout differs from the original in a meaningful way.

## Kickoff / layout

| Decision | Choice | Why |
| --- | --- | --- |
| Keep `original_c/` at repo root | Keep (not relocated) | Reference sources + differential builds without disturbing kickoff layout already in the repo. |
| Original tests path | `tests/original/` | Port Mortem layout; tree hashed at kickoff (see `.port-mortem.toml`). **Do not modify.** |
| New tests | `tests/port/` | Rust unit/integration tests that are not part of the frozen C suite. |
| Dual-layer design | Safe Rust core + C ABI FFI | Passes original C tests without rewriting them; keeps idiomatic Rust for maintainability. |
| Module map | `src/{common,writer,reader,expect,node}.rs` + `src/ffi/` | Mirrors C dependency order (common → writer/reader → expect → node). Safe modules stay `forbid(unsafe_code)`; raw pointers live only under `src/ffi/`. |
| Public C headers / inlines | Upstream MPack headers; `mpack-platform.c` for header-inline ABI | Accessors such as `mpack_writer_error()` / buffer helpers stay C inline / platform TU. Rust does not re-export them. |
| Frozen-suite link adapter | `tests/port/frozen-link/` links suite objects to the Rust library | Compiles `mpack-platform.c` only for inlines; does **not** compile C encoder/decoder/expect/node sources. How-to: `tests/port/frozen-link/README.md`. |
| FFI layout probe | `tests/port/ffi-harness/` (+ C `sizeof`/`offsetof` checks) | Compares C and Rust layouts before behavioral tests; first end-to-end proof path for the writer ABI. |
| First vertical slice | Fixed-buffer writer (`nil` → `0xc0`) under embed-writer | Proved include path + link to the Rust library; growable / file / builder / full write surface followed. |
| Default ABI feature | embed-writer layout (Cargo feature off) | Matches upstream `embed-writer` unit config so debug/release writer layouts stay identical. |
| Everything-suite ABI | Cargo feature `full-suite-abi` | Switches `#[repr(C)]` to the upstream everything layout (extensions, tracking, builder, …). One library build cannot satisfy both layouts. |
| C-visible allocators | Suite hooks `test_malloc` / `test_free` (`MPACK_MALLOC` / `MPACK_FREE`) for pointers returned to C; libc only for library-private buffers | Frozen everything links with `MPACK_FREE=test_free`, which adjusts `test_malloc_active`. Mixing libc `malloc` with suite `test_free` underflows the counter (`test-system.c`). Private file/writer buffers freed only inside FFI may still use libc. |
| Everything-gate soft abort | Force-include soft `abort` redirect | Suite hardcodes `TEST_EARLY_EXIT`; soft abort lets the process print a failure summary instead of dying on the first assertion. Ops detail: `tests/port/frozen-link/README.md`. |

## FFI boundary and ownership

| Decision | Choice | Why |
| --- | --- | --- |
| Safe encode/decode modules | `forbid(unsafe_code)` on `common` / `writer` / `reader` / `expect` / `node` | Keeps MessagePack algorithms free of raw pointers; every `# Safety` contract for C pointers lives in `src/ffi/`. |
| C-visible structs | No Rust references, trait objects, or `Box` stored in ABI types | FFI builds a temporary slice for one operation, calls safe core, then advances C cursors (`data` / buffer used). Storing Rust borrows in `mpack_*_t` would outlive the operation. |
| Authoritative C objects | C owns `mpack_writer_t` / `mpack_reader_t` / `mpack_tree_t` storage | Matches the frozen suite: tests allocate C structs on the stack and pass pointers into the library. |
| `mpack_error_t` representation | `c_int` constants with the intentional gap (`ok = 0`, `io = 2`) | Reading an unknown C integer as a Rust fieldless enum would be undefined behavior; mapping from `common::Error` is explicit. |
| Null writer / reader / buffer | Fail-closed sticky `mpack_error_bug` (or early return); no dereference | C often `mpack_assert`s on null. Rust FFI hardens the boundary; validity and exclusivity of non-null pointers remain caller requirements. |
| `set_fill` / `set_skip` with `size == 0` | Sticky `mpack_error_bug`; do not install the callback | C uses `mpack_assert` (fatal in debug). This port fail-closes like other invalid FFI setup (see `src/ffi/reader.rs`). |
| Unwinding panics in exports | `catch_unwind` → sticky `mpack_error_bug` where a writer/reader exists | Contains panics at the ABI edge (`src/ffi/guard.rs`). Ineffective if the crate is built with `panic = "abort"`. |

## Writer vertical slice

| Decision | Choice | Why |
| --- | --- | --- |
| Safe writer core | `src/writer.rs` (`Writer`, `GrowableWriter`, `Builder`, `WriteTracker`) | Encode algorithms stay pointer-free; FFI maps C buffers/callbacks onto these types for the duration of a call. |
| Writer FFI surface | `src/ffi/writer.rs` | Fixed buffer, growable (suite allocator under everything), flush / error / teardown callbacks, filename/stdfile, compound start/build/complete, timestamps/ext, UTF-8 helpers. |
| Builder page storage | Rust side-table (`Mutex<HashMap<usize, …>>` keyed by writer pointer); ABI `builder` field left empty for layout | C stores builder pages inside `mpack_writer_t.builder`. The side-table keeps compound-size resolution without growing unsafe fields inside the C struct. Observable if C inspects builder pointers. |
| Write-tracking hooks | Real FFI tracking under `full-suite-abi` | Initializes the ABI track stack and wires element, byte, compound, and builder push/pop checks through `src/ffi/stubs/track.rs`, including frozen-suite `mpack_break_hit` semantics. |
| Embed-writer gate | `python3 tests/port/frozen-link/run.py --full` | Writer lane acceptance: frozen suite under embed-writer reports `0 failures` (see team ownership rules). |

## Reader vertical slice

| Decision | Choice | Why |
| --- | --- | --- |
| Safe reader core | `src/reader.rs` over `&[u8]` with sticky `Error` | Fixed-buffer decode without fill callbacks; FFI layers fill/skip/file on top. |
| Reader FFI under `full-suite-abi` | `src/ffi/reader.rs` replaces former reader stubs | C owns `mpack_reader_t`; each decode builds a temporary safe-core `Reader` over `data..end`, advances `data` by `used()`, maps sticky errors through `flag_error` (including C’s `end = data` truncation). |
| `mpack_discard` / `mpack_print_data_to_*` | Iterative heap frame stack | C uses recursive call stacks. Hostile deep nesting completes or sticky-errors instead of overflowing the Rust stack. Pseudo-JSON for normal inputs matches C (`tests/port/reader_ffi_safety.rs`). |
| `mpack_read_bytes_alloc_impl` size + optional NUL | `checked_add`; wrap → sticky `mpack_error_too_big`; allocate via `test_malloc` | Stricter than upstream C’s latent overflow TODO on large sizes / 32-bit. Suite frees with `test_free`, so alloc must use the same hook. |
| UTF-8 / timestamp validation failure (safe core) | Sticky error; do **not** advance `Reader::used()` | Atomic cursor on validation failure. May diverge from C cursor consumption on the same paths; FFI can emulate C later if strict ABI parity is required. |
| Read tracking / `mpack_done_type` | Real track stack via `src/ffi/stubs/track.rs`; `done_type` / destroy / `remaining` call push/pop/check_empty | Needed for `test_expect_tracking` and compound `done_*` under everything (`MPACK_READ_TRACKING=1`). Discard skips str/bin/ext only after `track_bytes` so sticky bug does not mask EOF. |
| File init | Minimal `init_stdfile` / `init_filename` (owned buffer + fread fill / optional fseek skip) | Lets EOF loops reach `mpack_error_eof` without hanging the everything suite; fuller `test-file.c` / `test-buffer.c` streaming edge cases are not claimed green. |

## Expect table

| Decision | Choice | Why |
| --- | --- | --- |
| Safe-core shape | Free functions on `&mut Reader<'_>`; `ExpectCompound { is_nil, count }` for `*_or_nil` | Mirrors `mpack_expect_*(reader)` without a second parser; FFI can map to C `(bool, *count)` without inventing another shape. |
| Rust keywords | `r#bool` / `r#str`; `true_` / `false_` | Locked names for C `mpack_expect_bool` / `mpack_expect_str` / true/false expects. |
| Allocator-backed expects | Stay in FFI only (`*_alloc`, `char*` copies) | Safe core must not return allocator-owned pointers; teammates fill `src/expect.rs` bodies only. |
| Expect C ABI under `full-suite-abi` | `src/ffi/expect.rs` (replaces stubs) | Reuses Reader `read_with_core` / `ensure_*`; scalar paths via `expect_op!`; `*_alloc` stay FFI-only (`test_malloc` + `mpack_read_bytes_alloc_impl`). Gate: `test-expect.c` 0 failures under `--full --everything`. |
| Range expects on error | Return `min_value` (C parity); `mpack_assert_fail` if min > max | Matches `mpack-expect.c` / suite assert harness, not “zero on error”. |
| `double_strict` | Accepts float as well as double | C `mpack_expect_double_strict` promotes float; safe core aligned for FFI parity. |

## Node table

| Decision | Choice | Why |
| --- | --- | --- |
| Safe-core surface | Minimal locked `Tree` / `Node` API (`type_`, `&[u8]` payloads, no `*_alloc`) | Contract for later FFI wrapping; stream/file/pool/`copy_*` / print-to-file stay in FFI. Signature changes need lead approval and a row here. |
| Sticky errors on the tree | `Cell<Error>` shared through `&Tree` | Matches C `mpack_node_*` writing the tree error so accessors can flag through an immutable `Node` handle. |
| `as_f32` | `Tag::Float` only (no int/double widen) | Intentional minimal freeze vs full C `mpack_node_float` widening. |
| Required map lookup miss (`map_uint` / `map_str`) | Flag `Error::Data` | Optional/contains variants are not part of the locked surface. |
| Duplicate map keys | Not diagnosed | Out of the minimal freeze; C may expose richer diagnostics in some paths. |
| `Tree::parse` nesting | Iterative heap stack (+ `possible_nodes`-style remaining-byte reserve) | Matches C iterative parse; depth-1200 suite case must not blow the Rust stack. Absurd compound counts → `Error::Invalid`. |
| `Tree::size` / `parse_with_limits` | Expose consumed byte count; optional `max_nodes` → `TooBig` | Needed for `mpack_tree_size`, multi-message re-parse, and `init_pool` overflow. |
| Node C ABI under `full-suite-abi` | Real FFI in `src/ffi/node.rs` with side-table keyed by tree pointer | C owns `mpack_tree_t` / `mpack_node_t`; Rust graph + heap/pool ABI slots live off-struct (writer-builder pattern). Optional/contains/enum/dup-key/narrow/widen/utf8/copy/alloc/print/stream stay FFI-only. |
| File init empty / oversize | Empty file → `invalid`; `max_bytes != 0` and size > max → `too_big` (no silent truncate) | Matches C `mpack_file_tree_read`. |
| `tree.max_size` enforcement | Stream fill caps `owned_data` growth; do **not** reject `data_length > max_size` on parse of a preloaded buffer | C `max_size` is max **message** / fill accumulation, not whole multi-message buffer length. |
| Stream incomplete sticky errors | Greedy blocking fill + one-shot safe-core parse remaps core `Invalid`/`Eof` → `IO`, or `TOO_BIG` when fill hit `max_size` | C `reserve_fill` flags `too_big` when more bytes would exceed `max_size`; blocking `mpack_tree_parse` flags `io` when a `read_fn` still leaves the message incomplete (without a prior sticky error). This FFI does not claim call-for-call parity with C's on-demand reserve fill, never-EOF blocking read functions, or incremental `try_parse` resume. |

## Technical decisions (hotspots)

### Dual ABI layouts

- **MPack**: Compile-time `mpack-config.h` toggles fields on `mpack_writer_t` / `mpack_reader_t` / tags.
- **Rust intent**: Two explicit Cargo layouts — default embed-writer vs `full-suite-abi` — rather than one mega-struct with cfg soup in every export.
- **Status**: Mutually exclusive. Layout checks live under `tests/port/full_abi_layout.rs` and frozen-link. Embed-writer frozen-link is the Writer gate; everything uses `full-suite-abi` + staticlib on Windows so suite symbols (`test_malloc`, `mpack_assert_fail`) resolve.

### Sticky errors / NULL / assert

- **MPack**: Sticky `mpack_error_t` on reader/writer/tree; many invalid setups `mpack_assert` (debug abort) or leave behavior undefined.
- **Rust intent**: Safe core uses `common::Error`; FFI maps to ABI codes. At the FFI edge, prefer fail-closed sticky `bug` / early return over assert-abort; map unwinding panics to sticky errors when possible.
- **Status**: Writer and Reader FFI harden null / zero-size setup. This is FFI hardening, not a claim that concurrent mutation of a single C object is safe.
- **Intentional hardening**:
  - `mpack_writer_destroy(NULL)` → `mpack_error_bug` (no dereference).
  - Null reader buffer / `set_fill`/`set_skip` with `size == 0` → sticky `bug`.

### Nesting depth & recursion

- **MPack**: Recursive discard / print on the reader; iterative tree parse in node; depth/stack matter for hostile inputs.
- **Rust intent**: Prefer heap frame stacks at the FFI reader boundary for hostile depth; match observable results for well-formed / truncated inputs.
- **Status**: Reader FFI `discard` / `print_data_to_*` are iterative. Safe-core `node::Tree::parse` is iterative (heap frame stack) with a remaining-byte compound reserve. Extreme nesting on discard/print/tree completes or sticky-errors instead of stack overflow.

### Tracking

- **MPack**: Read/write track stacks enforce compound sizes when tracking is enabled. `mpack_writer_destroy` / `mpack_reader_destroy` destroy the track stack before flush/teardown; C discards `mpack_track_destroy`'s return value on the writer path.
- **Rust intent**: Read and write tracking are real under `full-suite-abi` so compound APIs match C. Writer destroy runs track cleanup before flush/teardown (same order as C / reader) so incomplete compounds sticky-error before growable teardown can hand a buffer to C.
- **Status**: The track stack lives in `src/ffi/stubs/track.rs` (push/pop/element/bytes/str_bytes_all/check_empty/destroy). Reader init/destroy/`done_type`/`read_tag`/`peek_tag`/`read_bytes`/`skip`/`remaining`/discard and Writer init/destroy/write/header/bytes/builder paths are wired.
- **Divergence**: Unlike C's writer destroy (which ignores `mpack_track_destroy`'s return), this port maps a track-destroy failure into sticky `mpack_error_bug` via `flag_error_impl` when the writer was still `ok`, matching reader destroy and preventing a successful growable hand-off after an incomplete message.

### Memory ownership

- **MPack**: Growable writers, `*_alloc` readers, and tree pools use `MPACK_MALLOC`; the suite frees with `MPACK_FREE` (`test_free` under frozen-link).
- **Rust intent**: Pointers returned to C (reader/expect `*_alloc`, node stubs that return buffers, growable writer output) use `test_malloc` / `test_free`. Library-private buffers freed only inside FFI (file reader owned buffer, track stack) may use libc. Never hand a Rust-global-allocator block to either free.
- **Status**: Reader `mpack_read_bytes_alloc_impl`, Expect `*_alloc` / `array_alloc`, Node `*_alloc`, and growable Writer output use suite hooks under `full-suite-abi`. Growable resize mirrors MPack’s `MPACK_MALLOC` fallback: allocate, copy initialized bytes, free the prior block.

### Full-suite stubs vs real Reader / Expect FFI

- **MPack**: One implementation for reader / expect / node / track / print.
- **Rust intent**: Under `full-suite-abi`, replace stubs module-by-module with safe-core calls (do not grow unsafe in stub bodies).
- **Status**: Reader / Expect / Node FFI are real under `full-suite-abi` (`src/ffi/{reader,expect,node}.rs`). Print helpers used by reader data-print live with reader; Writer FFI uses the shared track implementation.

### Safe-core surface shapes (Expect / Node)

- **MPack**: Pointer-rich `mpack_expect_*` / `mpack_node_*` / pools / `*_alloc` / `char*` copies.
- **Rust intent**: Safe core uses `&[u8]` / `Option` / sticky `Error`; Expect stays free functions on `&mut Reader<'_>`; Node stays the minimal locked `Tree` / `Node` list. Allocation and C string copies stay in `src/ffi/`.
- **Status**: Safe-core Expect/Node and Reader/Expect/Node FFI are done; gate `test-node.c` under `--full --everything`.
- **Signature changes** to locked safe-core exports require lead approval and a row in the Expect / Node tables above.

## Explicit non-goals (for now)

- Replacing the frozen C suite with `tests/port/` as the sole correctness proof for a claimed module.
- Wrapping the C library from Rust (disallowed by Port Mortem rules).
- Satisfying embed-writer and everything ABI layouts in a single library build.
- Editing `tests/original/` (or any frozen C suite path).
