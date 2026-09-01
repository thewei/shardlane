fn main() {
    let manifest_dir = match std::env::var_os("CARGO_MANIFEST_DIR") {
        Some(path) => std::path::PathBuf::from(path),
        None => {
            eprintln!("CARGO_MANIFEST_DIR is required to locate vendored ghostty-vt");
            std::process::exit(1);
        }
    };
    let lib_dir = manifest_dir.join("vendor/ghostty-vt/lib");

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=ghostty-vt");

    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("darwin") {
        println!("cargo:rustc-link-lib=c++");
    } else if target.contains("linux") {
        println!("cargo:rustc-link-lib=stdc++");
    }

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=vendor/ghostty-vt/lib/libghostty-vt.a");
}
