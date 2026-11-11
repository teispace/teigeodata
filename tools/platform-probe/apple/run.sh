#!/usr/bin/env bash
# The Apple prototype end to end (D-184): a repository on localhost, a store, and the loop a
# platform runs: `tgd install --downloads` lists ranges, the URLSession probe fetches them,
# and install runs again until the pack is committed.
#
# Usage: run.sh REPO_DIR WORK_DIR [--background] [--redirect]
#   REPO_DIR: written by `TGD_REPO_OUT=DIR cargo test -p teigeodata-cli --test sync -- --ignored write_repo`
#   TGD (env): the tgd binary (default target/debug/tgd)
set -euo pipefail
repo=$1; work=$2; shift 2
bg=""; redirect=""
for a in "$@"; do
  case $a in
    --background) bg="--background" ;;
    --redirect) redirect="--redirect" ;;
  esac
done
here=$(cd "$(dirname "$0")" && pwd)
tgd=${TGD:-target/debug/tgd}
rm -rf "$work"; mkdir -p "$work"
swiftc -O -o "$work/probe" "$here/ProbeCore.swift" "$here/cli/main.swift"
log="$work/requests.jsonl"
python3 "$here/../serve.py" "$repo" $redirect --log "$log" > "$work/url" &
server=$!
trap 'kill $server 2>/dev/null || true' EXIT
until [ -s "$work/url" ]; do sleep 0.1; done
url=$(cat "$work/url")
store="$work/store"
"$tgd" --store "$store" init --repo "$url/meta" --assets "$url/assets/" --root "$repo/root.json" >/dev/null
"$tgd" --store "$store" refresh --countries np
round=0
while :; do
  round=$((round + 1))
  set +e
  "$tgd" --store "$store" install np --downloads "$work/dl" > "$work/pending.json"
  code=$?
  set -e
  if [ $code -eq 0 ]; then break; fi
  if [ $code -ne 4 ]; then echo "install failed: $code"; exit 1; fi
  echo "round $round: $(python3 -c 'import json,sys; a=json.load(open(sys.argv[1])); r=[x for p in a for x in p["ranges"]]; print(len(r), "ranges,", sum(x["end"]-x["start"] for x in r), "bytes")' "$work/pending.json")"
  "$work/probe" "$work/pending.json" $bg
  [ $round -lt 5 ] || { echo "too many rounds"; exit 1; }
done
echo "installed after $round passes"
"$tgd" --store "$store" status --deep
echo "server saw:"
python3 - "$log" <<'EOF'
import collections, json, sys
seen = collections.Counter()
for line in open(sys.argv[1]):
    r = json.loads(line)
    if "/assets/" in r["path"] or "/blob/" in r["path"]:
        seen[(r["status"], "Range" if r["range"] else "no Range")] += 1
for (status, rng), n in sorted(seen.items()):
    print(f"  {n} asset requests: {status}, {rng}")
EOF
