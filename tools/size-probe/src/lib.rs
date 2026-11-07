//! Z2 size probe: each feature exports one C entry point that reaches a part of the runtime
//! crates, so the linker keeps exactly that code. Inputs are untrusted bytes; results are
//! reduced to an integer so nothing is optimised away. It links `std` like a real library;
//! a baseline build (only `tg_base`) is subtracted, so `std` itself is not counted.
#![allow(
    unsafe_code,
    clippy::missing_safety_doc,
    reason = "C entry points over caller buffers (a pointer and its length); measurement only, never shipped"
)]

/// In every build, the baseline too: what any host library links already (formatting, a
/// panic path, the allocator), so the subtraction leaves only what our crates add.
#[unsafe(no_mangle)]
pub extern "C" fn tg_base(n: usize) -> usize {
    let s = format!("{n}");
    let v = vec![1_usize; n % 7 + 1];
    s.len() + v[n % 9]
}

#[cfg(feature = "open")]
unsafe fn bytes<'a>(p: *const u8, n: usize) -> &'a [u8] {
    if p.is_null() {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(p, n) }
    }
}

#[cfg(feature = "open")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tg_open(p: *const u8, n: usize) -> usize {
    use teigeodata_format::{Kind, Pack};
    match Pack::open(unsafe { bytes(p, n) }) {
        Ok(pack) => pack.section(Kind::PLACES).map_or(1, |s| s.bytes().len()),
        Err(_) => 0,
    }
}

#[cfg(feature = "verify")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tg_verify(p: *const u8, n: usize) -> usize {
    use teigeodata_format::Pack;
    Pack::open(unsafe { bytes(p, n) })
        .is_ok_and(|pack| pack.verify_sections().is_ok())
        .into()
}

#[cfg(feature = "signature")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tg_signature(p: *const u8, n: usize, key: *const [u8; 32]) -> usize {
    use teigeodata_format::Pack;
    let Some(key) = (unsafe { key.as_ref() }) else {
        return 0;
    };
    let Ok(k) = ed25519_dalek::VerifyingKey::from_bytes(key) else {
        return 0;
    };
    Pack::open(unsafe { bytes(p, n) })
        .ok()
        .and_then(|pack| pack.verify_signature(&[k]).ok())
        .map_or(0, |i| i + 1)
}

#[cfg(feature = "sections")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tg_sections(p: *const u8, n: usize, row: usize) -> usize {
    use teigeodata_format::components::Components;
    use teigeodata_format::hierarchy::Hierarchy;
    use teigeodata_format::idmap::IdMap;
    use teigeodata_format::names::{Names, TEXT_BUFFER};
    use teigeodata_format::places::Places;
    use teigeodata_format::{Kind, Pack};
    let Ok(pack) = Pack::open(unsafe { bytes(p, n) }) else {
        return 0;
    };
    let mut sum = 0;
    if let Some(Ok(v)) = pack.section(Kind::PLACES).map(|s| Places::open(&s)) {
        sum += v
            .point(row)
            .map_or(0, |pt| pt.lat().unsigned_abs() as usize);
    }
    if let Some(Ok(v)) = pack.section(Kind::HIERARCHY).map(|s| Hierarchy::open(&s)) {
        sum += v.ancestors(row).count();
    }
    if let Some(Ok(v)) = pack.section(Kind::IDMAP).map(|s| IdMap::open(&s)) {
        sum += v.id(row).and_then(|id| v.row(id)).unwrap_or(0);
    }
    if let Some(Ok(v)) = pack.section(Kind::NAMES).map(|s| Names::open(&s)) {
        let mut buf = [0; TEXT_BUFFER];
        sum += v
            .canonical(row)
            .and_then(|c| c.text(&mut buf).ok())
            .map_or(0, str::len);
    }
    if let Some(Ok(v)) = pack.section(Kind::COMPONENTS).map(|s| Components::open(&s)) {
        sum += v
            .component(1)
            .and_then(|c| c.field(1))
            .and_then(|f| f.get(row))
            .unwrap_or(0) as usize;
    }
    sum
}

#[cfg(feature = "blobs")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tg_blob(p: *const u8, n: usize, i: usize) -> usize {
    use teigeodata_format::blobs::Blobs;
    use teigeodata_format::{Kind, Pack};
    let Ok(pack) = Pack::open(unsafe { bytes(p, n) }) else {
        return 0;
    };
    let Some(Ok(b)) = pack.section(Kind::BLOBS).map(|s| Blobs::open(&s)) else {
        return 0;
    };
    b.decoder()
        .ok()
        .and_then(|mut d| d.get(i).ok().map(<[u8]>::len))
        .unwrap_or(0)
}

#[cfg(feature = "write")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tg_write(p: *const u8, n: usize) -> usize {
    use teigeodata_format::{Encoding, Kind, Writer};
    let body = if p.is_null() {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(p, n) }
    };
    let Ok(mut w) = Writer::new("np", "places", "2026.11.0") else {
        return 0;
    };
    if w.section_bytes(Kind::PLACES, Encoding::RAW, 1, body)
        .is_err()
    {
        return 0;
    }
    w.layout(false)
        .and_then(|l| l.assemble(&[body]))
        .map_or(0, |v| v.len())
}
