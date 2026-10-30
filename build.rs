use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=src/jihoo");
    for name in ["JIHOO_BIN", "JIHOO_PATH", "JIHOO_LLVM", "JIHOO_BACKEND"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    if env::var_os("CARGO_FEATURE_JIHOO_KERNEL").is_none() {
        return;
    }
    assert_eq!(env::var("TARGET").unwrap(), "x86_64-unknown-uefi");
    let compiler = match env::var_os("JIHOO_BIN") {
        Some(path) => PathBuf::from(path),
        None if PathBuf::from("../jihoo/compiler").is_dir() => {
            println!("cargo:rerun-if-changed=../jihoo/compiler");
            println!("cargo:rerun-if-changed=../jihoo/lib");
            println!("cargo:rerun-if-changed=../jihoo/flake.nix");
            let output = Command::new("nix")
                .args([
                    "build",
                    "path:../jihoo#jihoo",
                    "--no-link",
                    "--print-out-paths",
                ])
                .output()
                .expect("run nix to build ../jihoo, or set JIHOO_BIN");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            PathBuf::from(String::from_utf8(output.stdout).unwrap().trim()).join("bin/jihoo")
        }
        None => PathBuf::from("jihoo"),
    };
    let object = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("kernel.obj");
    let status = Command::new(compiler)
        .args([
            "emit-obj",
            "--lib",
            "-O2",
            "-mno-red-zone",
            "-mgeneral-regs-only",
            "src/jihoo/kernel.jh",
            "-o",
        ])
        .arg(&object)
        .status()
        .expect("run the jihoo compiler");
    assert!(status.success(), "jihoo kernel compilation failed");
    println!("cargo:rustc-link-arg={}", object.display());
    println!("cargo:rerun-if-env-changed=CC");
    let arch = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("arch.obj");
    let status = Command::new(env::var_os("CC").unwrap_or_else(|| "clang".into()))
        .args([
            "-target",
            "x86_64-unknown-windows",
            "-ffreestanding",
            "-fno-pic",
            "-c",
            "src/jihoo/arch.S",
            "-o",
        ])
        .arg(&arch)
        .status()
        .expect("assemble the kernel architecture helpers with clang");
    assert!(status.success(), "kernel architecture assembly failed");
    println!("cargo:rustc-link-arg={}", arch.display());
}
