//! libFuzzer on [`teigeodata_format::fuzz::sections`].
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| teigeodata_format::fuzz::sections(data));
