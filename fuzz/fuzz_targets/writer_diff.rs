#![no_main]

use libfuzzer_sys::fuzz_target;
use mpack_fuzz::{writer_transfer_c, writer_transfer_rust, MAX_INPUT_LEN};

fuzz_target!(|data: &[u8]| {
    let data = if data.len() > MAX_INPUT_LEN {
        &data[..MAX_INPUT_LEN]
    } else {
        data
    };
    let rust = writer_transfer_rust(data);
    let c = writer_transfer_c(data);
    if rust != c {
        panic!(
            "writer_diff divergence\ninput_prefix={:02x?}\nrust={rust:?}\nc={c:?}",
            &data[..data.len().min(64)]
        );
    }
});
