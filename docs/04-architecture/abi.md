---
title: "The C ABI: rules for every exported function"
status: draft
updated: 2026-11-07
---

# The C ABI: rules for every exported function

`crates/teigeodata-ffi` exports the C ABI. Every function in it is generated from
`teigeodata-api` ([bindings](bindings.md)); the header is `bindings/c/teigeodata.h`. These rules
hold for every function, so a caller learns them once.

## 1. Calls

- **Every function returns `int32_t`:** `TG_OK` (0) or a `TG_ERROR_*` code. The codes are the
  API's `ErrorCode` values and are stable.
- **On failure, `tg_last_error(&message)`** gives the code and message of the last failure on
  the same thread, and clears it. `message` may be `NULL`.
- **Outputs are written through the last pointer argument, and only on success.** A null output
  pointer is `TG_ERROR_INVALID_ARGUMENT`, checked before any work.
- **Strings in** are UTF-8 pointer and length pairs. `NULL` is allowed only with length 0.
  Invalid UTF-8 is `TG_ERROR_INVALID_ARGUMENT`, with the byte position in the message.
- **Strings and blobs out** are `tg_bytes { ptr, len, cap }`, owned by the caller and freed once
  with `tg_bytes_free`. A zeroed `tg_bytes` is empty and safe to free. Strings have no NUL
  terminator. Blobs start 8-byte aligned on every supported platform (the system allocator gives
  16), and the C test checks it.
- **Callbacks** are a function pointer and a `void *` context, called on the caller's thread
  before the function returns. Returning `false` cancels with `TG_ERROR_CANCELLED`. A null
  callback is `TG_ERROR_INVALID_ARGUMENT`.
- **Panics never cross the boundary.** Each body runs under `catch_unwind`; a panic becomes
  `TG_ERROR_INTERNAL` with a message, and the thread can keep calling. This needs
  `panic = "unwind"`, which the shipped `dist` profile uses: it costs 176 bytes (D-065).

## 2. Unsafe code

The workspace forbids `unsafe`. The ABI crate and the `bindings/` workspace copy the lint table
with one change, `unsafe_code = "deny"`. `cargo xtask check-workspace` enforces this. Only these
modules allow `unsafe`, each with a reason:

| Module | Why |
|---|---|
| `teigeodata-ffi/src/abi.rs` | the shared runtime: `tg_bytes`, `tg_bytes_free`, `tg_last_error`, argument checks |
| `teigeodata-ffi/src/generated.rs` | `#[unsafe(no_mangle)]` exports and pointer writes, each with a `SAFETY` comment |
| `bindings/python/src/lib.rs`, `bindings/node/src/lib.rs` | PyO3's and napi-rs's macro glue; Node's one `throw_into` to rethrow a callback's exception |
| `bindings/jvm/src/lib.rs` | JNI exports, and one unchecked call per callback, with the method resolved and its signature fixed |

## 3. Versioning

The ABI is versioned with the library. Until 1.0 it may change at any release; from 1.0, a
removed or changed function needs a major version. `tg_version` returns the version.

## Changelog

| Date | Change | Why |
|---|---|---|
| 2026-11-07 | First version, from spike S4 | [S4](../02-research/benches/s4-bindings.md) |
