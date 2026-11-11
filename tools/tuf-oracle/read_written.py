"""python-tuf reads the repository teigeodata-sync's `repo` writer wrote (D-162), delegated by
country (D-176).

    python -I tools/tuf-oracle/read_written.py crates/teigeodata-sync/tests/tuf-written

Loads the root, timestamp, snapshot, targets and every delegated role with python-tuf's
`TrustedMetadataSet`, finds each pack's target as python-tuf's updater would (the top-level
targets, then the matching roles in order), and checks the asset against its length and hashes.
"""

import sys
from datetime import datetime, timezone
from pathlib import Path

from tuf.ngclient._internal.trusted_metadata_set import TrustedMetadataSet
from tuf.ngclient.config import EnvelopeType

d = Path(sys.argv[1])
trusted = TrustedMetadataSet((d / "1.root.json").read_bytes(), EnvelopeType.METADATA)
trusted.reference_time = datetime(2026, 11, 10, tzinfo=timezone.utc)
trusted.update_timestamp((d / "timestamp.json").read_bytes())
trusted.update_snapshot((d / "1.snapshot.json").read_bytes())
trusted.update_targets((d / "1.targets.json").read_bytes())
top = trusted.targets
roles = list(top.delegations.roles) if top.delegations else []
for name in roles:
    version = trusted.snapshot.meta[f"{name}.json"].version
    trusted.update_delegated_targets((d / f"{version}.{name}.json").read_bytes(), name, "targets")


def lookup(path):
    if path in top.targets:
        return top.targets[path]
    for name, terminating in top.delegations.get_roles_for_target(path):
        if path in trusted[name].targets:
            return trusted[name].targets[path]
        if terminating:
            break
    raise AssertionError(f"{path}: no role python-tuf would trust lists it")


paths = list(top.targets) + [p for name in roles for p in trusted[name].targets]
for path in paths:
    target = lookup(path)
    custom = target.unrecognized_fields["custom"]
    assert custom["schema"] == 1
    target.verify_length_and_hashes((d / custom["file"]).read_bytes())
assert len(roles) == 1 and len(paths) == 2, (roles, paths)
print(f"python-tuf read the written repository: {len(paths)} target(s), {len(roles)} delegated role(s)")
