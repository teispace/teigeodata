//! libFuzzer on [`teigeodata_engine::fuzz::engine`] (D-331): the first byte picks a seed pack,
//! the rest edits its sections and drives the queries. The seeds are the files the engine's
//! `tests/fuzz.rs` uses, embedded.
#![no_main]

const SEEDS: [&[u8]; 5] = [
    include_bytes!("../../bindings/fixtures/synth.tgd"),
    include_bytes!("../../bindings/fixtures/outline.tgd"),
    include_bytes!("../../bindings/fixtures/dem.tgd"),
    include_bytes!("../../bindings/fixtures/tz.tgd"),
    include_bytes!("../seeds/extra.tgd"),
];

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Some((&pick, rest)) = data.split_first() {
        let seed = SEEDS[usize::from(pick) % SEEDS.len()];
        teigeodata_engine::fuzz::engine(seed, rest);
    }
});
