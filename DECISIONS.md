# DECISIONS

Important divergences from MPack (C) only. Keep this short.

| Decision | Choice | Why |
| --- | --- | --- |
| Architecture | Safe Rust core + C-ABI FFI | Frozen suite links unchanged; core stays `forbid(unsafe_code)`. |
| Dual ABI | Default embed-writer vs `full-suite-abi` | Mutually exclusive layouts; one build cannot do both. |
| Frozen tests | `tests/original/` unchanged | Port Mortem rule; new tests go in `tests/port/`. |
| Null write `(ptr, 0)` | Treat as empty; null+nonzero → `bug` | Upstream C is UB on `mpack_write_str(NULL,0)`; we harden it. |
| Builder pages | Side-table by writer ptr; ABI `builder` empty | Avoid extra unsafe inside `mpack_writer_t`. |
| Deep discard / print / tree | Iterative stacks | Hostile nesting must not blow the Rust stack. |
| UTF-8 / timestamp fail (reader) | Sticky error; do not advance `used()` | May diverge from C cursor on the same paths. |
| Node `as_f32` | `Tag::Float` only | Minimal safe-core; no int/double widen like C. |
| Optional map miss | `None` (no sticky error) | Safe core has no C-style `missing` node. |
| Node stream fill | Blocking fill + one-shot parse | Not call-for-call with C incremental `try_parse`. |

## Non-goals

- Wrapping the C library from Rust.
- Editing `tests/original/`.
- Using `tests/port/` alone as parity proof.
