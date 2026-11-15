//! Runs the query-parsing harness (`teigeodata_server::fuzz`) on seeded random inputs, as the
//! format's harnesses run (D-072); libFuzzer runs it nightly (`fuzz/`, `server_query`).
//! `TG_FUZZ_ITERS` sets the number of inputs.

use teigeodata_server::fuzz;

fn iterations(default: u64) -> u64 {
    std::env::var("TG_FUZZ_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(if cfg!(miri) { 1 } else { default })
}

/// Seeded inputs of 0 to 255 random bytes, biased to the bytes that matter: digits, `.`, `-`,
/// `,`, `=`, `&`, `%` and the segment mark `0xff`.
fn drive(seed: u64, n: u64, harness: fn(&[u8])) {
    const BIAS: &[u8] = b"0123456789.-,=&%+eE\xff";
    let mut s = seed;
    let mut next = || {
        s = s.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = s;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    };
    let mut buf = Vec::new();
    for _ in 0..n {
        let len = usize::try_from(next() % 256).unwrap_or(0);
        buf.clear();
        while buf.len() < len {
            let r = next();
            let b = if r % 3 == 0 {
                BIAS.get(usize::try_from(r >> 8).unwrap_or(0) % BIAS.len())
                    .copied()
                    .unwrap_or(b'0')
            } else {
                u8::try_from(r >> 56).unwrap_or(0)
            };
            buf.push(b);
        }
        harness(&buf);
    }
}

#[test]
fn query_strings() {
    drive(11, iterations(50_000), fuzz::query_string);
}

/// Every MCP tool and resource on fuzzed arguments (D-331).
#[cfg(feature = "mcp")]
#[test]
fn mcp_calls() {
    drive(12, iterations(5_000), fuzz::mcp);
}

/// Valid query strings for each route (`ROUTES` order), with `{}` where a value goes.
const TEMPLATES: [&str; 18] = [
    "id={}&names={}",
    "ids={},{}",
    "lat={}&lon={}&limit={}&in_country={}",
    "id={}&limit={}&radius_m={}",
    "q={}&limit={}&focus={},{}&scale_km={}&strict={}",
    "id={}",
    "id={}&limit={}",
    "lat={}&lon={}",
    "lat={}&lon={}&t={}",
    "lat={}&lon={}",
    "code={}&prefix={}&limit={}",
    "id={}&lod={}",
    "from={},{}&to={}",
    "lat={}&lon={}&radius_m={}",
    "x={}",
    "q={}&format={}&limit={}&countrycodes={}&viewbox={},{},{},{}&bounded={}&addressdetails={}",
    "lat={}&lon={}&zoom={}&format={}&addressdetails={}",
    "osm_ids={},{},N{}&format={}",
];

/// The values put in a template: at and past every bound, off the globe, not numbers.
const VALUES: [&str; 30] = [
    "0",
    "1",
    "7",
    "10",
    "50",
    "51",
    "-1",
    "27.7",
    "85.3",
    "90",
    "-90.0001",
    "180",
    "180.5",
    "NaN",
    "inf",
    "1e308",
    "1000000",
    "1000000.1",
    "true",
    "",
    "tgd:np/1xhb",
    "Pokhara",
    "%FF%FE",
    "4",
    "json",
    "jsonv2",
    "geojson",
    "xml",
    "np",
    "18",
];

#[test]
fn query_strings_from_templates() {
    let mut s: u64 = 12;
    let mut next = || {
        s = s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        usize::try_from(s >> 33).unwrap_or(0)
    };
    for _ in 0..iterations(50_000) {
        let route = next() % TEMPLATES.len();
        let mut q = String::new();
        for (i, part) in TEMPLATES
            .get(route)
            .copied()
            .unwrap_or("")
            .split("{}")
            .enumerate()
        {
            if i > 0 {
                q.push_str(VALUES.get(next() % VALUES.len()).copied().unwrap_or(""));
            }
            q.push_str(part);
        }
        let mut input = vec![u8::try_from(route << 1).unwrap_or(0)];
        input.extend_from_slice(q.as_bytes());
        fuzz::query_string(&input);
    }
}
