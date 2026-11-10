"""Writes the TUF differential corpus: repositories built and judged by python-tuf (D-161).

Each case is a directory of metadata files signed with python-tuf, and `case.json`: the steps a
client takes and python-tuf's verdict on each, from its `TrustedMetadataSet` at a fixed time.
`crates/teigeodata-sync/tests/tuf_differential.rs` replays the steps with our client and must
reach the same verdicts, except where a step records `ours` with the reason we differ.

    python -I tools/tuf-oracle/generate.py crates/teigeodata-sync/tests/tuf

The output is deterministic (fixed keys, times and Ed25519), so CI regenerates it and fails
on any difference.
"""

import copy
import json
import shutil
import sys
from datetime import datetime, timezone
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from securesystemslib.signer import CryptoSigner, SSlibKey
from tuf.api import exceptions
from tuf.api.metadata import (
    Metadata,
    MetaFile,
    Role,
    Root,
    Snapshot,
    TargetFile,
    Targets,
    Timestamp,
)
from tuf.api.serialization.json import JSONSerializer
from tuf.ngclient._internal.trusted_metadata_set import TrustedMetadataSet
from tuf.ngclient.config import EnvelopeType

NOW = datetime(2026, 11, 10, tzinfo=timezone.utc)
LATER = datetime(2027, 11, 10, tzinfo=timezone.utc)
PAST = datetime(2026, 11, 9, tzinfo=timezone.utc)
PACK = b"a pack's bytes"
SERIALIZER = JSONSerializer(compact=True)


def signer(seed: int) -> CryptoSigner:
    private = Ed25519PrivateKey.from_private_bytes(bytes([seed]) * 32)
    return CryptoSigner(private, SSlibKey.from_crypto(private.public_key(), keyid=f"k{seed}"))


def key(seed: int) -> SSlibKey:
    return signer(seed).public_key


def to_bytes(md: Metadata) -> bytes:
    return md.to_bytes(SERIALIZER)


def signed(payload, seeds) -> Metadata:
    md = Metadata(payload)
    for s in seeds:
        md.sign(signer(s), append=True)
    return md


def root(version, root_keys=(1, 2, 3), threshold=2, ts=10, sn=20, tg=30, expires=LATER) -> Root:
    r = Root(version=version, expires=expires, consistent_snapshot=True)
    r.roles = {name: Role([], 1) for name in ("root", "timestamp", "snapshot", "targets")}
    r.roles["root"].threshold = threshold
    for s in root_keys:
        r.add_key(key(s), "root")
    r.add_key(key(ts), "timestamp")
    r.add_key(key(sn), "snapshot")
    r.add_key(key(tg), "targets")
    return r


def metafile(version, data=None, hashes=True) -> MetaFile:
    if data is None or not hashes:
        return MetaFile(version=version)
    mf = MetaFile.from_data(version, data, ["sha256"])
    return mf


def targets(version, expires=LATER, custom=None, seeds=(30,)) -> bytes:
    t = Targets(version=version, expires=expires)
    tf = TargetFile.from_data("np.places.tgd", PACK, ["sha256"])
    if custom is not None:
        tf.unrecognized_fields["custom"] = custom
    t.targets[tf.path] = tf
    return to_bytes(signed(t, seeds))


def snapshot(version, targets_data, targets_version, expires=LATER, hashes=True, extra=None) -> bytes:
    s = Snapshot(version=version, expires=expires)
    s.meta = {"targets.json": metafile(targets_version, targets_data, hashes)}
    if extra:
        s.meta.update(extra)
    return to_bytes(signed(s, (20,)))


def timestamp(version, snapshot_data, snapshot_version, expires=LATER, seeds=(10,)) -> bytes:
    t = Timestamp(version=version, expires=expires, snapshot_meta=metafile(snapshot_version, snapshot_data))
    return to_bytes(signed(t, seeds))


class Case:
    def __init__(self, name, why, trusted_root: bytes):
        self.name, self.why, self.files, self.steps = name, why, {"root.json": trusted_root}, []

    def step(self, op, data: bytes, ours=None, path=None):
        file = f"{len(self.steps) + 1:02}-{op}.json" if op != "target" else f"{len(self.steps) + 1:02}-target.bin"
        self.files[file] = data
        s = {"op": op, "file": file}
        if path:
            s["path"] = path
        if ours:
            s["ours"], s["ours_why"] = ours
        self.steps.append(s)
        return self


def verdict(fn):
    try:
        fn()
        return "ok"
    except exceptions.ExpiredMetadataError:
        return "expired"
    except exceptions.EqualVersionNumberError:
        return "equal"
    except (exceptions.RepositoryError, exceptions.LengthOrHashMismatchError, RuntimeError, ValueError) as e:
        return "reject"


