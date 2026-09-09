use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn run(mut command: Command, description: &str) {
    let status = command.status().unwrap_or_else(|error| {
        panic!("failed to start {description}: {error}");
    });
    if !status.success() {
        panic!("{description} failed with status {status}");
    }
}

fn main() {
    println!("cargo:rerun-if-changed=../../native/warpigs/src/warpigs.cpp");
    println!("cargo:rerun-if-changed=../../native/warpigs/include/warpigs.h");

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR missing"));
    let object = out.join("warpigs.o");
    let archive = out.join("libwarpigs_core.a");

    let source = PathBuf::from("../../native/warpigs/src/warpigs.cpp");
    let include = PathBuf::from("../../native/warpigs/include");

    let mut compile = Command::new("c++");
    compile
        .arg("-std=c++20")
        .arg("-O2")
        .arg("-Wall")
        .arg("-Wextra")
        .arg("-Wpedantic")
        .arg("-Wconversion")
        .arg("-Wshadow")
        .arg("-c")
        .arg(&source)
        .arg("-I")
        .arg(&include)
        .arg("-o")
        .arg(&object);
    run(compile, "C++ WarPigs compilation");

    if archive.exists() {
        fs::remove_file(&archive).expect("remove stale archive");
    }
    let mut ar = Command::new("ar");
    ar.arg("crus").arg(&archive).arg(&object);
    run(ar, "C++ WarPigs archive creation");

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=warpigs_core");
    println!("cargo:rustc-link-lib=dylib=stdc++");
}
