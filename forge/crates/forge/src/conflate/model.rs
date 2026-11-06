//! The Fellegi–Sunter model (forge note §4, "as Splink implements it"): a comparison vector of
//! levels per field, u from seeded random pairs, m and λ by EM over the counts of each
//! comparison pattern (a few hundred patterns, not millions of pairs), weights in bits.

/// Comparison fields and their number of levels. Level 0 is the strongest agreement.
pub(crate) const FIELDS: [(&str, usize); 4] = [
    ("name", 4),     // fold equal, join key equal, phonetic key equal, none
    ("distance", 5), // < 250 m, < 1 km, < 3 km, < 10 km, farther
    ("kind", 3),     // same, one side `other` or unknown, different
    ("id", 2),       // a stated same-as link, none
];
pub(crate) const NAME: usize = 0;
pub(crate) const ID: usize = 3;

/// A comparison vector: one level per field.
pub(crate) type Gamma = [u8; 4];

/// The smallest probability any level may take, so no single field can veto or force a match.
const FLOOR: f64 = 1e-6;

/// m and u per field and level, and the prior λ.
#[derive(Clone, Debug)]
pub(crate) struct Model {
    pub(crate) m: [[f64; 5]; 4],
    pub(crate) u: [[f64; 5]; 4],
    pub(crate) lambda: f64,
    /// Fields left out of the weight (the evaluation model leaves out `id`).
    pub(crate) skip: [bool; 4],
}

/// Counts of each comparison pattern.
pub(crate) type Patterns = std::collections::BTreeMap<Gamma, u64>;

pub(crate) fn levels(field: usize) -> usize {
    FIELDS.get(field).map_or(0, |f| f.1)
}

/// Level frequencies per field over a set of patterns, floored and normalised.
pub(crate) fn frequencies(p: &Patterns) -> [[f64; 5]; 4] {
    let mut out = [[0.0; 5]; 4];
    for (g, &n) in p {
        for (f, &l) in g.iter().enumerate() {
            if let Some(v) = out.get_mut(f).and_then(|r| r.get_mut(usize::from(l))) {
                #[allow(clippy::cast_precision_loss, reason = "counts")]
                let n = n as f64;
                *v += n;
            }
        }
    }
    for (f, row) in out.iter_mut().enumerate() {
        normalise(row, levels(f));
    }
    out
}

fn normalise(row: &mut [f64; 5], n: usize) {
    let total: f64 = row.iter().take(n).sum();
    for v in row.iter_mut().take(n) {
        *v = if total > 0.0 {
            (*v / total).max(FLOOR)
        } else {
            1.0 / 5.0
        };
    }
}

impl Model {
    /// The match weight in bits: log2(λ/(1−λ)) + Σ log2(m/u).
    pub(crate) fn weight(&self, g: Gamma) -> f64 {
        self.weight_without(g, &[])
    }

    /// The weight with some fields left out: an EM session conditions on the fields its
    /// blocking rule selected on, so they carry no evidence inside it.
    fn weight_without(&self, g: Gamma, conditioned: &[usize]) -> f64 {
        let prior = (self.lambda / (1.0 - self.lambda)).log2();
        prior
            + (0..4)
                .filter(|f| !conditioned.contains(f))
                .map(|field| self.bayes_bits(field, g))
                .sum::<f64>()
    }

    fn bayes_bits(&self, field: usize, g: Gamma) -> f64 {
        if self.skip.get(field).copied().unwrap_or(true) {
            return 0.0;
        }
        let level = g.get(field).map_or(0, |l| usize::from(*l));
        let at = |t: &[[f64; 5]; 4]| {
            t.get(field)
                .and_then(|r| r.get(level))
                .copied()
                .unwrap_or(FLOOR)
        };
        (at(&self.m).max(FLOOR) / at(&self.u).max(FLOOR)).log2()
    }

