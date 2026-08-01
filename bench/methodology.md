# Benchmark methodology

Compare the C reference MPack (`original_c/mpack-develop/`) against the Rust port
(`src/`) on the same MessagePack workloads.

## Workloads

1. **Encode throughput** — write a fixed nested map/array document (ints, strings,
   bins) into a growable buffer; measure documents/s and MB/s.
2. **Decode throughput** — parse the same fixture with the reader (streaming) and
   with the node/tree API; measure documents/s and MB/s.
3. **p99 latency** — single-document encode and decode wall time over ≥10k
   iterations; report p50 / p99 / max.
4. **RSS** — peak resident set size while decoding a large (~10–50 MiB) blob.
5. **Startup** — time from process start to first successful nil encode/decode
   (cold binary, no warm-up).

## Procedure

- Build both sides in release / optimized mode (`cargo build --release`; C with
  `-O2` or the unit suite’s release flags).
- Pin CPU frequency / use a quiet machine when possible; run each metric ≥5 times
  and record median.
- Use identical fixtures (e.g. `tests/original/test/messagepack/*.mp` plus a
  generated nested document of known size).
- Record environment in `results.json` (`cpu`, `os`, `rustc`, `cc`).

## Out of scope (for now)

Results in `results.json` are placeholders until the port implements encode/decode
and measurements are collected. Do not treat null metrics as measured parity.