def judge(case: Case):
    trusted = TrustedMetadataSet(case.files["root.json"], EnvelopeType.METADATA)
    trusted.reference_time = NOW
    for s in case.steps:
        data = case.files[s["file"]]
        op = s["op"]
        if op == "root":
            s["python"] = verdict(lambda: trusted.update_root(data))
        elif op == "timestamp":
            s["python"] = verdict(lambda: trusted.update_timestamp(data))
        elif op in ("snapshot", "snapshot_stored"):
            s["python"] = verdict(lambda: trusted.update_snapshot(data, trusted=op == "snapshot_stored"))
        elif op == "targets":
            s["python"] = verdict(lambda: trusted.update_targets(data))
        elif op == "target":
            def check():
                trusted.targets.targets[s["path"]].verify_length_and_hashes(data)
            s["python"] = verdict(check)
        else:
            raise ValueError(op)


def cases():
    r1 = to_bytes(signed(root(1), (1, 2)))
    tg1 = targets(1)
    sn1 = snapshot(1, tg1, 1)
    ts1 = timestamp(1, sn1, 1)

    def chain(name, why):
        return Case(name, why, r1)

    yield chain("chain", "the whole chain and the target").step("timestamp", ts1).step("snapshot", sn1).step(
        "targets", tg1).step("target", PACK, path="np.places.tgd")
    yield chain("target-tampered", "a target whose bytes changed").step("timestamp", ts1).step(
        "snapshot", sn1).step("targets", tg1).step("target", PACK + b"!", path="np.places.tgd")

    # Root rotation: 1 -> 2 (new keys 4, 5, 6) -> 3.
    r2 = to_bytes(signed(root(2, (4, 5, 6)), (1, 2, 4, 5)))
    r3 = to_bytes(signed(root(3, (4, 5, 6)), (4, 5)))
    yield chain("root-rotation", "two rotations, then the chain").step("root", r2).step("root", r3).step(
        "timestamp", ts1).step("snapshot", sn1).step("targets", tg1)
    yield chain("root-new-keys-only", "v2 signed by its own keys alone").step(
        "root", to_bytes(signed(root(2, (4, 5, 6)), (4, 5))))
    yield chain("root-old-keys-only", "v2 signed by the old keys alone").step(
        "root", to_bytes(signed(root(2, (4, 5, 6)), (1, 2))))
    yield chain("root-skip", "v3 straight after v1").step("root", to_bytes(signed(root(3, (4, 5, 6)), (1, 2, 4, 5))))
    yield chain("root-same-version", "v1 again").step("root", r1)
    yield chain("root-below-threshold", "one of two root signatures").step(
        "root", to_bytes(signed(root(2), (1,))))
    expired_r2 = to_bytes(signed(root(2, expires=PAST), (1, 2)))
    r3_after = to_bytes(signed(root(3), (1, 2)))
    yield chain("root-expired-in-the-walk", "an expired root may be walked through").step(
        "root", expired_r2).step("root", r3_after).step("timestamp", ts1)
    yield chain("root-expired-final", "the last root expired").step("root", expired_r2).step("timestamp", ts1)

    # Timestamp.
    sn2 = snapshot(2, tg1, 1)
    ts2 = timestamp(2, sn2, 2)
    yield chain("timestamp-rollback", "a lower timestamp version").step("timestamp", ts2).step("timestamp", ts1)
    yield chain("timestamp-equal", "the same timestamp again").step("timestamp", ts1).step("timestamp", ts1)
    yield chain("timestamp-snapshot-rollback", "a newer timestamp naming an older snapshot").step(
        "timestamp", ts2).step("timestamp", timestamp(3, sn1, 1))
    yield chain("timestamp-expired", "an expired timestamp blocks the snapshot").step(
        "timestamp", timestamp(1, sn1, 1, expires=PAST)).step("snapshot", sn1)
    yield chain("timestamp-wrong-key", "signed by the snapshot key").step(
        "timestamp", timestamp(1, sn1, 1, seeds=(20,)))

    # Snapshot.
    other = snapshot(1, targets(2), 2)
    yield chain("snapshot-mix-and-match", "a validly signed snapshot of another hash").step(
        "timestamp", ts1).step("snapshot", other)
    yield chain("snapshot-version", "the timestamp names version 2, the snapshot is 1 by hash").step(
        "timestamp", timestamp(1, sn1, 2)).step("snapshot", sn1)
    yield chain("snapshot-stored-replaced", "a stored snapshot a newer timestamp replaced").step(
        "timestamp", ts2).step("snapshot_stored", sn1).step("targets", tg1)
    sn_more = snapshot(1, tg1, 1, extra={"extra.json": MetaFile(version=1)})
    sn_drop = snapshot(2, tg1, 1)
    yield Case("snapshot-drops-a-file", "a newer snapshot without a file the stored one had", r1).step(
        "timestamp", timestamp(2, sn_drop, 2)).step("snapshot_stored", sn_more).step("snapshot", sn_drop)
    sn_tg2 = snapshot(1, targets(2), 2)
    yield chain("snapshot-targets-rollback", "a newer snapshot with an older targets version").step(
        "timestamp", timestamp(2, sn1, 2)).step("snapshot_stored", sn_tg2).step("snapshot", sn1)
    yield chain("snapshot-expired", "an expired snapshot blocks the targets").step(
        "timestamp", timestamp(1, snapshot(1, tg1, 1, expires=PAST), 1)).step(
        "snapshot", snapshot(1, tg1, 1, expires=PAST)).step("targets", tg1)

    # Targets.
    yield chain("targets-mix-and-match", "targets of another hash").step("timestamp", ts1).step(
        "snapshot", sn1).step("targets", targets(2))
    by_version = snapshot(1, None, 2, hashes=False)
    yield chain("targets-version", "the snapshot names version 2 by version alone").step(
        "timestamp", timestamp(1, by_version, 1)).step("snapshot", by_version).step("targets", tg1).step(
        "targets", targets(2))
    exp_tg = targets(1, expires=PAST)
    exp_sn = snapshot(1, exp_tg, 1)
    yield chain("targets-expired", "expired targets").step("timestamp", timestamp(1, exp_sn, 1)).step(
        "snapshot", exp_sn).step("targets", exp_tg)
    wrong = targets(1, seeds=(20,))
    wrong_sn = snapshot(1, wrong, 1)
    yield chain("targets-wrong-key", "targets signed by the snapshot key").step(
        "timestamp", timestamp(1, wrong_sn, 1)).step("snapshot", wrong_sn).step("targets", wrong)

    # Canonical JSON: the signed bytes of awkward strings, keys and numbers.
    custom = {"Z": 1, "a": [0, -1, 18446744073709551615], "é": "q\"uo\\te\nline\ttab\u0001",
              "ü": {"b": True, "a": None}, "emoji": "\U0001f30f", "": ""}
    cj_tg = targets(1, custom=custom)
    cj_sn = snapshot(1, cj_tg, 1)
    yield chain("canonical-json", "custom data with escapes, control characters and non-ASCII keys").step(
        "timestamp", timestamp(1, cj_sn, 1)).step("snapshot", cj_sn).step("targets", cj_tg)

    # The same key under two key IDs, threshold 2, signed once under each ID.
    dup = root(2)
    dup.keys["k1b"] = copy.deepcopy(key(1))
    dup.keys["k1b"].keyid = "k1b"
    dup.roles["root"].keyids = ["k1", "k1b"]
    dup.roles["root"].threshold = 2
    md = Metadata(dup)
    md.sign(signer(1), append=True)
    md.sign(signer(2), append=True)
    s1b = CryptoSigner(Ed25519PrivateKey.from_private_bytes(bytes([1]) * 32), dup.keys["k1b"])
    md.sign(s1b, append=True)
    md3 = signed(root(3), (1, 2))
    md3.sign(s1b, append=True)
    yield chain("one-key-two-ids", "v2 lists one key under two IDs for a threshold of 2").step(
        "root", to_bytes(md),
        ours=("reject", "a threshold counts distinct keys, not key IDs (D-159); python-tuf counts IDs")).step(
        "root", to_bytes(md3), ours=("reject", "v2 was refused, so v3 skips a version"))


def main():
    out = Path(sys.argv[1])
    if out.exists():
        shutil.rmtree(out)
    index = []
    for case in cases():
        judge(case)
        d = out / case.name
        d.mkdir(parents=True)
        for name, data in case.files.items():
            (d / name).write_bytes(data)
        (d / "case.json").write_text(json.dumps(
            {"why": case.why, "steps": case.steps}, indent=1, sort_keys=True) + "\n")
        index.append(case.name)
    meta = {"cases": index, "now": int(NOW.timestamp()), "oracle": "python-tuf 7.0.1"}
    (out / "corpus.json").write_text(json.dumps(meta, indent=1) + "\n")
    print(f"{len(index)} cases written to {out}")


if __name__ == "__main__":
    main()
