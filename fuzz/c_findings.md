# Upstream C bug hunt (fuzz + test suites)

Date: 2026-08-02. Method: original C unit suite + sanitizers + AFL++ on
`test/fuzz/fuzz.c` + differential `reader_diff` / `node_diff`. **No** source
mining of C TODOs; findings come only from failing checks / sanitizer reports /
fuzzer crashes.

This is a **C language UB** finding (null to `memcpy` with `n == 0`), not a
remote / MessagePack-byte-triggered memory corruption. Do not treat it as a
CVE-class network vulnerability when reporting upstream.

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

Note: AFL `detect_leaks=0` / `AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES` are fuzz
harness tradeoffs and do not hide ASan crashes. **Zero AFL crashes does not mean
the writer `(NULL, 0)` API is clean** — `fuzz.c` never calls that shape.

## Finding: `mpack_write_str(NULL, 0)` is C UB

### Discovery path

`run-sanitize-undefined-debug` prints (unique message once per run; UBSan
dedupes the same PC when both `write_str` and `write_utf8` hit it):

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
TEST_SIMPLE_WRITE_NOERROR(mpack_write_utf8(&writer, NULL, 0));
```

### Mechanism (post-hit triage)

Both build flavors pass a null source into `mpack_memcpy` when `count == 0`:

| `MPACK_OPTIMIZE_FOR_SIZE` | Path | UBSan site |
| --- | --- | --- |
| `0` (suite default) | fixstr fast path encodes header then `mpack_memcpy(..., data, count)` | `mpack-writer.c:1266` |
| `1` | `mpack_start_str_notrack` then `mpack_write_native` → `mpack_memcpy` | `mpack-writer.c:526` |

`mpack_write_utf8(NULL, 0)` is the same underlying UB: `mpack_utf8_check(str, 0)`
does not read the pointer, then it calls `mpack_write_str(writer, str, 0)`.

Related note (not the suite hit): `mpack_assert(count == 0 || data != NULL)` means
`NULL` + **nonzero** `count` is fatal in debug / may be treated as unreachable in
release; that is a different contract from the `(NULL, 0)` UB above.

The suite counts `(NULL, 0)` as a passing check because UBSan recovers by
default and the encode still produces empty fixstr `0xa0` with `error == 0`.

### Minimal SIZE=0 / SIZE=1 repro

```bash
# From a scratch dir; adjust SRC to original_c/mpack-develop/src
SRC=.../original_c/mpack-develop/src
mkdir -p size0 size1
# Write mpack-config.h into size0/ and size1/ with writer-only features and
# MPACK_OPTIMIZE_FOR_SIZE 0 or 1 respectively, then:

cat > repro.c <<'EOF'
#include "mpack/mpack.h"
#include <stdio.h>
int main(void) {
    char buf[64];
    mpack_writer_t w;
    mpack_writer_init(&w, buf, sizeof(buf));
    mpack_write_str(&w, NULL, 0);
    size_t n = mpack_writer_buffer_used(&w);
    mpack_error_t err = mpack_writer_destroy(&w);
    printf("used=%zu err=%d first=0x%02x\n", n, (int)err, n ? (unsigned char)buf[0] : 0);
    return 0;
}
EOF

for size in 0 1; do
  gcc -fsanitize=undefined -g -O0 -DMPACK_HAS_CONFIG=1 \
    -I./size$size -I"$SRC" \
    -c "$SRC/mpack/mpack-platform.c" -o size$size/plat.o
  gcc -fsanitize=undefined -g -O0 -DMPACK_HAS_CONFIG=1 \
    -I./size$size -I"$SRC" \
    -c "$SRC/mpack/mpack-common.c" -o size$size/common.o
  gcc -fsanitize=undefined -g -O0 -DMPACK_HAS_CONFIG=1 \
    -I./size$size -I"$SRC" \
    -c "$SRC/mpack/mpack-writer.c" -o size$size/writer.o
  gcc -fsanitize=undefined -g -O0 -DMPACK_HAS_CONFIG=1 \
    -I./size$size -I"$SRC" \
    repro.c size$size/*.o -o size$size/repro
  UBSAN_OPTIONS=print_stacktrace=1 ./size$size/repro
done
```

Observed (2026-08-02):

```text
SIZE=0 → UBSan at mpack-writer.c:1266; used=1 err=0 first=0xa0
SIZE=1 → UBSan at mpack-writer.c:526  (via write_str → write_native);
         used=1 err=0 first=0xa0
```

Upstream fixes that only patch the fixstr fast path at `:1266` would miss the
`MPACK_OPTIMIZE_FOR_SIZE=1` path through `:526`.

### Rust port status

Behavior is correct on both entry points; the hardening is **not** all via one
helper:

- `mpack_write_str` / `mpack_write_bytes` / `mpack_write_object_bytes` →
  `write_c_bytes`: null+zero → empty `&[]`; null+nonzero → sticky
  `mpack_error_bug`.
- `mpack_write_utf8` → **independent** null/count branch in
  `src/ffi/writer.rs` (does not call `write_c_bytes`), then UTF-8 check + safe
  core write.

Safe core takes `&[u8]` and cannot express a null pointer.

### Severity

- **Class**: C undefined behavior (null to `memcpy` with zero length).
- **Attack surface**: not reachable from untrusted MessagePack bytes alone;
  the caller must pass `(NULL, 0)` into the writer API.
- **Observed effect under UBSan**: diagnostic only; harness still reports
  `0 failures`; output is empty fixstr `0xa0`.
- **Practical risk**: low on common libcs (often a no-op for `n==0`), but still
  non-portable / invalid C; sanitizer builds correctly flag it. Not RCE.

## Finding: `mpack_expect_str_match` high bytes vs signed `char`

### Discovery path

Differential `expect_diff` (precise sticky errors) with expected byte `0xa1`
and matching fixstr payload: C sticky `Type`, Rust ok.

### Root cause

Upstream C (`mpack-expect.c`):

```c
if (mpack_expect_native_u8(reader) != *str++) {
    mpack_reader_flag_error(reader, mpack_error_type);
```

`native_u8` is `uint8_t`; `*str` is `char`. On signed-char hosts (Linux gcc
default), byte `0xa1` promotes to `-95` and does not equal `161`.

### Port / fuzz stance

Safe-core `expect::str_match` compares `&[u8]` (correct). Fuzz harness masks
`str_match` expected bytes to 7-bit ASCII on both sides so digests stay fair.
Frozen suite only exercises ASCII `cstr_match`.