    /// One EM session over blocked patterns, with u fixed. The `blocked` fields are left out of
    /// the expectation and keep their m: the session cannot estimate what it selected on. λ
    /// becomes P(match) among this session's pairs.
    pub(crate) fn em(&mut self, patterns: &Patterns, blocked: &[usize], rounds: usize) {
        for _ in 0..rounds {
            let mut m = [[0.0; 5]; 4];
            let (mut matched, mut total) = (0.0, 0.0);
            for (g, &n) in patterns {
                #[allow(clippy::cast_precision_loss, reason = "counts")]
                let n = n as f64;
                let p = probability(self.weight_without(*g, blocked)) * n;
                matched += p;
                total += n;
                for (f, &l) in g.iter().enumerate() {
                    if let Some(v) = m.get_mut(f).and_then(|r| r.get_mut(usize::from(l))) {
                        *v += p;
                    }
                }
            }
            for (f, row) in m.iter_mut().enumerate() {
                if blocked.contains(&f) {
                    continue;
                }
                normalise(row, levels(f));
                if let Some(dst) = self.m.get_mut(f) {
                    *dst = *row;
                }
            }
            if total > 0.0 {
                self.lambda = (matched / total).clamp(1e-9, 1.0 - 1e-9);
            }
        }
    }
}

/// P(match) from a weight in bits.
pub(crate) fn probability(bits: f64) -> f64 {
    let k = bits.exp2();
    if k.is_infinite() { 1.0 } else { k / (1.0 + k) }
}

/// The starting m: agreement levels likely among matches.
pub(crate) fn initial_m() -> [[f64; 5]; 4] {
    [
        [0.6, 0.2, 0.15, 0.05, 0.0],
        [0.35, 0.35, 0.15, 0.1, 0.05],
        [0.7, 0.2, 0.1, 0.0, 0.0],
        [0.5, 0.5, 0.0, 0.0, 0.0],
    ]
}

/// The level of a distance in metres.
pub(crate) fn distance_level(m: f64) -> u8 {
    match m {
        d if d < 250.0 => 0,
        d if d < 1000.0 => 1,
        d if d < 3000.0 => 2,
        d if d < 10_000.0 => 3,
        _ => 4,
    }
}

/// A kind's family: populated places and localities, admin units, or anything else.
pub(crate) fn family(kind: &str) -> u8 {
    match kind {
        "populated_place" | "locality" => 0,
        "admin" => 1,
        _ => 2,
    }
}

/// The level of a pair of kinds.
pub(crate) fn kind_level(a: &str, b: &str) -> u8 {
    if a == b || family(a) == family(b) {
        0
    } else if family(a) == 2 || family(b) == 2 {
        1
    } else {
        2
    }
}

/// A seeded `SplitMix64`: deterministic sampling with no dependency.
pub(crate) struct Rng(pub(crate) u64);

impl Rng {
    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `0..n`.
    pub(crate) fn below(&mut self, n: usize) -> usize {
        let n = u64::try_from(n).unwrap_or(u64::MAX).max(1);
        usize::try_from(self.next() % n).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::{Model, Patterns, distance_level, frequencies, initial_m, kind_level, probability};

    #[test]
    fn em_separates_matches_from_noise() {
        // 900 non-matches disagree on everything; 100 matches agree on name and distance.
        let mut blocked = Patterns::new();
        blocked.insert([0, 0, 0, 1], 100);
        blocked.insert([3, 4, 0, 1], 900);
        let mut random = Patterns::new();
        random.insert([3, 4, 0, 1], 100_000);
        random.insert([0, 4, 0, 1], 10);
        let mut model = Model {
            m: initial_m(),
            u: frequencies(&random),
            lambda: 0.5,
            skip: [false; 4],
        };
        model.em(&blocked, &[], 50);
        // Conditioning on the name field: the session still separates on distance.
        let mut conditioned = model.clone();
        conditioned.em(&blocked, &[0], 50);
        assert!(
            (conditioned.lambda - 0.1).abs() < 0.01,
            "λ {}",
            conditioned.lambda
        );
        assert!((model.lambda - 0.1).abs() < 0.01, "λ {}", model.lambda);
        assert!(probability(model.weight([0, 0, 0, 1])) > 0.99);
        assert!(probability(model.weight([3, 4, 0, 1])) < 0.01);
    }

    #[test]
    fn levels() {
        assert_eq!(distance_level(10.0), 0);
        assert_eq!(distance_level(2500.0), 2);
        assert_eq!(distance_level(50_000.0), 4);
        assert_eq!(kind_level("populated_place", "locality"), 0);
        assert_eq!(kind_level("populated_place", "other"), 1);
        assert_eq!(kind_level("admin", "populated_place"), 2);
        assert!((probability(0.0) - 0.5).abs() < 1e-12);
    }
}
