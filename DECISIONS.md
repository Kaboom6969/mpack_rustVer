# DECISIONS

Non-trivial divergences from MPack (C) and why. Update this file whenever behavior, API surface, or layout differs from the original in a meaningful way.

## Kickoff / layout

| Decision | Choice | Why |
| --- | --- | --- |
| Keep `original_c/` at repo root | Keep (not relocated) | Reference sources + differential builds without disturbing kickoff layout already in the repo. |
| Original tests path | `tests/original/` | Port Mortem layout; tree hashed at kickoff (see `.port-mortem.toml`). **Do not modify.** |
| New tests | `tests/port/` | Rust unit/integration tests that are not part of the frozen C suite. |
| Differential fuzz | `fuzz/` (cargo-fuzz; C oracle vs safe core) | Optional parity finder; does not replace frozen-suite module gates. |
| Dual-layer design | Safe Rust core + C ABI FFI | Passes original C tests without rewriting them; keeps idiomatic Rust for maintainability. |
| Module map | `src/{common,writer,reader,expect,node}.rs` + `src/ffi/` | Mirrors C dependency order (common → writer/reader → expect → node). Safe modules stay `forbid(unsafe_code)`; raw pointers live only under `src/ffi/`. |
| Public C headers / inlines | Upstream MPack headers; `mpack-platform.c` for header-inline ABI | Accessors such as `mpack_writer_error()` / buffer helpers stay C inline / platform TU. Rust does not re-export them. |
| Frozen-suite link adapter | `tests/port/frozen-link/` links suite objects to the Rust library | Compiles `mpack-platform.c` only for inlines; does **not** compile C encoder/decoder/expect/node sources. How-to: `tests/port/frozen-link/README.md`. |
| FFI layout probe | `tests/port/ffi-harness/` (+ C `sizeof`/`offsetof` checks) | Compares C and Rust layouts before behavioral tests; first end-to-end proof path for the writer ABI. |
| First vertical slice | Fixed-buffer writer (`nil` → `0xc0`) under embed-writer | Proved include path + link to the Rust library; growable / file / builder / full write surface followed. |
| Default ABI feature | embed-writer layout (Cargo feature off) | Matches upstream `embed-writer` unit config so debug/release writer layouts stay identical. |
| Everything-suite ABI | Cargo feature `full-suite-abi` | Switches `#[repr(C)]` to the upstream everything layout (extensions, tracking, builder, …). One library build cannot satisfy both layouts. |
| C-visible allocators | Suite hooks `test_malloc` / `test_free` (`MPACK_MALLOC` / `MPACK_FREE`) for pointers returned to C; under frozen-link also for file/track private buffers via `suite_libc` | Frozen everything links with `MPACK_FREE=test_free`, which adjusts `test_malloc_active`. Mixing libc `malloc` with suite `test_free` underflows the counter (`test-system.c`). |
| Everything parity gate | Runner forwards C suite exit + `Unit testing complete. N failures` | Acceptance is the frozen harness itself (`tests/original/.../test.c`). Soft-abort / quiet printf are opt-in `--soft-continue` only (debug; still not fake green). `--expect-missing` removed. How-to: `tests/port/frozen-link/README.md`. |
| Fair C↔Rust bench | `bench/`: same C driver; upstream C vs `full-suite-abi` staticlib; everything features + forced tracking; libc malloc (thin `test_*` identity wrappers for Rust symbol names) with **post-link `objdump` assert** that `test_free`→libc (else refuse `measured`); decode-only RSS in a fresh process; per-trial shuffled C/Rust order; release opts | Measures the C ABI path under 2B feature lock, not safe-core and not suite fail-injection allocators. Allocator gate prevents silent noop-`test_free` link wins. See `bench/methodology.md`. |
| Hot-path opts (no new unsafe) | Bulk `write_bytes`/`write_header`; builder side-table skipped via `AtomicUsize` when no open builders; FFI drops safe-core `Vec<NodeData>` after ABI materialize; parse `children` pre-reserve | Encode/RSS gaps vs C are dual-layer tax. Keep `forbid(unsafe_code)` on safe core; do not grow `src/` unsafe counts. Gate: `--embed-writer` + `--everything` 0 failures, and `reader_diff`/`node_diff`/`total_diff` each 60s clean. |

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
| Embed-writer gate | `python3 tests/port/frozen-link/run.py --embed-writer` | Writer lane acceptance: frozen suite under embed-writer reports `0 failures` and exit 0 (C harness verdict; see team ownership rules). |
| Everything gate | `python3 tests/port/frozen-link/run.py --everything` | Reader/Expect/Node (and related) acceptance: same C harness verdict under everything + `full-suite-abi`. Runner must not map failures to success. |

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
| `nil` / `bool` / true/false / `str` | Type-byte path (`read_native_u8` / `u16` / `u32`), matching C `mpack_expect_type_byte` / non-size-optimized `mpack_expect_str` | Avoids full `read_tag` on truncated multi-byte markers (would sticky `Invalid` instead of C's `Type`). Map/array/bin/ext still use `read_tag` (same as C). |
| `float_range` / `double_range` | Reject with C's `val < min \|\| val > max` | IEEE NaN bounds: C's comparisons are false so NaN bounds do not reject; `>= && <=` would. |
| Rust keywords | `r#bool` / `r#str`; `true_` / `false_` | Locked names for C `mpack_expect_bool` / `mpack_expect_str` / true/false expects. |
| Allocator-backed expects | Stay in FFI only (`*_alloc`, `char*` copies) | Safe core must not return allocator-owned pointers; teammates fill `src/expect.rs` bodies only. |
| Expect C ABI under `full-suite-abi` | `src/ffi/expect.rs` (replaces stubs) | Reuses Reader `read_with_core` / `ensure_*`; scalar paths via `expect_op!`; `*_alloc` stay FFI-only (`test_malloc` + `mpack_read_bytes_alloc_impl`). Gate: `test-expect.c` 0 failures under `--everything`. |
| `str_match` | Byte-at-a-time via `read_native_u8` (C `mpack_expect_str_match`) | Mismatch flags `Type` before truncated bulk reads would sticky `Invalid`. |
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
- **Status**: Mutually exclusive. Layout checks live under `tests/port/full_abi_layout.rs` and frozen-link. Embed-writer and everything gates both require the C harness `0 failures` + exit 0 (`run.py` forwards; no fake green). Everything uses `full-suite-abi` + staticlib on Windows so suite symbols (`test_malloc`, `mpack_assert_fail`) resolve. Staticlib link may still use `-Wl,--allow-multiple-definition` for `mpack-platform.c` vs Rust export overlap (documented risk; prefer eliminating duplicates).

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
- **Status**: The track stack lives in `src/ffi/stubs/track.rs` (push/pop/element/bytes/str_bytes_all/check_empty/destroy). Reader init/destroy/`done_type`/`read_tag`/`peek_tag`/`read_bytes`/`skip`/`remaining`/discard and Writer init/destroy/write/header/bytes/builder paths are wired. Growable `mpack_writer_init_growable` and file `mpack_writer_init_stdfile` match C on track-init failure: keep the buffer, sticky-error, and still wire flush/teardown (plus FILE context) so destroy reclaims without a dangling pointer. `mpack_write_object_bytes` calls write-element tracking before the raw write, matching C. Growable teardown only shrinks when `used < capacity/2` (C parity); always-realloc on destroy was double-firing the error handler under suite fail-injection.
- **Divergence**: Unlike C's writer destroy (which ignores `mpack_track_destroy`'s return), this port maps a track-destroy failure into sticky `mpack_error_bug` via `flag_error_impl` when the writer was still `ok`, matching reader destroy and preventing a successful growable hand-off after an incomplete message.

