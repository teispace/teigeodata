"""python-tuf reads the repository teigeodata-sync's `repo` writer wrote (D-162).

    python -I tools/tuf-oracle/read_written.py crates/teigeodata-sync/tests/tuf-written

Loads the root, timestamp, snapshot and targets with python-tuf's `TrustedMetadataSet` and
checks the target file against its length and hashes, as python-tuf's updater would.
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
for path, target in trusted.targets.targets.items():
    custom = target.unrecognized_fields["custom"]
    assert custom["schema"] == 1
    target.verify_length_and_hashes((d / custom["file"]).read_bytes())
print(f"python-tuf read the written repository: {len(trusted.targets.targets)} target(s)")
