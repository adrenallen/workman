use std::{env, path::PathBuf, process::Command};

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        link_macos_compiler_runtime();
    }
    tauri_build::build()
}

fn link_macos_compiler_runtime() {
    let output = Command::new("xcrun")
        .args(["clang", "--print-resource-dir"])
        .output()
        .expect("failed to locate Apple's clang runtime with xcrun");
    assert!(
        output.status.success(),
        "xcrun clang --print-resource-dir failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let resource_dir = String::from_utf8(output.stdout)
        .expect("Apple clang resource directory is not valid UTF-8");
    let runtime_dir = PathBuf::from(resource_dir.trim()).join("lib/darwin");
    let runtime = runtime_dir.join("libclang_rt.osx.a");
    assert!(
        runtime.is_file(),
        "Apple clang runtime was not found at {}",
        runtime.display()
    );

    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");
    println!("cargo:rustc-link-search=native={}", runtime_dir.display());
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
}
