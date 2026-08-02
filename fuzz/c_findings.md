# Upstream C bug hunt (fuzz + test suites)

Date: 2026-08-02. Method: original C unit suite + sanitizers + AFL++ on
`test/fuzz/fuzz.c` + differential `reader_diff` / `node_diff`. **No** source
mining of C TODOs; findings come only from failing checks / sanitizer reports /
fuzzer crashes.

## Suite results

| Run | Result |
| --- | --- |
| `run-everything-debug` | `0 failures in 1032103 checks` |
| `run-sanitize-address-debug` | `0 failures in 1032103 checks` |
| `run-sanitize-undefined-debug` | `0 failures` in harness counts, **but 1 UBSan runtime error** (below) |
| AFL++ `mpack-fuzz` + ASan, `-V 600` | `797168` execs, `10` cycles, corpus `1954`, **0 crashes / 0 hangs**, stability `100%` |
| `reader_diff` `-max_total_time=300 -max_len=65536` | `9074480` runs, exit 0, **0 crashes** |
| `node_diff` `-max_total_time=300 -max_len=65536` | `1963181` runs, exit 0, **0 crashes** |

Commands (from `original_c/mpack-develop/` unless noted):

```bash
CC=gcc python3 test/unit/configure.py
CC=gcc ninja -f .build/unit/build.ninja run-everything-debug
CC=gcc ninja -f .build/unit/build.ninja run-sanitize-address-debug
CC=gcc ninja -f .build/unit/build.ninja run-sanitize-undefined-debug

AFL_USE_ASAN=1 make -f test/fuzz/Makefile CC=afl-clang-fast
AFL_SKIP_CPUFREQ=1 AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES=1 \
  ASAN_OPTIONS='abort_on_error=1:detect_leaks=0:symbolize=0' \
  afl-fuzz -i test/messagepack -o .build/fuzz/sync -V 600 -M fuzzer01 -- \
  .build/fuzz/mpack-fuzz
```

From repo root:

```bash
export CXX=g++
export RUSTFLAGS="-C linker=g++"
cargo +nightly fuzz run reader_diff --fuzz-dir fuzz -- -max_total_time=300 -max_len=65536
cargo +nightly fuzz run node_diff --fuzz-dir fuzz -- -max_total_time=300 -max_len=65536
```

## Finding: `mpack_write_str(NULL, 0)` is C UB

### Discovery path

`run-sanitize-undefined-debug` prints (unique message once per run):

```text
src/mpack/mpack-writer.c:1266:13: runtime error: null pointer passed as argument 2,
which is declared to never be null
```

With `UBSAN_OPTIONS=print_stacktrace=1`:

```text
#0 mpack_write_str          src/mpack/mpack-writer.c:1266
#1 test_write_utf8          test/unit/src/test-write.c:1022
#2 test_writes              test/unit/src/test-write.c:1266
#3 main                     test/unit/src/test.c:94
```

The unit test **expects success**:

```c
TEST_SIMPLE_WRITE_NOERROR(mpack_write_str(&writer, NULL, 0));
```

(also `mpack_write_utf8(&writer, NULL, 0)` nearby).

### Mechanism (post-hit triage)

For `count <= 31`, C encodes a fixstr then calls `mpack_memcpy(..., data, count)`.
With `data == NULL` and `count == 0`, that is still undefined behavior under C
(libstdc++/glibc `memcpy` is `__nonnull` on the source). The suite counts this as
a passing check because UBSan recovers by default and the encode still produces
empty fixstr `0xa0` in practice.

### Minimal trigger

Any growable/fixed writer in OK state:

```c
mpack_write_str(&writer, NULL, 0);
```

No crafted MessagePack input is required; AFL on `fuzz.c` (Reader→Writer→Node
over stdin) does not exercise this API shape, which is why AFL reported 0 crashes.

### Rust port status

`src/ffi/writer.rs` `write_c_bytes` already treats `data.is_null() && count == 0`
as an empty `&[]` and never passes a null pointer into a copy. Non-null
requirement is only enforced when `count != 0` (sticky `mpack_error_bug`).
Safe core takes `&[u8]` and cannot express null.

### Severity

- **Class**: C undefined behavior (null to `memcpy` with zero length).
- **Observed effect under UBSan**: diagnostic only; harness still reports
  `0 failures`.
- **Practical risk**: low on common libcs (often a no-op for `n==0`), but still
  non-portable / invalid C; sanitizer builds correctly flag it.
