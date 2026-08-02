use std::env;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let repo = manifest_dir.parent().expect("fuzz/ is under repo root");
    let oracle_c = manifest_dir.join("c");
    let mpack_src = repo.join("original_c/mpack-develop/src");
    let mpack_dir = mpack_src.join("mpack");

    let sources = [
        mpack_dir.join("mpack-common.c"),
        mpack_dir.join("mpack-platform.c"),
        mpack_dir.join("mpack-reader.c"),
        mpack_dir.join("mpack-node.c"),
        oracle_c.join("oracle_reader.c"),
        oracle_c.join("oracle_node.c"),
    ];

    for source in &sources {
        println!("cargo:rerun-if-changed={}", source.display());
    }
    println!(
        "cargo:rerun-if-changed={}",
        oracle_c.join("mpack-config.h").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        oracle_c.join("oracle_digest.h").display()
    );

    cc::Build::new()
        .files(&sources)
        .include(&oracle_c)
        .include(&mpack_src)
        .define("MPACK_HAS_CONFIG", "1")
        .flag_if_supported("-std=c11")
        .warnings(false)
        .compile("mpack_c_oracle");
}
