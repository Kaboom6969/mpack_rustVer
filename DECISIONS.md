# DECISIONS

Non-trivial divergences from MPack (C) and why. Update whenever behavior, API
surface, or layout differs from the original in a meaningful way.

## Layout and gates

| Decision | Choice | Why |
| --- | --- | --- |
| Upstream for fuzz / bench | Fetch pinned checkout into `target/upstream/mpack/pinned/` via `tools/upstream_mpack.py` (`kickoff_hash`, fallback `source_version` tag) | No tracked upstream source tree; reproducible differential tooling. Cache metadata must match current pin fields. |
| Frozen suite | `tests/original/` (hashed in `.port-mortem.toml`) | Port Mortem layout. **Do not modify.** |
| New tests | `tests/port/` | Rust unit/integration tests outside the frozen suite. |
| Dual-layer | Safe Rust core + C-ABI FFI | Suite links unchanged; codecs stay idiomatic Rust (`Result` / enums / owned buffers in core). |
| Unsafe isolation | Safe modules `forbid(unsafe_code)`; all `unsafe` under `src/ffi/` only | Encode/decode algorithms stay pointer-free; FFI owns C ABI pointers so the unmodified suite can link. |
| Module map | `src/{common,writer,reader,expect,node}.rs` + `src/ffi/` | Mirrors C order. Reader / Expect / Node / Writer FFI under `full-suite-abi` call safe core (not C codec TUs). |
| Public headers / inlines | Vendored `include/upstream/` (+ `mpack-platform.c`); override with `MPACK_UPSTREAM_SRC` | Header-inline accessors stay C; Rust does not re-export them. |
| Frozen-link | `tests/port/frozen-link/` | Links suite `.c` to the Rust library; compiles `mpack-platform.c` for inlines only — **not** C encoder/decoder/expect/node sources. |
| Dual ABI | Default embed-writer vs Cargo `full-suite-abi` | Mutually exclusive layouts. One build cannot satisfy both. |
| Suite allocators | `test_malloc` / `test_free`; frozen-link also routes file/track/stdio via `suite_libc` (`--cfg mpack_frozen_link`) | Everything links `MPACK_FREE=test_free`. Mixing libc `malloc` with suite `test_free` underflows counters. |
| Parity gates | `run.py --embed-writer` / `--everything` | Acceptance = C harness `0 failures` + exit 0. Soft-continue is debug only (never fake green). `--expect-missing` rejected. |
| Fair bench | `bench/`: same C driver; pinned upstream vs `full-suite-abi` staticlib; post-link `objdump` asserts `test_free`→libc | Measures ABI path under locked features; refuse `measured` if noop `test_free` wins. See `bench/methodology.md`. |
| Error style | Sticky `common::Error` in safe core; FFI maps to `mpack_error_t` | Idiomatic Rust interior; C sticky-error model preserved at the ABI. |

## FFI boundary

| Decision | Choice | Why |
| --- | --- | --- |
| ABI storage | C owns `mpack_*_t`; no Rust borrows/`Box` in ABI structs | Suite stacks C objects. FFI builds temporary slices, calls safe core, advances cursors. |
| `mpack_error_t` | `c_int` constants (gap: `ok=0`, `io=2`) | Unknown C integers must not be read as a fieldless Rust enum. |
| Null / zero-size setup | Fail-closed sticky `mpack_error_bug` | C often `mpack_assert`s. Includes null destroy, null buffers, `set_fill`/`set_skip` with `size==0`. |
| Panics at ABI edge | `catch_unwind` → sticky `bug` when a writer/reader exists | `src/ffi/guard.rs`. Ineffective under `panic="abort"`. |

## Live divergences

