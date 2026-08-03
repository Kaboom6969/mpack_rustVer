fn main() {
    #[cfg(feature = "ffi-harness")]
    {
        use std::env;
        use std::path::PathBuf;

        let repository = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let harness = repository.join("tests/port/ffi-harness");
        let source = harness.join("c/harness.c");
        let config_include = harness.join("include");
        let upstream_include = resolve_upstream_include(&repository);
        validate_upstream_include(&upstream_include);

        println!("cargo:rerun-if-env-changed=MPACK_UPSTREAM_SRC");
        println!("cargo:rerun-if-changed={}", source.display());
        println!(
            "cargo:rerun-if-changed={}",
            config_include.join("mpack-config.h").display()
        );
        for header in [
            "mpack.h",
            "mpack-platform.h",
            "mpack-common.h",
            "mpack-writer.h",
            "mpack-reader.h",
            "mpack-expect.h",
            "mpack-node.h",
        ] {
            println!(
                "cargo:rerun-if-changed={}",
                repository
                    .join("include/upstream/mpack")
                    .join(header)
                    .display()
            );
        }

        cc::Build::new()
            .file(source)
            .include(config_include)
            .include(upstream_include)
            .define("MPACK_HAS_CONFIG", "1")
            .flag_if_supported("-std=c11")
            .warnings(true)
            .extra_warnings(true)
            .compile("mpack_ffi_harness");

        fn resolve_upstream_include(repo: &PathBuf) -> PathBuf {
            if let Some(override_path) = env::var_os("MPACK_UPSTREAM_SRC") {
                return PathBuf::from(override_path);
            }
            repo.join("include").join("upstream")
        }

        fn validate_upstream_include(include_root: &PathBuf) {
            let header = include_root.join("mpack").join("mpack.h");
            if header.is_file() {
                return;
            }
            panic!(
                "Invalid MPACK_UPSTREAM_SRC/include root: {} (expected mpack/mpack.h)",
                include_root.display()
            );
        }
    }
}
