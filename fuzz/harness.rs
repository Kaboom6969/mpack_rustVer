//! Differential fuzzer template: compare C MPack vs the Rust port on the same bytes.
//!
//! This is a scaffold only — not a completed fuzzer. When wired up:
//!
//! 1. Generate or mutate MessagePack byte strings.
//! 2. Decode with C MPack (`original_c/mpack-develop/src/mpack/`) via FFI or a
//!    thin C helper (outside `tests/original/`).
//! 3. Decode with the Rust port (`mpack::reader` / `mpack::node`).
//! 4. Compare sticky errors, tags, and scalar payloads; log any divergence.
//!
//! Suggested build (once both sides link):
//!
//! ```text
//! rustc --edition 2021 fuzz/harness.rs -L target/release/deps \
//!   -l mpack -o target/fuzz-harness
//! ```
//!
//! Or add a `[[bin]]` / cargo-fuzz target later. Do not claim a bonus without a
//! real ≥60s run recorded in `fuzz/log.txt` with zero divergences.

fn main() {
    eprintln!(
        "mpack differential fuzzer template — not yet linked to C and Rust decoders"
    );
    eprintln!("Feed identical MessagePack bytes to both sides and compare tags/errors.");
    // Placeholder corpus byte (MessagePack nil = 0xc0).
    let sample: &[u8] = &[0xc0];
    let _ = sample;
}
