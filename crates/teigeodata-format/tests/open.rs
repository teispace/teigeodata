//! Targeted tests of the container: each refusal of `Pack::open`, signatures, determinism and
//! the 16 KiB head rule. The fuzz properties (`tests/fuzz.rs`) cover the rest.
#![allow(
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests: a panic reports a failure, and indexes are into packs built here"
)]

use ed25519_dalek::SigningKey;
use teigeodata_format::{
    Encoding, Error, FORMAT_MAJOR, FORMAT_MINOR, Kind, Layout, Pack, Tag, Writer, key_id, signer,
};

const PLACES: &[u8] = b"place rows";
const NAMES: &[u8] = &[0x5a; 200];

fn writer() -> Writer {
    let mut w = Writer::new("np", "places", "2026.11.0").unwrap_or_else(|e| panic!("{e}"));
    w.build_id(0xfeed).created_unix(1_762_473_600);
    for (kind, body) in [(Kind::PLACES, PLACES), (Kind::NAMES, NAMES)] {
        w.section_bytes(kind, Encoding::RAW, 3, body)
            .unwrap_or_else(|e| panic!("{e}"));
    }
    w.manifest(Tag::LICENCE, b"odbl")
        .unwrap_or_else(|e| panic!("{e}"));
    w
}

fn build(signed: bool, key: &SigningKey) -> Vec<u8> {
    let mut layout: Layout = writer().layout(signed).unwrap_or_else(|e| panic!("{e}"));
    if signed {
        let block = signer(key)(layout.digest());
        layout
            .set_signature(&block)
            .unwrap_or_else(|e| panic!("{e}"));
    }
    layout
        .assemble(&[PLACES, NAMES])
        .unwrap_or_else(|e| panic!("{e}"))
}

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn unsigned() -> Vec<u8> {
    build(false, &key(1))
}

#[test]
fn reads_what_was_written() {
    let bytes = unsigned();
    let p = Pack::open_verified(&bytes).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(p.format_version(), (FORMAT_MAJOR, FORMAT_MINOR));
    assert_eq!((p.build_id(), p.created_unix()), (0xfeed, 1_762_473_600));
    assert_eq!(p.manifest().text(Tag::LICENCE), Some("odbl"));
    let kinds: Vec<Kind> = p.sections().map(|s| s.kind()).collect();
    assert_eq!(kinds, [Kind::PLACES, Kind::NAMES]);
    assert_eq!(p.section(Kind::NAMES).map(|s| s.bytes()), Some(NAMES));
    assert_eq!(p.section(Kind::NAMES).map(|s| s.item_count()), Some(3));
    assert!(p.section(Kind::IDMAP).is_none());
    assert!(p.section(Kind(1000)).is_none());
    // Sections start on 64-byte boundaries.
    for s in p.sections() {
        let off = s.bytes().as_ptr() as usize - bytes.as_ptr() as usize;
        assert_eq!(off % 64, 0, "{}", s.kind());
    }
    assert_eq!(p.signature(), None);
}

#[test]
fn builds_are_reproducible() {
    assert_eq!(unsigned(), unsigned());
    assert_eq!(build(true, &key(1)), build(true, &key(1)));
    // The identity, and nothing else, sets the UUID.
    let other = Writer::new("np", "places", "2026.11.1")
        .and_then(|w| w.layout(false))
        .and_then(|l| l.assemble(&[]))
        .and_then(|b| Pack::open(&b).map(|p| p.uuid()));
    let this = Pack::open(&unsigned()).map(|p| p.uuid());
    assert!(other.is_ok() && this.is_ok() && other != this);
}

#[test]
fn an_empty_pack_is_valid() {
    let bytes = Writer::new("np", "places", "2026.11.0")
        .and_then(|w| w.layout(false))
        .and_then(|l| l.assemble(&[]))
        .unwrap_or_else(|e| panic!("{e}"));
    let p = Pack::open_verified(&bytes).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(p.sections().len(), 0);
}

#[test]
fn signatures() {
    let signer_key = key(1);
    let bytes = build(true, &signer_key);
    let p = Pack::open_verified(&bytes).unwrap_or_else(|e| panic!("{e}"));
    let trusted = [key(2).verifying_key(), signer_key.verifying_key()];
    assert_eq!(p.verify_signature(&trusted), Ok(1));
    assert_eq!(
        p.signature().map(|s| s.0),
        Some(key_id(&signer_key.verifying_key()))
    );
    assert_eq!(
        p.verify_signature(&trusted[..1]),
        Err(Error::Signature("signed by a key that is not trusted"))
    );
    // A signature byte changed: the structure is fine, the signature is not.
    let mut bad = bytes.clone();
    // signature_off is the u64 at header offset 72; skip the 8-byte key ID.
    let sig_off = u64::from_le_bytes(bytes[72..80].try_into().unwrap_or_default());
    let sig_at = usize::try_from(sig_off).unwrap_or(usize::MAX) + 8 + 5;
    bad[sig_at] ^= 0x40;
    let p = Pack::open(&bad).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        p.verify_signature(&trusted),
        Err(Error::Signature("the signature is invalid"))
    );
    // Unsigned.
    let bytes = unsigned();
    let p = Pack::open(&bytes).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        p.verify_signature(&trusted),
        Err(Error::Signature("the pack is not signed"))
    );
}

