use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=GBUILD_BUILD_ID");

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let repository = manifest_dir.join("../..");
    println!(
        "cargo:rerun-if-changed={}",
        repository.join(".git/HEAD").display()
    );
    if let Ok(head) = std::fs::read_to_string(repository.join(".git/HEAD"))
        && let Some(reference) = head.trim().strip_prefix("ref: ")
    {
        println!(
            "cargo:rerun-if-changed={}",
            repository.join(".git").join(reference).display()
        );
    }

    let build_id = env::var("GBUILD_BUILD_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            Command::new("git")
                .args(["rev-parse", "--short=12", "HEAD"])
                .current_dir(&repository)
                .output()
                .ok()
                .filter(|output| output.status.success())
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| env::var("CARGO_PKG_VERSION").unwrap());

    println!("cargo:rustc-env=GBUILD_BUILD_ID={build_id}");
}
