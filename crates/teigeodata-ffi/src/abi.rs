//! The runtime every generated function shares: owned buffers, status codes, the last error
//! and the panic guard.
#![allow(
    unsafe_code,
    reason = "the C ABI: raw pointers from the caller and buffers handed across it (audited)"
)]

use core::ffi::c_void;
use core::mem::ManuallyDrop;
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};

use teigeodata_api::{Error, ErrorCode};

/// An owned byte buffer handed to the caller: a string (UTF-8, no NUL) or a blob. Free it with
/// [`tg_bytes_free`]. A zeroed value is empty and safe to free.
#[repr(C)]
#[derive(Debug)]
pub struct TgBytes {
    /// The first byte; null when empty.
    pub ptr: *mut u8,
    /// Length in bytes.
    pub len: usize,
    /// Capacity of the allocation, needed to free it.
    pub cap: usize,
}

impl TgBytes {
    /// Hands a buffer to the caller.
    #[must_use]
    pub fn from_vec(v: Vec<u8>) -> Self {
        if v.capacity() == 0 {
            return Self::EMPTY;
        }
        let mut v = ManuallyDrop::new(v);
        Self {
            ptr: v.as_mut_ptr(),
            len: v.len(),
            cap: v.capacity(),
        }
    }

    /// The empty buffer.
    pub const EMPTY: Self = Self {
        ptr: core::ptr::null_mut(),
        len: 0,
        cap: 0,
    };
}

/// Frees a buffer returned by any function. Freeing an empty (zeroed) buffer does nothing.
///
/// # Safety
///
/// `bytes` must come from this library and must not be freed twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tg_bytes_free(bytes: TgBytes) {
    if !bytes.ptr.is_null() {
        // SAFETY: the caller passes a buffer made by `TgBytes::from_vec`, unchanged.
        drop(unsafe { Vec::from_raw_parts(bytes.ptr, bytes.len, bytes.cap) });
    }
}

thread_local! {
    static LAST: RefCell<Option<Error>> = const { RefCell::new(None) };
}

/// The code of the last failure on this thread, or 0, and its message in `message` (which the
/// caller frees). Reading clears it. `message` may be null to read only the code.
///
/// # Safety
///
/// `message` is null or points to writable memory for one `TgBytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tg_last_error(message: *mut TgBytes) -> i32 {
    let Some(e) = LAST.with(RefCell::take) else {
        return 0;
    };
    if !message.is_null() {
        // SAFETY: checked non-null; the caller guarantees it is writable.
        unsafe { message.write(TgBytes::from_vec(e.message.into_bytes())) };
    }
    e.code as i32
}

fn invalid(message: String) -> Error {
    Error {
        code: ErrorCode::InvalidArgument,
        message,
    }
}

/// Runs a call body: maps its error to a status and the last error, and stops any panic at
/// the boundary.
pub(crate) fn call(body: impl FnOnce() -> Result<(), Error>) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(body)).unwrap_or_else(|_| {
        Err(Error {
            code: ErrorCode::Internal,
            message: "a panic inside teigeodata was stopped at the C boundary".into(),
        })
    });
    match result {
        Ok(()) => 0,
        Err(e) => {
            let code = e.code as i32;
            LAST.with(|last| *last.borrow_mut() = Some(e));
            code
        }
    }
}

/// A UTF-8 string argument.
///
/// # Safety
///
/// `ptr` is null with `len` 0, or valid for reads of `len` bytes for the whole call.
pub(crate) unsafe fn str_arg<'a>(ptr: *const u8, len: usize, name: &str) -> Result<&'a str, Error> {
    if len == 0 {
        return Ok("");
    }
    if ptr.is_null() {
        return Err(invalid(format!("{name} is null with length {len}")));
    }
    // SAFETY: non-null, and the caller guarantees `len` readable bytes.
    let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
    core::str::from_utf8(bytes).map_err(|e| invalid(format!("{name} is not UTF-8: {e}")))
}

/// Checks an output pointer before any work is done.
/// Bytes in: `len` readable bytes at `ptr` (null only with 0).
///
/// # Safety
///
/// `ptr` points to `len` readable bytes that stay valid and unchanged during the call.
pub(crate) unsafe fn bytes_arg<'a>(
    ptr: *const u8,
    len: usize,
    name: &str,
) -> Result<&'a [u8], Error> {
    if len == 0 {
        return Ok(&[]);
    }
    if ptr.is_null() {
        return Err(invalid(format!("{name} is null with length {len}")));
    }
    // SAFETY: the caller's contract above.
    Ok(unsafe { core::slice::from_raw_parts(ptr, len) })
}

/// A handle argument, borrowed for the call.
///
/// # Safety
///
/// `ptr` is null or a live handle of type `T` made by this library and not yet freed.
pub(crate) unsafe fn handle<'a, T>(ptr: *const T, name: &str) -> Result<&'a T, Error> {
    // SAFETY: the caller's contract above.
    unsafe { ptr.as_ref() }.ok_or_else(|| invalid(format!("{name} is null")))
}

/// Frees a handle made with `Box::into_raw`; null does nothing. A panic in its drop stops
/// here.
///
/// # Safety
///
/// `ptr` is null or a handle of type `T` made by this library, freed once.
pub(crate) unsafe fn free<T>(ptr: *mut T) {
    if !ptr.is_null() {
        // SAFETY: the caller's contract above.
        let owned = unsafe { Box::from_raw(ptr) };
        let _ = catch_unwind(AssertUnwindSafe(move || drop(owned)));
    }
}

pub(crate) fn out<T>(ptr: *mut T) -> Result<*mut T, Error> {
    if ptr.is_null() {
        Err(invalid("the output pointer is null".into()))
    } else {
        Ok(ptr)
    }
}

/// Checks a callback is present.
pub(crate) fn callback<F>(f: Option<F>, name: &str) -> Result<F, Error> {
    f.ok_or_else(|| invalid(format!("{name} is null")))
}

/// The context pointer handed back to a callback, unchanged.
pub type Ctx = *mut c_void;

#[cfg(test)]
mod tests {
    use super::{TgBytes, call, tg_bytes_free, tg_last_error};

    /// A panic inside a call becomes `TG_ERROR_INTERNAL` with a message, and the thread can
    /// keep calling. (Holds in builds with `panic = "unwind"`, as the shipped library is.)
    #[test]
    fn a_panic_stops_at_the_boundary() {
        let status = call(|| -> Result<(), teigeodata_api::Error> { panic!("deliberate") });
        assert_eq!(status, teigeodata_api::ErrorCode::Internal as i32);
        let mut m = TgBytes::EMPTY;
        // SAFETY: a valid out pointer; the message is freed once.
        let code = unsafe { tg_last_error(&raw mut m) };
        assert_eq!(code, 3);
        // SAFETY: as above.
        let text = unsafe { core::slice::from_raw_parts(m.ptr, m.len) };
        assert!(core::str::from_utf8(text).unwrap().contains("panic"));
        // SAFETY: freed once.
        unsafe { tg_bytes_free(m) };
        assert_eq!(call(|| Ok(())), 0);
    }
}
