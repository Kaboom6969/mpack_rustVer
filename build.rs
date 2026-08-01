fn main() {
    #[cfg(feature = "ffi-harness")]
    {
        use std::env;
        use std::path::PathBuf;

        let repository = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let harness = repository.join("tests/port/ffi-harness");
        let source = harness.join("c/harness.c");
        let config_include = harness.join("include");
        let upstream_include = repository.join("original_c/mpack-develop/src");

        println!("cargo:rerun-if-changed={}", source.display());
        println!(
            "cargo:rerun-if-changed={}",
            config_include.join("mpack-config.h").display()
        );

        cc::Build::new()
            .file(source)
            .include(config_include)
            .include(upstream_include)
            .define("MPACK_HAS_CONFIG", "1")
            .flag_if_supported("-std=c11")
            .warnings(true)
            .extra_warnings(true)
            .compile("mpack_ffi_harness");
    }
}
