//! libFuzzer on [`teigeodata_format::fuzz::mutate`] (D-072).
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| teigeodata_format::fuzz::mutate(data));