| Area | Choice | Why |
| --- | --- | --- |
| Builder pages | Rust side-table keyed by writer pointer; ABI `builder` left empty | Avoids growing unsafe fields inside `mpack_writer_t`. Observable if C inspects builder pointers. |
| Writer track destroy | Map track-destroy failure → sticky `bug` when still `ok` | C ignores `mpack_track_destroy` return on writer destroy. Prevents growable hand-off after incomplete message. Track code: `src/ffi/stubs/track.rs`. |
| Null write `(ptr,0)` | `write_str` / `write_bytes` / `write_object_bytes`: null+zero → empty slice; null+nonzero → sticky `bug`. `write_utf8` has its own null branch | Upstream C is UB on `mpack_write_str(NULL,0)` (see `fuzz/c_findings.md`). Hardened after differential / sanitizer evidence. |
| Discard / print / tree parse | Iterative heap frame stacks | Hostile depth must not blow the Rust stack; normal inputs match C. |
| UTF-8 / timestamp fail (safe reader) | Sticky error; do **not** advance `used()` | May diverge from C cursor consumption on the same paths. |
| `*_alloc` size | `checked_add`; wrap → `too_big`; allocate via `test_malloc` | Stricter than C’s latent overflow on large sizes. |
| Expect scalars | Type-byte paths for `nil`/`bool`/true/false/`str`; `str_match` byte-at-a-time; `float_range`/`double_range` use C’s `< \|\| >` (NaN bounds); locked names `r#bool`/`r#str`/`true_`/`false_` | Truncation and IEEE edge parity with C; `*_alloc` stay FFI-only. |
| Node `as_f32` | `Tag::Float` only (no int/double widen) | Intentional minimal safe-core freeze vs full C widening. |
| Optional map miss | `None`, no sticky error | C uses `missing` node; safe core has no `missing` type. |
| Retain `NodeData` after materialize | Keep graph + ABI maps in `FfiTreeState` | Single lookup implementation for map/contains/enum via safe core. |
| Node stream incomplete | Greedy blocking fill + one-shot parse; remap `Invalid`/`Eof`→`IO`, or `TOO_BIG` at `max_size` | Not call-for-call parity with C on-demand `reserve_fill` / incremental `try_parse`. |
| `tree.max_size` | Caps stream fill growth; does **not** reject preloaded `data_length > max_size` | C `max_size` is max message / fill accumulation, not whole multi-message buffer length. |

### Dual ABI

Two Cargo layouts (embed-writer default vs `full-suite-abi`), not one mega-struct.
Layout checks: `tests/port/full_abi_layout.rs` and frozen-link. Everything links
`staticlib` (Windows needs suite symbols). Link may use
`-Wl,--allow-multiple-definition` for `mpack-platform.c` vs Rust export overlap.

### Memory ownership

Pointers returned to C use `test_malloc` / `test_free`. Under
`--cfg mpack_frozen_link`, file/track buffers and stdio also go through suite
`test_*`. Otherwise cargo-test shims or libc apply (`src/ffi/suite_libc.rs`).

### Safe-core shapes

Expect: free functions on `&mut Reader<'_>`; `ExpectCompound` for `*_or_nil`.
Node: locked `Tree`/`Node` (`&[u8]`, map required/optional/contains, `enum_str`);
no `*_alloc` / C string copies in safe core. Signature changes need a row above.

## Differential fuzz

| Decision | Choice | Why |
| --- | --- | --- |
| Tooling | `cargo-fuzz` on Linux/WSL; driver `python3 fuzz/run_all.py` | Coverage-guided; optional — not a substitute for frozen gates. |
| C oracle | Pinned upstream objects behind `oracle_*` | Suite link ≠ differential oracle. |
| Rust side | `default-features = false` (no `ffi`) in `fuzz/` | Avoid `#[no_mangle]` clashes with C `mpack_*`. |
| Targets | `reader_diff`, `node_diff`, `total_diff`, `expect_diff`, `writer_diff`; `fuzz_ffi/ffi_crash` | Diff digests vs crash-only smoke (no oracle). |
| Digests | Sticky error + packed records; `bytes_used=0` on error; expect ops in first byte; `str_match` harness masks 7-bit ASCII | Cursor noise and signed-`char` footguns intentionally weakened/harnessed. |
| Evidence | [`fuzz/log.txt`](fuzz/log.txt) records ≥60s clean windows (0 digest panics / crashes) | Supports the differential-fuzz survivor narrative; re-run with `--seconds 60` to refresh. |
| Cleanup | Manual `py -3 tools/upstream_mpack.py cleanup` | Fetch is retained after runs. |

Upstream C finding (null-write UB under UBSan): full write-up in
[`fuzz/c_findings.md`](fuzz/c_findings.md). Rust hardens the corresponding writer
FFI entry points (see Live divergences).

## Explicit non-goals

- Replacing the frozen C suite with `tests/port/` as sole correctness proof.
- Wrapping the C library from Rust (disallowed by Port Mortem).
- Satisfying embed-writer and everything layouts in one library build.
- Editing `tests/original/` (or any frozen C suite path).
