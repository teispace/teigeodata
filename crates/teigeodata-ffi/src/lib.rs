//! The C ABI of teigeodata ([ADR-0016], [boundary rules]).
//!
//! Every exported function is generated from `teigeodata-api` into `generated.rs` by `cargo
//! xtask bindings`; the header is `bindings/c/teigeodata.h`. The shared runtime (owned byte
//! buffers, status codes, the per-thread last error, the panic guard) is `abi.rs`. Those two
//! modules are the only places `unsafe` is allowed.
//!
//! **Contract for every function:**
//! - It returns a status: `TG_OK` (0) or a `TG_ERROR_*` code. On failure, `tg_last_error`
//!   gives the message on the same thread.
//! - Outputs are written through the last pointer argument, only on success.
//! - Strings in are UTF-8 pointer and length pairs; a null pointer is allowed only with
//!   length 0.
//! - Strings and blobs out are `tg_bytes`, owned by the caller, freed with `tg_bytes_free`.
//! - A panic never unwinds into the caller: it becomes `TG_ERROR_INTERNAL` (in a build with
//!   `panic = "unwind"`) or aborts the process (with `panic = "abort"`).
//! - Callbacks run on the calling thread, before the function returns.
//!
//! [ADR-0016]: https://github.com/teispace/teigeodata/blob/main/docs/03-decisions/adr/ADR-0016-bindings.md
//! [boundary rules]: https://github.com/teispace/teigeodata/blob/main/docs/04-architecture/abi.md

pub mod abi;
pub mod generated;