### Memory ownership

- **MPack**: Growable writers, `*_alloc` readers, and tree pools use `MPACK_MALLOC`; the suite frees with `MPACK_FREE` (`test_free` under frozen-link). Stdio is remapped via macros (`fopen` → `test_fopen`, …) in C TUs only.
- **Rust intent**: Pointers returned to C (reader/expect `*_alloc`, node buffers, growable writer output) use `test_malloc` / `test_free`. Under frozen-link (`--cfg mpack_frozen_link`), FFI also routes file/track buffers and stdio through `test_*` so `test_files_count` / fail-injection stay honest. Without that cfg, cargo-test shims or libc apply.
- **Status**: `src/ffi/suite_libc.rs` centralizes suite vs libc entry points. Frozen-link builds staticlib with `--cfg mpack_frozen_link` (buffer size 33 / track init capacity 3). Tree file init rejects `max_bytes > LONG_MAX` with `mpack_break` + bug before open. Node `print_to_file` uses C depth-2 indent.

### Full-suite stubs vs real Reader / Expect FFI

- **MPack**: One implementation for reader / expect / node / track / print.
- **Rust intent**: Under `full-suite-abi`, replace stubs module-by-module with safe-core calls (do not grow unsafe in stub bodies).
- **Status**: Reader / Expect / Node FFI are real under `full-suite-abi` (`src/ffi/{reader,expect,node}.rs`). Print helpers used by reader data-print live with reader; Writer FFI uses the shared track implementation.

