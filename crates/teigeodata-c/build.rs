//! The shared library's own name, recorded in every program linked against it (D-286):
//! `@rpath/libteigeodata.dylib` on Apple platforms, so an app or framework finds it beside
//! itself; `libteigeodata.so.<abi>` on ELF, where `<abi>` changes only with a breaking release
//! (`0.<minor>` before 1.0, `<major>` after; `cargo xtask abi` names those releases). Android
//! loads libraries by their file name, so its soname is unversioned.

fn main() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let mut parts = version.split('.');
    let (major, minor) = (parts.next().unwrap_or("0"), parts.next().unwrap_or("0"));
    let abi = if major == "0" {
        format!("0.{minor}")
    } else {
        major.to_owned()
    };
    match os.as_str() {
        "macos" | "ios" | "tvos" | "watchos" | "visionos" => {
            println!("cargo::rustc-cdylib-link-arg=-Wl,-install_name,@rpath/libteigeodata.dylib");
        }
        "android" => println!("cargo::rustc-cdylib-link-arg=-Wl,-soname,libteigeodata.so"),
        "linux" | "freebsd" | "netbsd" | "openbsd" | "dragonfly" | "illumos" | "solaris" => {
            println!("cargo::rustc-cdylib-link-arg=-Wl,-soname,libteigeodata.so.{abi}");
        }
        _ => {}
    }
    println!("cargo::rerun-if-changed=build.rs");
}
