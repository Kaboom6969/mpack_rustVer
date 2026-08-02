#![no_main]

//! Combined differential: same input must match C vs Rust on both reader and node digests.

use libfuzzer_sys::fuzz_target;
use mpack_fuzz::{
    node_digest_c, node_digest_rust, reader_digest_c, reader_digest_rust, MAX_INPUT_LEN,
};

fuzz_target!(|data: &[u8]| {
    let data = if data.len() > MAX_INPUT_LEN {
        &data[..MAX_INPUT_LEN]
    } else {
        data
    };

    let rust_reader = reader_digest_rust(data);
    let c_reader = reader_digest_c(data);
    if rust_reader != c_reader {
        panic!(
            "total_diff reader divergence\ninput_prefix={:02x?}\nrust={rust_reader:?}\nc={c_reader:?}",
            &data[..data.len().min(64)]
        );
    }

    let rust_node = node_digest_rust(data);
    let c_node = node_digest_c(data);
    if rust_node != c_node {
        panic!(
            "total_diff node divergence\ninput_prefix={:02x?}\nrust={rust_node:?}\nc={c_node:?}",
            &data[..data.len().min(64)]
        );
    }
});
