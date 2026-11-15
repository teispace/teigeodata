//! libFuzzer on [`teigeodata_ffi::fuzz::abi`] (D-331): every `Engine` function of the C ABI on
//! the bindings' fixture packs, with arguments from the fuzz bytes.
#![no_main]

const SEEDS: [&[u8]; 4] = [
    include_bytes!("../../bindings/fixtures/synth.tgd"),
    include_bytes!("../../bindings/fixtures/outline.tgd"),
    include_bytes!("../../bindings/fixtures/dem.tgd"),
    include_bytes!("../../bindings/fixtures/tz.tgd"),
];

libfuzzer_sys::fuzz_target!(|data: &[u8]| teigeodata_ffi::fuzz::abi(&SEEDS, data));
