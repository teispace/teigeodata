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
    DelegatedRole,
    Delegations,
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

    def step(self, op, data: bytes, ours=None, path=None, role=None):
        file = f"{len(self.steps) + 1:02}-{op}.json" if op != "target" else f"{len(self.steps) + 1:02}-target.bin"
        self.files[file] = data
        s = {"op": op, "file": file}
        if path:
            s["path"] = path
        if role:
            s["role"] = role
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


def lookup(trusted, path) -> TargetFile:
    """The target at `path` as python-tuf's updater finds it: the top-level targets, then the
    delegated roles that match it in order, stopping at a terminating one. A role that must be
    consulted and is not loaded is a refusal (the updater would fetch it; a step loads it)."""
    top = trusted.targets
    if path in top.targets:
        return top.targets[path]
    if top.delegations is not None:
        for name, terminating in top.delegations.get_roles_for_target(path):
            try:
                role = trusted[name]
            except KeyError:
                raise RuntimeError(f"{name} is not loaded")
            if path in role.targets:
                return role.targets[path]
            if terminating:
                break
    raise RuntimeError("no such target")


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
        elif op == "delegated":
            s["python"] = verdict(lambda: trusted.update_delegated_targets(data, s["role"], "targets"))
        elif op == "target":
            s["python"] = verdict(lambda: lookup(trusted, s["path"]).verify_length_and_hashes(data))
        else:
            raise ValueError(op)


def delegating(roles, extra_targets=(), hash_bins=False) -> bytes:
    """Top-level targets delegating `roles` (name, key seed, patterns, terminating), in order,
    with the targets of `extra_targets` (each path's bytes are the path)."""
    t = Targets(version=1, expires=LATER)
    for p in extra_targets:
        tf = TargetFile.from_data(p, p.encode(), ["sha256"])
        t.targets[tf.path] = tf
    keys, listed = {}, {}
    for name, seed, paths, terminating in roles:
        keys[f"k{seed}"] = key(seed)
        if hash_bins:
            listed[name] = DelegatedRole(name, [f"k{seed}"], 1, terminating, path_hash_prefixes=paths)
        else:
            listed[name] = DelegatedRole(name, [f"k{seed}"], 1, terminating, paths=list(paths))
    t.delegations = Delegations(keys, listed)
    return to_bytes(signed(t, (30,)))


def role_file(paths, seeds, version=1, expires=LATER, nested=None) -> bytes:
    """A delegated role listing `paths`, each with its own path as its bytes."""
    t = Targets(version=version, expires=expires)
    for p in paths:
        tf = TargetFile.from_data(p, p.encode(), ["sha256"])
        t.targets[tf.path] = tf
    if nested is not None:
        t.delegations = nested
    return to_bytes(signed(t, seeds))


def delegated_chain(r1, name, why, top, roles, hashes=True) -> Case:
    """A case past the top-level targets: the snapshot lists `top` and each of `roles` (name,
    bytes, version)."""
    extra = {f"{n}.json": metafile(v, data, hashes) for n, data, v in roles}
    sn = snapshot(1, top, 1, extra=extra)
    return Case(name, why, r1).step("timestamp", timestamp(1, sn, 1)).step("snapshot", sn).step("targets", top)


