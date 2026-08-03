use std::env;
use std::path::PathBuf;
use std::process::Command;

fn resolve_upstream_src(repo: &PathBuf) -> PathBuf {
    let helper = repo.join("tools").join("upstream_mpack.py");
    let candidates: [(&str, &[&str]); 3] = [
        ("py", &["-3"]),
        ("python3", &[]),
        ("python", &[]),
    ];
    let mut failures = Vec::new();

    for (program, prefix_args) in candidates {
        let mut command = Command::new(program);
        command.args(prefix_args).arg(&helper).arg("ensure");
        match command.output() {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let path = stdout.trim();
                if !path.is_empty() {
                    return PathBuf::from(path);
                }
                panic!(
                    "tools/upstream_mpack.py returned an empty path via {}",
                    program
                );
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                failures.push(format!(
                    "{}: {}{}",
                    program,
                    stderr.trim(),
                    if stdout.trim().is_empty() {
                        String::new()
                    } else {
                        format!(" {}", stdout.trim())
                    }
                ));
            }
            Err(_) => continue,
        }
    }

    panic!(
        "Unable to resolve the pinned upstream MPack checkout via tools/upstream_mpack.py. Tried py -3, python3, and python. Failures: {}",
        failures.join(" | ")
    );
}

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let repo = manifest_dir.parent().expect("fuzz/ is under repo root");
    let oracle_c = manifest_dir.join("c");
    let mpack_src = resolve_upstream_src(&repo.to_path_buf());
    let mpack_dir = mpack_src.join("mpack");

    println!("cargo:rerun-if-changed={}", helper_path(repo).display());
    println!(
        "cargo:rerun-if-changed={}",
        repo.join(".port-mortem.toml").display()
    );

    let sources = [
        mpack_dir.join("mpack-common.c"),
        mpack_dir.join("mpack-platform.c"),
        mpack_dir.join("mpack-reader.c"),
        mpack_dir.join("mpack-writer.c"),
        mpack_dir.join("mpack-expect.c"),
        mpack_dir.join("mpack-node.c"),
        oracle_c.join("oracle_reader.c"),
        oracle_c.join("oracle_node.c"),
        oracle_c.join("oracle_writer.c"),
        oracle_c.join("oracle_expect.c"),
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

fn helper_path(repo: &std::path::Path) -> PathBuf {
    repo.join("tools").join("upstream_mpack.py")
}
