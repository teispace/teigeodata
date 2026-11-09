//! libFuzzer on [`teigeodata_sync::fuzz::table`] (D-072).
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| teigeodata_sync::fuzz::table(data));