#[test]
fn refusals() {
    let good = unsigned();
    let open = |b: &[u8]| Pack::open(b).err();
    assert_eq!(open(&[]), Some(Error::Truncated));
    assert_eq!(open(&good[..159]), Some(Error::Truncated));

    let mut b = good.clone();
    b[0] = b'X';
    assert_eq!(open(&b), Some(Error::Magic));

    // A pack that needs reader 9.
    let mut b = good.clone();
    b[12] = 9;
    assert_eq!(
        open(&b),
        Some(Error::Version {
            min_reader_major: 9
        })
    );

    // Truncated by one byte, and one byte appended.
    assert_eq!(open(&good[..good.len() - 1]), Some(Error::Magic));
    let mut b = good.clone();
    b.push(0);
    assert_eq!(open(&b), Some(Error::Magic));

    // The build ID is covered by the head hash.
    let mut b = good.clone();
    b[32] ^= 1;
    assert_eq!(open(&b), Some(Error::HeadHash));

    // A section byte changed: the structure is fine, the hash is not.
    let mut b = good.clone();
    let at = b.len() - 40;
    b[at] ^= 1;
    assert!(Pack::open(&b).is_ok());
    assert_eq!(
        Pack::open_verified(&b).err(),
        Some(Error::SectionHash {
            kind: Kind::NAMES.0
        })
    );
}

#[test]
fn writer_refusals() {
    let mut w = writer();
    let dup = w.section_bytes(Kind::PLACES, Encoding::RAW, 0, b"again");
    assert_eq!(
        dup.err(),
        Some(Error::Write("a section kind was given twice"))
    );
    // The failed call left the first section in place.
    let bytes = w.layout(false).and_then(|l| l.assemble(&[PLACES, NAMES]));
    assert!(bytes.is_ok());

    assert!(Writer::new("NP", "places", "1").is_err());
    assert!(Writer::new("np", "", "1").is_err());
    assert!(Writer::new("np", "places", "../1").is_err());
    let mut w = writer();
    assert!(w.manifest(Tag::ADDRESS, b"in").is_err());
    assert!(w.manifest(Tag(0x8123), b"").is_err());
    assert!(w.section_bytes(Kind(0), Encoding::RAW, 0, b"").is_err());
    assert!(
        w.section(Kind(40), Encoding::RAW, 0, 5, 4, [0; 32])
            .is_err()
    );

    // The head must fit in 16 KiB.
    let mut w = writer();
    let big = vec![0; 16 * 1024];
    assert!(w.manifest(Tag(0x0100), &big).is_ok());
    assert_eq!(
        w.layout(false).err(),
        Some(Error::Write("the head is longer than 16 KiB"))
    );

    // assemble checks the bytes against the plan.
    let layout = writer().layout(false).unwrap_or_else(|e| panic!("{e}"));
    assert!(layout.assemble(&[PLACES]).is_err());
    assert!(layout.assemble(&[PLACES, b"not the names"]).is_err());
    let signed = writer().layout(true).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        signed.assemble(&[PLACES, NAMES]).err(),
        Some(Error::Write("the signature has not been set"))
    );
}

#[test]
fn unknown_kinds_are_kept() {
    let mut w = Writer::new("np", "places", "2026.11.0").unwrap_or_else(|e| panic!("{e}"));
    let added = w.section_bytes(Kind(4000), Encoding(77), 1, b"future");
    assert!(added.is_ok());
    let bytes = w
        .layout(false)
        .and_then(|l| l.assemble(&[b"future"]))
        .unwrap_or_else(|e| panic!("{e}"));
    let p = Pack::open_verified(&bytes).unwrap_or_else(|e| panic!("{e}"));
    let s = p.section(Kind(4000));
    assert_eq!(
        s.map(|s| (s.encoding(), s.bytes())),
        Some((Encoding(77), &b"future"[..]))
    );
    assert_eq!(Kind(4000).to_string(), "kind 4000");
    assert_eq!(Kind::REDIRECTS.to_string(), "REDIRECTS");
}