### Safe-core surface shapes (Expect / Node)

- **MPack**: Pointer-rich `mpack_expect_*` / `mpack_node_*` / pools / `*_alloc` / `char*` copies.
- **Rust intent**: Safe core uses `&[u8]` / `Option` / sticky `Error`; Expect stays free functions on `&mut Reader<'_>`; Node stays the minimal locked `Tree` / `Node` list. Allocation and C string copies stay in `src/ffi/`.
- **Status**: Safe-core Expect/Node and Reader/Expect/Node FFI are done; gate `test-node.c` under `--everything`.
- **Signature changes** to locked safe-core exports require lead approval and a row in the Expect / Node tables above.

## Differential fuzz (C oracle vs safe core)

| Decision | Choice | Why |
| --- | --- | --- |
| Tooling | `cargo-fuzz` / libFuzzer under Linux or WSL (`fuzz/`) | Coverage-guided; matches Port Mortem “differential fuzzer” intent better than shelling out per input. |
| C oracle | Compile `original_c/.../mpack-{common,platform,reader,writer,expect,node}.c` into the fuzz binary behind `oracle_*` helpers | Frozen-link links the C *suite* to Rust FFI; differential needs the real C implementation as a separate object set. |
| Rust side | `mpack` with `default-features = false` (Cargo feature `ffi` off) | Avoids `#[no_mangle]` clashes between Rust FFI and original C `mpack_*` symbols. Default builds keep `ffi` enabled. |
| Embed / FFI tests | Frozen-link embed path passes `--features ffi`; FFI port tests use `required-features = ["ffi"]` (or `full-suite-abi`) | Do not rely on `default = ["ffi"]` alone — `--no-default-features` must not silently drop `mpack_*` from the embed cdylib. |
| Targets | `reader_diff`, `node_diff`, `total_diff`, `expect_diff`, `writer_diff` in `fuzz/`; `ffi_crash` in `fuzz_ffi/` | Reader/node/total digests; expect opcode digest; writer read→rewrite transfer; FFI **crash-only** package (no C oracle — not parity evidence for expect/writer). Driver: `python3 fuzz/run_all.py`. |
| Expect input | First byte = ops length; ops drive `mpack_expect_*`; remainder = MessagePack payload | Expect is schema-typed; unstructured MessagePack alone cannot exercise the surface. |
| Expect error codes | Precise `error_to_c` sticky codes (same as reader/writer digests); op walk stops at first sticky error | After aligning safe-core `nil`/`bool`/`str` to C type-byte paths, Type/Invalid/Eof/TooBig remain comparable. Remaining map/array/bin/ext paths already match C's `read_tag`. |
| Expect `str_match` harness | Expected bytes masked to 7-bit ASCII in both digests | Upstream C compares `uint8_t` payload to `char` expected; on signed-char hosts (Linux gcc default) bytes ≥ 0x80 falsely sticky `Type`. Safe-core `&[u8]` compare is correct; harness avoids the platform footgun rather than collapsing errors. |
| Writer transfer | Growable rewrite of one top-level value (depth ≤ 1024); compare reader/writer sticky errors + emitted bytes | Mirrors upstream AFL `fuzz.c` transfer path without Expect/Builder. Oracle config has `MPACK_READ/WRITE_TRACKING=0`, so transfer does **not** exercise `done_*` tracking (unlike frozen `--everything`). |
| FFI crash package | Separate `fuzz_ffi/` with `full-suite-abi`, no C oracle objects | Differential fuzz must keep Rust `#[no_mangle]` off; crash harness needs the port’s C ABI exports. Narrow opcode surface (~10 ops, fixed writer buf); **crash smoke only**, not FFI/expect/writer parity. |
| Digest | Sticky error + packed tag records (type/aux/scalar/FNV-1a payload); depth cap 1024 | Reader/node/total: raw payload bytes only (no UTF-8 validation in the digest walk). Expect: opcode/ok/value/hash records, including UTF-8 ops that exercise `expect::utf8*` validation. On sticky error, `bytes_used = 0`. |
| `bytes_used` on error | Cleared to 0 in both digests when sticky error ≠ ok | C may consume partial payload bytes before flagging invalid; safe-core often stops earlier. Structure + error remain compared (intentional weakening for cursor noise). |
| FFI LSan | `ffi_crash` runs with leak detection enabled | Under `cfg(fuzzing)`, `suite_libc` pairs `test_malloc`→`calloc` with real `test_free`→`free`; disabling LSan is no longer required for noop-free. |
| libFuzzer `-max_len` | Document `-max_len=65536` in runbook (default is 4096) | Matches `ORACLE_MAX_INPUT` so large-input paths are reachable. |
| Evidence | Optional `fuzz/log.txt` after timed clean runs; `fuzz/run_all.py` for sequential coverage | Documents smoke; not a substitute for frozen-suite module gates. |

