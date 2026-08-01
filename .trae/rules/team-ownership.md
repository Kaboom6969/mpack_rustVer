---
description: Teammate ownership lanes, shared ABI locks, and per-module frozen-suite merge gates
alwaysApply: true
---

# Team Ownership (parallel slices)

Prefer **one lane per PR**. A lane is **not done** until its **module acceptance** below passes.

## Lanes → module acceptance (hard gate)

| Lane | Own paths | Must pass |
| --- | --- | --- |
| **Reader** | `src/reader.rs`; replace stubs with `src/ffi/reader.rs` | `tests/port` reader tests **and** frozen suite: **0 failures** from `test-reader.c` (and reader parts of `test-buffer.c` / `test-file.c` you touch) under `python3 tests/port/frozen-link/run.py --full --everything` |
| **Expect** | `src/expect.rs`; replace `src/ffi/stubs/expect.rs` | port expect tests (when added) **and** **0 failures** from `test-expect.c` under `--full --everything` |
| **Node** | `src/node.rs`; replace `src/ffi/stubs/node.rs` | port node tests (when added) **and** **0 failures** from `test-node.c` under `--full --everything` |
| **Writer + integration** | `src/writer.rs`, `src/ffi/writer.rs`, gates, `DECISIONS.md` | `cargo test` writer tests **and** `python3 tests/port/frozen-link/run.py --full` (**embed-writer**) reports **0 failures**; builder: also clear `test-builder.c` under everything when that surface is claimed |

**How to judge module green:** suite is one binary; attribute failures by path in `TEST FAILED AT …/test-<module>.c`. Other modules may still fail—that is OK until their lane owns them. Crashes (`exit < 0` / `>= 128`) are never OK.

Dependency: common → writer/reader → expect → node. **Reader FFI before Expect/Node module-green.**

## Shared locks (serialize / explicit review)

1. **`src/ffi/types.rs`** / `full-suite-abi` layout: one changer; run `full_abi_layout` + C layout check.
2. Default embed-writer vs `full-suite-abi`: mutually exclusive; do not require both layouts green in one PR unless intentional.
3. **Never edit** `tests/original/`.

## Working rules

- Replace stub bodies **only in your lane**; no `unsafe` growth in `forbid(unsafe_code)` modules—FFI calls safe core.
- Safe core first, then FFI. New tests only under `tests/port/`.
- Intermediate PRs may leave other modules red; **your claimed module must be green** before calling the lane done.
- Divergences → `DECISIONS.md` (English).

## Agents / AIs

These project rules auto-apply in this repo (`alwaysApply`). Before marking work complete, **run the lane’s gate commands** and cite the suite summary (or `TEST FAILED` grep for your `test-*.c`). Do not claim parity from port-only tests alone.
