---
description: Use when reviewing a PR, pull request, diff, or code review for this Port Mortem C-to-Rust port
alwaysApply: false
---

# PR Review Rules (Port Mortem)

You are reviewing a PR for a C → Rust port with a C-ABI FFI layer.
Be defect-first: find real bugs, ABI breaks, and test-suite risks. Skip style nitpicks unless they hide bugs.

## Hard blockers (must call out)

1. **Frozen tests touched**: any change under `tests/original/` (or equivalent frozen C suite) is a blocker.
2. **ABI / header drift**: `include/**` structs, enums, constants, or exported symbol names/signatures diverge from original C without a documented reason in `DECISIONS.md`.
3. **Allocator contract break**: C-visible pointers allocated with Rust global allocator, or freed with the wrong hook/`free`.
4. **Unsafe sprawl**: new `unsafe` in core parse/encode paths without a clear `# Safety` boundary; prefer unsafe confined to FFI/node modules.
5. **Silent behavior change**: NULL/error sticky-state, depth limits, UTF-8, float/int width, or MessagePack/JSON edge cases changed without tests + `DECISIONS.md`.

## Architecture checks

- Dual-layer preserved: safe Rust core + `extern "C"` FFI; no “rewrite the C tests in Rust” shortcuts.
- For **cJSON-style** ports: thin adapter outside frozen tree is OK; do not require editing frozen includes.
- For **MPack-style** ports: prefer linking Rust instead of compiling `*.c`; flag unnecessary cJSON-like include-adapters.
- Public headers remain the contract; Rust must match layout and calling convention.

## Review output format

Respond in Chinese unless the user asks otherwise. Structure:

1. **Verdict**: Approve / Request changes / Needs discussion
2. **Blockers**: bullet list (file:line when possible)
3. **Non-blocking**: important nits only
4. **Test gaps**: what should be run or added in `tests/port/` (never edit frozen suite)
5. **DECISIONS.md**: note if a divergence needs documenting

## Severity guide

- **P0**: wrong results vs C, UB/allocator mismatch, frozen tests edited, ABI break
- **P1**: missing error sticky-state, feature-flag skew, leak on error path
- **P2**: unclear safety comments, missing port tests, docs drift

## Do / Don't

- DO compare against `original_c/` when behavior is unclear.
- DO demand evidence: failing/passing original suite, `cargo test`, Miri for pointer-heavy FFI.
- DON'T request refactors unrelated to the PR.
- DON'T approve “looks idiomatic Rust” if it breaks C parity.