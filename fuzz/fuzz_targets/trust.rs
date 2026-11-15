//! libFuzzer on [`teigeodata_sync::fuzz::trust`] (D-331): the TUF update chain and the catalog
//! on a repository with one role attacked.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| teigeodata_sync::fuzz::trust(data));
