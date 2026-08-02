# Differential fuzz (C MPack vs Rust safe core)

In-process libFuzzer targets compare digests from **original C MPack**
(`original_c/mpack-develop/src/mpack/`) against the Rust **safe core**
(`mpack::reader` / `mpack::node`). The fuzz package depends on `mpack` with
`default-features = false` so Rust FFI `#[no_mangle]` symbols are not linked
(avoiding clashes with the C oracle).

## Targets

| Target | Oracle |
| --- | --- |
| `reader_diff` | One top-level value: iterative `read_tag` + raw str/bin/ext payloads |
| `node_diff` | `Tree::parse` / `mpack_tree_parse` + preorder tag digest |

Depth is capped at 1024 (aligned with upstream `test/fuzz/fuzz.c`). Digests
record sticky error, tag records (type / aux / scalar / payload FNV-1a), and
`bytes_used` **only on success** (truncated cursor advancement is normalized
away — see `DECISIONS.md`). The node oracle walk is recursive at that depth
limit; extreme nesting can overflow the fuzz worker stack (reliability of the
harness, not a production ABI claim).

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

## Run

```bash
# from repo root (Linux or WSL)
cargo +nightly fuzz run reader_diff --fuzz-dir fuzz -- -max_len=65536
cargo +nightly fuzz run node_diff --fuzz-dir fuzz -- -max_len=65536

# timed smoke (example); raise -max_len so inputs can reach ORACLE_MAX_INPUT
cargo +nightly fuzz run reader_diff --fuzz-dir fuzz -- \
  -max_total_time=60 -max_len=65536
```

libFuzzer’s default `-max_len` is **4096**, while the oracle caps at
`ORACLE_MAX_INPUT` / `MAX_INPUT_LEN` (**65536**). Pass `-max_len=65536` when
you want coverage of the large-input truncation path.

Seeds live under `corpus/reader_diff/` and `corpus/node_diff/`. Artifacts and
`fuzz/target/` are gitignored. Hex-named libFuzzer units under `corpus/` are
also gitignored; hand-named seeds stay tracked.

## Layout

```text
fuzz/
  Cargo.toml          # mpack-fuzz package (cargo-fuzz metadata)
  build.rs            # compiles original C + oracle_*.c
  c/                  # mpack-config.h + oracle helpers
  src/                # Rust digest mirror + oracle FFI
  fuzz_targets/       # reader_diff, node_diff
  corpus/             # seed inputs
```

This replaces the old `fuzz/harness.rs` scaffold.