## Upstream C findings (suite / fuzz only)

Discovered empirically via original C sanitizer suite (not by reading C TODOs).
Full run log: `fuzz/c_findings.md`.

| Finding | Evidence | Rust port |
| --- | --- | --- |
| `mpack_write_str(writer, NULL, 0)` is C undefined behavior: null source passed to `mpack_memcpy` when `count == 0`. Two build paths: `MPACK_OPTIMIZE_FOR_SIZE=0` fixstr fast path (`mpack-writer.c:1266`); `MPACK_OPTIMIZE_FOR_SIZE=1` via `mpack_write_native` (`mpack-writer.c:526`). `mpack_write_utf8(NULL, 0)` reaches the same UB after `mpack_utf8_check` (length 0 does not read the pointer) then `mpack_write_str`. Unit test `test_write_utf8` expects `NOERROR`. Not MessagePack-byte-triggered; caller must pass `(NULL, 0)`. | `run-sanitize-undefined-debug` (SIZE=0 default) UBSan at `:1266`, stack `test-write.c:1022`. Minimal SIZE=0/1 repros both hit UBSan, emit `0xa0`, `error=0` (see `fuzz/c_findings.md`). ASan unit + AFL++ `fuzz.c` (600s) found no memory crashes (AFL does not call this writer API shape). | Hardened separately: `mpack_write_str` / `mpack_write_bytes` / `mpack_write_object_bytes` via `write_c_bytes` (null+zero → `&[]`; null+nonzero → sticky `bug`). `mpack_write_utf8` does **not** use `write_c_bytes`; it has its own null/count branch then writes via safe core (`src/ffi/writer.rs`). |

## Explicit non-goals (for now)

- Replacing the frozen C suite with `tests/port/` as the sole correctness proof for a claimed module.
- Wrapping the C library from Rust (disallowed by Port Mortem rules).
- Satisfying embed-writer and everything ABI layouts in a single library build.
- Editing `tests/original/` (or any frozen C suite path).
