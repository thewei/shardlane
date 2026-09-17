/**
 * [INPUT]: TARGET env triple (Cargo-injected) + per-target vendored archives under
 * vendor/ghostty-vt/lib/<rust-triple>/ (libghostty-vt.a on unix, ghostty-vt.lib on
 * windows-msvc).
 * [OUTPUT]: static-link directives - per-target ghostty-vt search path and C++ runtime.
 * [POS]: repository-root build contract and the multi-platform ABI entry; a missing
 * <triple> directory points at scripts/vendor-ghostty-vt.sh. The legacy single
 * archive remains only as the aarch64-apple-darwin fallback.
 * [PROTOCOL]: Update this header on change, then check CLAUDE.md.
 */
fn main() {
    let manifest_dir = match std::env::var_os("CARGO_MANIFEST_DIR") {
        Some(path) => std::path::PathBuf::from(path),
        None => {
            eprintln!("CARGO_MANIFEST_DIR is required to locate vendored ghostty-vt");
            std::process::exit(1);
        }
    };
    let vendor_root = manifest_dir.join("vendor/ghostty-vt/lib");
    let target = std::env::var("TARGET").unwrap_or_default();

    // Per-target vendored archive layout. The windows-msvc toolchain resolves
    // `static=ghostty-vt` against `ghostty-vt.lib`; unix toolchains resolve
    // against `libghostty-vt.a`.
    let archive = if target.contains("windows") {
        vendor_root.join(&target).join("ghostty-vt.lib")
    } else {
        vendor_root.join(&target).join("libghostty-vt.a")
    };
    let lib_dir = if archive.exists() {
        match archive.parent() {
            Some(dir) => dir.to_path_buf(),
            None => {
                eprintln!("vendored ghostty-vt archive has no parent directory");
                std::process::exit(1);
            }
        }
    } else if target == "aarch64-apple-darwin" {
        // Legacy single-archive layout (pre-multi-platform). Kept so local
        // Apple Silicon development works without rerunning the vendor script.
        vendor_root.join("libghostty-vt.a")
    } else {
        panic!(
            "no vendored ghostty-vt archive for {target}: expected {} -- produce it with
             scripts/vendor-ghostty-vt.sh --triple <target> (or trigger
             .github/workflows/vendor-ghostty-vt.yml) and commit the result",
            archive.display()
        );
    };

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=ghostty-vt");

    if target.contains("darwin") {
        println!("cargo:rustc-link-lib=c++");
    } else if target.contains("linux") {
        println!("cargo:rustc-link-lib=stdc++");
    }
    // windows-msvc: the Zig-built COFF resolves UCRT/Win32 imports through the
    // default MSVC link set (libcmt/kernel32); add system libs here only after
    // the Windows CI job proves they are required.

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", archive.display());
    println!(
        "cargo:rerun-if-changed={}",
        vendor_root.join("libghostty-vt.a").display()
    );
}