def delegation_cases(r1):
    """D-176: the catalog delegated by country, one level, `paths` patterns."""
    np_in = [("np", 40, ["np.*", "np-*"], True), ("in", 41, ["in.*", "in-*"], True)]
    top = delegating(np_in, extra_targets=["earth.places.tgd"])
    np1 = role_file(["np.places.tgd", "np-p1.places.tgd", "in-x.places.tgd"], (40,))
    in1 = role_file(["in.places.tgd"], (41,))
    both = [("np", np1, 1), ("in", in1, 1)]
    yield delegated_chain(r1, "deleg-chain", "a country's role and its packs", top, both).step(
        "target", b"earth.places.tgd", path="earth.places.tgd").step(
        "target", b"np.places.tgd", path="np.places.tgd").step(
        "delegated", np1, role="np").step("target", b"np.places.tgd", path="np.places.tgd").step(
        "target", b"np-p1.places.tgd", path="np-p1.places.tgd").step(
        "target", b"np.places.tgd!", path="np.places.tgd").step(
        "target", b"in-x.places.tgd", path="in-x.places.tgd").step(
        "delegated", in1, role="in").step("target", b"in.places.tgd", path="in.places.tgd").step(
        "target", b"in-x.places.tgd", path="in-x.places.tgd")
    for name, why, seeds in [("deleg-wrong-key", "Nepal's role signed by India's key", (41,)),
                             ("deleg-targets-key", "Nepal's role signed by the top-level targets key", (30,)),
                             ("deleg-unsigned", "Nepal's role unsigned", ())]:
        bad = role_file(["np.places.tgd"], seeds)
        yield delegated_chain(r1, name, why, top, [("np", bad, 1)]).step("delegated", bad, role="np")
    yield delegated_chain(r1, "deleg-mix-and-match", "a role of another hash than the snapshot's", top,
                          [("np", np1, 1)]).step("delegated", role_file(["np.places.tgd"], (40,)), role="np")
    yield delegated_chain(r1, "deleg-version", "the snapshot names version 2 by version alone", top,
                          [("np", np1, 2)], hashes=False).step("delegated", np1, role="np")
    old = role_file(["np.places.tgd"], (40,), expires=PAST)
    yield delegated_chain(r1, "deleg-expired", "an expired role", top, [("np", old, 1)]).step(
        "delegated", old, role="np")
    yield delegated_chain(r1, "deleg-unknown-role", "a role the targets do not delegate", top, both).step(
        "delegated", np1, role="fj")
    yield delegated_chain(r1, "deleg-unlisted-role", "a role the snapshot does not list", top,
                          [("np", np1, 1)]).step("delegated", in1, role="in")

    # Order and terminating roles: a (x*, not terminating), b (x*, terminating), c (*).
    abc = delegating([("a", 40, ["x*"], False), ("b", 41, ["x*"], True), ("c", 42, ["*"], False)])
    ra, rb, rc = role_file(["xa"], (40,)), role_file(["xa", "xb"], (41,)), role_file(["xc", "zz"], (42,))
    yield delegated_chain(r1, "deleg-terminating", "roles in order; a terminating role ends the search",
                          abc, [("a", ra, 1), ("b", rb, 1), ("c", rc, 1)]).step(
        "delegated", ra, role="a").step("delegated", rb, role="b").step("delegated", rc, role="c").step(
        "target", b"xa", path="xa").step("target", b"xb", path="xb").step("target", b"zz", path="zz").step(
        "target", b"xc", path="xc")
    # A star never crosses a slash; a question mark is one character.
    seg = delegating([("s", 40, ["*.tgd", "??.x", "d/*"], True)])
    rs = role_file(["a/b.tgd", "b.tgd", "ने.x", "n.x", "ab.x", "d/e", "d/e/f"], (40,))
    yield delegated_chain(r1, "deleg-segments", "patterns match one path segment at a time", seg,
                          [("s", rs, 1)]).step("delegated", rs, role="s").step(
        "target", b"b.tgd", path="b.tgd").step("target", b"a/b.tgd", path="a/b.tgd").step(
        "target", "ने.x".encode(), path="ने.x").step("target", b"n.x", path="n.x").step(
        "target", b"ab.x", path="ab.x").step("target", b"d/e", path="d/e").step(
        "target", b"d/e/f", path="d/e/f")
    # An earlier matching role not loaded: the search never skips it for a later one.
    skip = delegating([("y", 40, ["y*"], False), ("all", 41, ["*"], False)])
    ry, rall = role_file(["yy"], (40,)), role_file(["yy"], (41,))
    yield delegated_chain(r1, "deleg-unloaded-first", "a later role while an earlier matching one is not loaded",
                          skip, [("y", ry, 1), ("all", rall, 1)]).step("delegated", rall, role="all").step(
        "target", b"yy", path="yy").step("delegated", ry, role="y").step("target", b"yy", path="yy")

    # Where we are stricter than python-tuf, each with its reason.
    nested = Delegations({"k42": key(42)}, {"np-east": DelegatedRole("np-east", ["k42"], 1, True, paths=["np-e*"])})
    np_nested = role_file(["np.places.tgd"], (40,), nested=nested)
    yield delegated_chain(r1, "deleg-nested", "a country's role that delegates again", top,
                          [("np", np_nested, 1)]).step(
        "delegated", np_nested, role="np",
        ours=("reject", "one level of delegation (D-176): the catalog never nests, so a nested role is a fault"))
    bins = delegating([("bin-0", 40, ["0"], False)], hash_bins=True)
    yield Case("deleg-hash-bins", "a role by path_hash_prefixes (hash bins)", r1).step(
        "timestamp", timestamp(1, snapshot(1, bins, 1), 1)).step("snapshot", snapshot(1, bins, 1)).step(
        "targets", bins, ours=("reject", "path_hash_prefixes are refused, never misread (D-176)"))
    klass = delegating([("np", 40, ["np[.]*"], True)])
    yield Case("deleg-character-class", "a pattern with a character class", r1).step(
        "timestamp", timestamp(1, snapshot(1, klass, 1), 1)).step("snapshot", snapshot(1, klass, 1)).step(
        "targets", klass, ours=("reject", "only * and ? globs are read; a [class] is refused (D-176)"))
    unsafe = delegating([("..", 40, ["np.*"], True)])
    yield Case("deleg-unsafe-name", "a role named ..", r1).step(
        "timestamp", timestamp(1, snapshot(1, unsafe, 1), 1)).step("snapshot", snapshot(1, unsafe, 1)).step(
        "targets", unsafe, ours=("reject", "a role name becomes a file name, so it must be a safe one (D-176)"))


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

    yield from delegation_cases(r1)


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
