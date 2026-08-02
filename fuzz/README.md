# Differential fuzz (C MPack vs Rust safe core) + FFI crash harness

## Packages

| Package | Role |
| --- | --- |
| [`fuzz/`](.) | C↔Rust **digest diffs** (`mpack` with `default-features = false` so Rust FFI `#[no_mangle]` does not clash with the C oracle) |
| [`../fuzz_ffi/`](../fuzz_ffi/) | Crash-only FFI harness (`mpack` with `full-suite-abi`; **no** C oracle) |

## Targets

| Target | Package | Oracle / mode |
| --- | --- | --- |
| `reader_diff` | `fuzz` | One top-level value: iterative `read_tag` + raw str/bin/ext payloads |
| `node_diff` | `fuzz` | `Tree::parse` / `mpack_tree_parse` + preorder tag digest |
| `total_diff` | `fuzz` | Same input: both reader and node digests must match C vs Rust |
| `expect_diff` | `fuzz` | Opcode stream + MessagePack payload → `mpack_expect_*` digest |
| `writer_diff` | `fuzz` | Read→rewrite growable transfer (mirrors upstream AFL `fuzz.c`) |
| `ffi_crash` | `fuzz_ffi` | Opcode-driven FFI init/write/read/expect/node/destroy (**crash smoke only**, no C oracle / not parity) |

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

`run_all.py` sets `CXX=g++` / `RUSTFLAGS=-C linker=g++` when unset, then runs
every target above in order and exits non-zero if any fail.

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
```

libFuzzer’s default `-max_len` is **4096**, while the oracle caps at
`ORACLE_MAX_INPUT` / `MAX_INPUT_LEN` (**65536**). Pass `-max_len=65536` when
you want coverage of the large-input truncation path.

Seeds live under `corpus/<target>/` (and `fuzz_ffi/corpus/ffi_crash/`).
Artifacts and `*/target/` are gitignored. Hex-named libFuzzer units under
`corpus/` are also gitignored; hand-named seeds stay tracked.

## Layout

```text
fuzz/
  Cargo.toml          # mpack-fuzz package (cargo-fuzz metadata)
  build.rs            # compiles original C + oracle_*.c
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
