//! B1 on sections: PLACES and HIERARCHY of the `GeoNames` Nepal tree, bytes per column, and the
//! Elias–Fano bounds the children columns are compared with.
#![allow(clippy::print_stdout, reason = "a report printed for the results page")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "byte and bit counts far below 2^52; rows and offsets far below 2^31"
)]

use teigeodata_bench::{geonames_path, geonames_tree};
use teigeodata_core::bitpack::Packed;
use teigeodata_core::table::Table;
use teigeodata_format::hierarchy::{self, CHILD_END, CHILDREN, PARENT};
use teigeodata_format::places::{self, PlaceColumns};

/// Elias–Fano bits for `n` ascending values below `universe`: n × (2 + ⌈log2(u / n)⌉).
fn elias_fano_bits(n: usize, universe: usize) -> f64 {
    if n == 0 {
        return 0.0;
    }
    let low = (universe as f64 / n as f64).log2().ceil().max(0.0);
    n as f64 * (2.0 + low)
}

fn report(name: &str, bytes: &[u8], ids: &[(u16, &str)], n: usize) {
    let Ok(table) = Table::parse(bytes) else {
        println!("{name}: not a table");
        return;
    };
    println!(
        "\n{name}: {} bytes, {:.3} B per place",
        bytes.len(),
        bytes.len() as f64 / n as f64
    );
    println!("| column | coding | bytes | B per place |");
    println!("|---|---|---:|---:|");
    for &(id, label) in ids {
        if let Some(b) = table.column_bytes(id) {
            let coding = Packed::parse(b).map_or("?", |p| match p.coding() {
                teigeodata_core::bitpack::Coding::For => "FOR",
                teigeodata_core::bitpack::Coding::Delta => "delta",
            });
            println!(
                "| {label} | {coding} | {} | {:.3} |",
                b.len(),
                b.len() as f64 / n as f64
            );
        }
    }
}

fn main() {
    let path = geonames_path();
    let Some(tree) = geonames_tree(&path) else {
        println!("missing {}", path.display());
        return;
    };
    let n = tree.points.len();
    let with_parent = tree.parents.iter().filter(|p| p.is_some()).count();
    let depth = (0..n)
        .map(|mut row| {
            let mut d = 0;
            while let Some(p) = tree.parents.get(row).copied().flatten() {
                row = p as usize;
                d += 1;
            }
            d
        })
        .max()
        .unwrap_or(0);
    println!("{n} places, {with_parent} with a parent, deepest chain {depth}");

    let cols = PlaceColumns {
        points: &tree.points,
        kinds: &tree.kinds,
        admin_levels: &tree.levels,
        ..PlaceColumns::default()
    };
    let Ok(p) = places::encode(&cols) else {
        println!("places refused");
        return;
    };
    report(
        "PLACES",
        &p,
        &[
            (places::LAT, "lat"),
            (places::LON, "lon"),
            (places::KIND, "kind"),
            (places::ADMIN_LEVEL, "admin level"),
        ],
        n,
    );
    let Ok(h) = hierarchy::encode(&tree.parents) else {
        println!("hierarchy refused");
        return;
    };
    report(
        "HIERARCHY",
        &h,
        &[
            (PARENT, "parent"),
            (CHILD_END, "child end"),
            (CHILDREN, "children"),
        ],
        n,
    );

    // A/B: the parent as a signed offset from the row (zigzag), against the row itself.
    let zig = |d: i64| ((d << 1) ^ (d >> 63)) as u32;
    let offsets: Vec<u32> = tree
        .parents
        .iter()
        .enumerate()
        .map(|(row, p)| p.map_or(0, |p| zig(i64::from(p) - row as i64) + 1))
        .collect();
    let plain: Vec<u32> = tree
        .parents
        .iter()
        .map(|p| p.map_or(0, |p| p + 1))
        .collect();
    for (label, v) in [
        ("parent row + 1", &plain),
        ("zigzag(parent - row) + 1", &offsets),
    ] {
        let b = teigeodata_core::bitpack::encode_auto(v);
        println!("{label}: {:.3} B per place", b.len() as f64 / n as f64);
    }

    // The Elias–Fano bounds: the running ends as one ascending list; each child list as an
    // ascending list of rows below n.
    let mut groups = vec![0_usize; n];
    for p in tree.parents.iter().flatten() {
        if let Some(g) = groups.get_mut(*p as usize) {
            *g += 1;
        }
    }
    let ends = elias_fano_bits(n, with_parent + 1) / 8.0;
    let children: f64 = groups.iter().map(|&g| elias_fano_bits(g, n)).sum::<f64>() / 8.0;
    let parent = n as f64 * (n as f64 + 1.0).log2().ceil() / 8.0;
    println!(
        "\nbounds: child end EF {ends:.0} B, children EF {children:.0} B (per group), parent flat {parent:.0} B"
    );
}
