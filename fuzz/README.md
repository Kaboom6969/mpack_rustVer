# Differential fuzz (C MPack vs Rust safe core) + FFI crash harness

The C oracle is fetched at runtime from `.port-mortem.toml` and cached under:

```text
target/upstream/mpack/pinned/
```

Resolution order is:
1. `kickoff_hash` if it is a valid upstream MPack commit
2. `source_version` tag (`v<version>` first, then `<version>`)

## Packages

| Package                       | Role                                                                                                                          |
|-------------------------------|-------------------------------------------------------------------------------------------------------------------------------|
| [`fuzz/`](.)                  | C↔Rust **digest diffs** (`mpack` with `default-features = false` so Rust FFI `#[no_mangle]` does not clash with the C oracle) |
| [`../fuzz_ffi/`](../fuzz_ffi) | Crash-only FFI harness (`mpack` with `full-suite-abi`; **no** C oracle)                                                       |

## Targets

| Target        | Package    | Oracle / mode                                                                                          |
|---------------|------------|--------------------------------------------------------------------------------------------------------|
| `reader_diff` | `fuzz`     | One top-level value: iterative `read_tag` + raw str/bin/ext payloads                                   |
| `node_diff`   | `fuzz`     | `Tree::parse` / `mpack_tree_parse` + preorder tag digest                                               |
| `total_diff`  | `fuzz`     | Same input: both reader and node digests must match C vs Rust                                          |
| `expect_diff` | `fuzz`     | Opcode stream + MessagePack payload → `mpack_expect_*` digest                                          |
| `writer_diff` | `fuzz`     | Read→rewrite growable transfer (mirrors upstream AFL `fuzz.c`)                                         |
| `ffi_crash`   | `fuzz_ffi` | Opcode-driven FFI init/write/read/expect/node/destroy (**crash smoke only**, no C oracle / not parity) |

Depth is capped at 1024 (aligned with upstream `test/fuzz/fuzz.c`). Reader/node
digests record sticky error, tag records (type / aux / scalar / payload FNV-1a),
and `bytes_used` **only on success** (truncated cursor advancement is normalized
away — see `DECISIONS.md`). Expect digests compare **precise** sticky error codes
(and may exercise UTF-8 expects). The C oracle builds with
`MPACK_READ/WRITE_TRACKING=0`, so writer transfer does not cover `done_*`.
`ffi_crash` is crash smoke only (no C oracle). The node oracle walk is recursive
at that depth limit; extreme nesting can overflow the fuzz worker stack
(reliability of the harness, not a production ABI claim).

## Requirements (WSL / Linux)

- Nightly Rust (`rustup toolchain install nightly`)
- `cargo-fuzz` (`cargo +nightly install cargo-fuzz`)
- C toolchain + C++ headers for libFuzzer (`g++`, `libstdc++-*-dev`)

### Clang as default `cc` / `c++` (common on this VM)

libFuzzer needs a C++ runtime. When the default `c++` is Clang and cannot find
libstdc++ cleanly, force GCC’s linker driver (same spirit as the C suite’s
`CC=gcc` guidance in `AGENTS.md`):

```bash
export CXX=g++
export RUSTFLAGS="-C linker=g++"
cargo +nightly fuzz run reader_diff --fuzz-dir fuzz
```

## Run all targets

```bash
# from repo root (Linux or WSL)
python3 fuzz/run_all.py                  # 60s per target, -max_len=65536
python3 fuzz/run_all.py --seconds 10     # shorter smoke
python3 fuzz/run_all.py --seconds 0 --runs 100
```

`run_all.py` sets `CXX=g++` / `RUSTFLAGS=-C linker=g++` when unset, resolves the
pinned upstream checkout before the first target build, then runs every target
above in order and exits non-zero if any fail. It does not delete the fetched
checkout automatically.

**What “ok” means:** libFuzzer exit 0. Diff targets compare C vs Rust digests on
**every** input and `panic!` on mismatch, so exit 0 after `Done N runs` means
**N comparisons, 0 divergences**. The summary line prints that explicitly.
`ffi_crash` only asserts no crash (not parity).

## Run individually

```bash
cargo +nightly fuzz run reader_diff --fuzz-dir fuzz -- -max_len=65536
cargo +nightly fuzz run node_diff --fuzz-dir fuzz -- -max_len=65536
cargo +nightly fuzz run total_diff --fuzz-dir fuzz -- -max_len=65536
cargo +nightly fuzz run expect_diff --fuzz-dir fuzz -- -max_len=65536
cargo +nightly fuzz run writer_diff --fuzz-dir fuzz -- -max_len=65536
cargo +nightly fuzz run ffi_crash --fuzz-dir fuzz_ffi -- -max_len=65536

# timed smoke
cargo +nightly fuzz run expect_diff --fuzz-dir fuzz -- \
  -max_total_time=60 -max_len=65536

# manual cleanup after review
py -3 tools/upstream_mpack.py cleanup
```

libFuzzer’s default `-max_len` is **4096**, while the oracle caps at
`ORACLE_MAX_INPUT` / `MAX_INPUT_LEN` (**65536**). Pass `-max_len=65536` when
you want coverage of the large-input truncation path.

Direct `cargo +nightly fuzz run ...` still auto-fetches the pinned upstream
checkout through `fuzz/build.rs`, but it does not auto-clean on exit. Use the
helper script above only when you want to remove the fetched checkout manually
after the run is complete.

Seeds live under `corpus/<target>/` (and `fuzz_ffi/corpus/ffi_crash/`).
Artifacts and `*/target/` are gitignored. Hex-named libFuzzer units under
`corpus/` are also gitignored; hand-named seeds stay tracked.

## Layout

```text
fuzz/
  Cargo.toml          # mpack-fuzz package (cargo-fuzz metadata)
  build.rs            # resolves pinned upstream C + compiles oracle_*.c
  run_all.py          # sequential driver for all targets
  c/                  # mpack-config.h + oracle helpers
  src/                # Rust digest mirror + oracle FFI
  fuzz_targets/       # reader/node/total/expect/writer_diff
  corpus/             # seed inputs
fuzz_ffi/
  Cargo.toml          # mpack-fuzz-ffi (full-suite-abi, no C oracle)
  fuzz_targets/       # ffi_crash
  corpus/ffi_crash/   # seed inputs
```
