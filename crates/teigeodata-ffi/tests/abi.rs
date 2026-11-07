//! The C ABI called as C would call it: every function, every documented failure, and the
//! ownership rules of `tg_bytes`.
#![allow(
    unsafe_code,
    reason = "these tests call the C ABI through raw pointers"
)]

use core::ffi::c_void;
use core::ptr;

use teigeodata_ffi::abi::{TgBytes, tg_bytes_free, tg_last_error};
use teigeodata_ffi::generated::{
    TG_ERROR_CANCELLED, TG_ERROR_INVALID_ARGUMENT, TG_OK, TG_SAMPLE_ROWS_MAX, tg_fold, tg_noop,
    tg_progress, tg_sample_rows, tg_version,
};

/// Takes a buffer's bytes and frees it.
fn take(b: TgBytes) -> Vec<u8> {
    let v = if b.ptr.is_null() {
        Vec::new()
    } else {
        // SAFETY: a buffer the library just returned.
        unsafe { core::slice::from_raw_parts(b.ptr, b.len) }.to_vec()
    };
    // SAFETY: freed once.
    unsafe { tg_bytes_free(b) };
    v
}

fn last_error() -> (i32, String) {
    let mut m = TgBytes::EMPTY;
    // SAFETY: a valid out pointer.
    let code = unsafe { tg_last_error(&raw mut m) };
    (code, String::from_utf8_lossy(&take(m)).into_owned())
}

fn fold(s: &[u8]) -> (i32, Vec<u8>) {
    let mut out = TgBytes::EMPTY;
    // SAFETY: `s` is readable for its length; `out` is writable.
    let status = unsafe { tg_fold(s.as_ptr(), s.len(), &raw mut out) };
    (status, take(out))
}

#[test]
fn strings_cross_both_ways() {
    let mut out = TgBytes::EMPTY;
    // SAFETY: a writable out pointer.
    assert_eq!(unsafe { tg_version(&raw mut out) }, TG_OK);
    assert_eq!(take(out), env!("CARGO_PKG_VERSION").as_bytes());
    // SAFETY: no arguments.
    assert_eq!(unsafe { tg_noop() }, TG_OK);
    assert_eq!(
        fold("São  Paulo".as_bytes()),
        (TG_OK, b"sao paulo".to_vec())
    );
    assert_eq!(fold(b""), (TG_OK, Vec::new()));
    // A null pointer with length 0 is the empty string.
    let mut out = TgBytes::EMPTY;
    // SAFETY: null with length 0 is allowed.
    assert_eq!(unsafe { tg_fold(ptr::null(), 0, &raw mut out) }, TG_OK);
    assert_eq!(take(out), b"");
    assert_eq!(last_error().0, 0);
}

#[test]
fn bad_arguments_fail_with_a_message() {
    let mut bytes = TgBytes::EMPTY;
    let mut n = 0u32;
    assert_eq!(fold(b"\xff\xfe").0, TG_ERROR_INVALID_ARGUMENT);
    let (code, msg) = last_error();
    assert_eq!(code, TG_ERROR_INVALID_ARGUMENT);
    assert!(msg.starts_with("text is not UTF-8"), "{msg}");
    // Reading clears it.
    assert_eq!(last_error(), (0, String::new()));
    // SAFETY: null with a length is rejected before any read.
    assert_eq!(
        unsafe { tg_fold(ptr::null(), 3, &raw mut bytes) },
        TG_ERROR_INVALID_ARGUMENT
    );
    assert!(last_error().1.contains("null"));
    // SAFETY: a null out pointer is rejected before any write.
    assert_eq!(
        unsafe { tg_fold(b"a".as_ptr(), 1, ptr::null_mut()) },
        TG_ERROR_INVALID_ARGUMENT
    );
    // SAFETY: as above.
    assert_eq!(
        unsafe { tg_sample_rows(TG_SAMPLE_ROWS_MAX + 1, &raw mut bytes) },
        TG_ERROR_INVALID_ARGUMENT
    );
    assert!(last_error().1.contains("at most"));
    // SAFETY: a missing callback is rejected.
    assert_eq!(
        unsafe { tg_progress(1, None, ptr::null_mut(), &raw mut n) },
        TG_ERROR_INVALID_ARGUMENT
    );
    let _ = last_error();
    // A zeroed buffer frees as a no-op.
    // SAFETY: the documented empty value.
    unsafe { tg_bytes_free(TgBytes::EMPTY) };
}

#[test]
fn blobs_are_aligned_and_decode() {
    let mut out = TgBytes::EMPTY;
    // SAFETY: a writable out pointer.
    assert_eq!(unsafe { tg_sample_rows(1000, &raw mut out) }, TG_OK);
    assert_eq!(out.ptr as usize % 8, 0, "the blob starts 8-aligned");
    let bytes = take(out);
    assert_eq!(bytes, teigeodata_api::sample_rows(1000).unwrap().into_vec());
}

unsafe extern "C" fn count(ctx: *mut c_void, step: u32) -> bool {
    // SAFETY: the test passes a `Vec<u32>` as the context.
    let seen = unsafe { &mut *ctx.cast::<Vec<u32>>() };
    seen.push(step);
    step < 3
}

#[test]
fn callbacks_run_and_cancel() {
    let mut seen: Vec<u32> = Vec::new();
    let mut out = 0u32;
    let ctx = (&raw mut seen).cast::<c_void>();
    // SAFETY: `count` matches the callback type and `ctx` is the `Vec` it expects.
    assert_eq!(
        unsafe { tg_progress(2, Some(count), ctx, &raw mut out) },
        TG_OK
    );
    assert_eq!((out, seen.as_slice()), (2, [1, 2].as_slice()));
    seen.clear();
    // SAFETY: as above.
    assert_eq!(
        unsafe { tg_progress(9, Some(count), ctx, &raw mut out) },
        TG_ERROR_CANCELLED
    );
    assert_eq!(seen, [1, 2, 3]);
    assert_eq!(out, 2, "outputs are written only on success");
    assert_eq!(
        last_error(),
        (TG_ERROR_CANCELLED, "cancelled at step 3 of 9".into())
    );
}
