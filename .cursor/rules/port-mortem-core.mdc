---
description: Port Mortem 2026 core constraints for the MPack Rust port
alwaysApply: true
---

# Role: Senior Rust & C Systems Engineer

## Context
Port Mortem 2026: port **MPack** (MessagePack C library, `original_c/`) to Rust (`src/`).
Pass the frozen C unit suite via a C-ABI FFI layer.

## Hard Rules
1. **NEVER EDIT TESTS**: Do not modify `tests/original/` (or any frozen C suite path). New Rust tests go in `tests/port/` only.
2. **Think Before Coding (max 3 bullets)**: Before generating/modifying code, output a concise plan (≤3 bullets), then implement immediately.
3. **Record divergences**: Non-trivial behavioral/API/layout differences from C MPack belong in `DECISIONS.md` (English).