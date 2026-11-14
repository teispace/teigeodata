"""The audit of the abi3t wheel (D-290): `abi3audit --strict` with one exception.

abi3audit 0.0.26 does not know PEP 793's `PyModExport_<module>`, the export hook a 3.15 module
defines (abi3t has no `PyInit_` hook), and reports it as outside the limited API. This runs
abi3audit and fails on anything else it reports: any other symbol, a version mismatch, an
extension that is not abi3 for another reason, or a wheel it could not read.

Usage: python abi3t_audit.py WHEEL  (cibuildwheel's `audit-command`, bindings/python/pyproject.toml)
"""

import json
import os
import subprocess
import sys


def problems(report):
    """What the report holds beyond the PEP 793 hook of each extension."""
    found = []
    for spec, entry in report.get("specs", {}).items():
        extensions = entry.get("wheel") or []
        if not extensions:
            found.append(f"{spec}: no extension audited")
        for ext in extensions:
            name, result = ext.get("name", "?"), ext.get("result") or {}
            module = name.split(".", 1)[0]
            hook = f"PyModExport_{module}"
            extra = [s for s in result.get("non_abi3_symbols", []) if s != hook]
            if extra:
                found.append(f"{name}: outside the limited API: {', '.join(extra)}")
            if not result.get("is_abi3_baseline_compatible", False):
                found.append(
                    f"{name}: needs {result.get('computed')}, tagged {result.get('baseline')}"
                )
            if result.get("future_abi3_objects"):
                found.append(f"{name}: newer than its tag: {result['future_abi3_objects']}")
    return found


def main(argv):
    if len(argv) != 2:
        print(__doc__.strip().splitlines()[-1], file=sys.stderr)
        return 2
    run = subprocess.run(
        [sys.executable, "-m", "abi3audit", "--report", argv[1]],
        capture_output=True,
        text=True,
        check=False,
    )
    sys.stderr.write(run.stderr)
    try:
        report = json.loads(run.stdout)
    except json.JSONDecodeError:
        print(f"abi3audit gave no report (exit {run.returncode}):\n{run.stdout}", file=sys.stderr)
        return 1
    found = problems(report)
    for p in found:
        print(p, file=sys.stderr)
    if found:
        return 1
    print(f"{os.path.basename(argv[1])}: abi3t, only the PEP 793 hook outside abi3audit's table")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
