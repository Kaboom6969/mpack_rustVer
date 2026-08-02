---
description: Safe-core teammate constraints (always on); lead/FFI work is explicitly exempt
alwaysApply: true
---

# Safe-core teammate constraints

Always applied. **Lead / FFI owner is exempt** when doing API-contract updates (document in `DECISIONS.md`) or any work under `src/ffi/**` / frozen-link wiring.

When the task is **Reader / Expect / Node safe-core implementation** (teammate lane), follow the constraints below strictly.

## Allowed paths (teammate lane)

- May edit **only your assigned module**: `src/reader.rs`, `src/expect.rs`, or
  `src/node.rs`, plus tests under `tests/port/` for that module.
- May read `original_c/` for behavior; do not copy pointer-style APIs into safe core.
- **Do not edit:** `src/ffi/**`, `tests/original/**`, frozen-link C adapters.
- Do not edit another teammate's module (e.g. Node lane must not touch `expect.rs`).

## Zero unsafe (hard)

- `reader`, `expect`, and `node` are `forbid(unsafe_code)`. No `unsafe`, no raw
  pointers in those modules.
- No C callbacks, no `malloc`/`free`, no allocator-owned pointer returns from safe core.
- Use `&[u8]`, `&mut [u8]`, `Tag`, `Error`, `Timestamp`, `Option` / `bool` only.

## Frozen public API (teammate lane)

- Do not change public signatures without lead approval (`DECISIONS.md` → Expect / Node tables and “Safe-core surface shapes” hotspot).
- Fill or fix function bodies; add port tests only.
- Expect stays free functions on `&mut Reader<'_>`.
- Keep `ExpectCompound` for `*_or_nil`. Use `r#bool` / `r#str` / `true_` / `false_`.
- Node stays the **minimal** locked surface (`Tree` / `Node`, `type_`, `&[u8]`
  payloads). Do not add optional/contains/enum/`*_alloc` without lead approval.
- Do not add `*_alloc` or FFI-shaped APIs to expect/reader/node.

## Out of scope for teammates

Streaming fill/skip, file init, `done_*` tracking, C `char*` copies, frozen-suite wiring → lead/FFI only.

## Done means (teammate lane)

- `cargo test` (including `tests/port` cases) passes.
- No public API drift; no `unsafe` in reader/expect/node.
- Do not claim `test-reader.c` / `test-expect.c` / `test-node.c` green — that is after FFI by the lead.

## Lead / FFI exemption

If the user is implementing or changing FFI, types layout, frozen-link, or intentionally revising the frozen safe-core contract: **ignore the teammate path/API deny-list above**; still keep `unsafe` out of `forbid(unsafe_code)` modules (put raw pointers only in `src/ffi/`).
