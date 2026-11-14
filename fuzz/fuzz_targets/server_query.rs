//! libFuzzer on [`teigeodata_server::fuzz::query_string`].
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| teigeodata_server::fuzz::query_string(data));
