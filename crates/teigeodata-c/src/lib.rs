//! `libteigeodata`, the C library as it ships (D-286).
//!
//! The functions are those of `teigeodata-ffi`: rustc exports every `#[no_mangle]` function of
//! a cdylib's dependencies, and a staticlib carries them, so this crate only links that crate
//! under the released file name. `build.rs` sets the install name (Apple) and the soname
//! (ELF). `cargo xtask package-c` builds it and checks that it exports exactly the header's
//! functions.

pub use teigeodata_ffi as ffi;
