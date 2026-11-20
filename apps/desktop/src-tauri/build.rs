use std::path::PathBuf;
use std::process::Command;

/// MinGW's `ld` (binutils 2.40) aborts with "internal error ... compare_section" when linking a binary
/// as large as a Tauri app. The Rust toolchain ships LLD, so on the Windows GNU target link with that
/// instead. MSVC and other targets are untouched.
fn use_bundled_lld_on_windows_gnu() {
    let is = |k: &str, v: &str| std::env::var(k).map(|x| x == v).unwrap_or(false);
    if !(is("CARGO_CFG_TARGET_OS", "windows") && is("CARGO_CFG_TARGET_ENV", "gnu")) {
        return;
    }
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let Ok(out) = Command::new(rustc).args(["--print", "sysroot"]).output() else { return };
    let sysroot = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let target = std::env::var("TARGET").unwrap_or_default();
    let gcc_ld: PathBuf = [&sysroot, "lib", "rustlib", &target, "bin", "gcc-ld"].iter().collect();
    if gcc_ld.exists() {
        println!("cargo:rustc-link-arg-bins=-fuse-ld=lld");
        println!("cargo:rustc-link-arg-bins=-B{}", gcc_ld.display());
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    use_bundled_lld_on_windows_gnu();
    tauri_build::build()
}
