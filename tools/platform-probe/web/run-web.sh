#!/usr/bin/env bash
# The browser prototype (D-184): headless Chrome with a profile of its own runs the page in this
# directory, served from one origin, against the mirror on another (so CORS is real). The worker
# fetches each range into OPFS. In the big round the browser is killed mid-download and started
# again on the same profile: the worker resumes the range from what OPFS kept. The page hands the
# files back, and `tgd install --downloads` checks and commits them.
#
# Usage: run-web.sh REPO_DIR WORK_DIR [--redirect]
#   TGD (env): the tgd binary (default target/debug/tgd)
#   RATE (env): the mirror's bytes a second (default 300000)
#   CHROME (env): the browser (default Google Chrome)
set -euo pipefail
repo=$1; work=$2; shift 2
redirect=""
[ "${1:-}" = "--redirect" ] && redirect="--redirect"
here=$(cd "$(dirname "$0")" && pwd)
tgd=${TGD:-target/debug/tgd}
rate=${RATE:-300000}
chrome=${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}
rm -rf "$work"; mkdir -p "$work/site" "$work/upload"
cp "$here/index.html" "$here/worker.js" "$work/site/"

serve() {  # dir, url file, extra args...
  local dir=$1 out=$2; shift 2
  python3 "$here/../serve.py" "$dir" "$@" > "$out" &
  local pid=$!
  disown "$pid"
  until [ -s "$out" ]; do sleep 0.1; done
  echo "$pid"
}
mirror_pid=$(serve "$repo" "$work/mirror" $redirect --log "$work/requests.jsonl" --rate "$rate")
site_pid=$(serve "$work/site" "$work/site-url" --upload "$work/upload" --log "$work/site.jsonl")
mirror=$(cat "$work/mirror"); site=$(cat "$work/site-url")
browser=0
cleanup() {
  kill "$mirror_pid" "$site_pid" 2>/dev/null || true
  pkill -9 -f "user-data-dir=$work/profile" 2>/dev/null || true
  rm -rf "$work/profile"
}
trap cleanup EXIT
open_page() {  # round
  "$chrome" --headless=new --user-data-dir="$work/profile" --no-first-run --no-default-browser-check \
    --disable-gpu --disable-extensions "$site/index.html?round=$1" > "$work/chrome-$1.log" 2>&1 &
  browser=$!
  disown "$browser"
}
# Every process of this profile: the browser and its helpers.
close_page() {
  pkill -f "user-data-dir=$work/profile" 2>/dev/null || true
  sleep 1
  pkill -9 -f "user-data-dir=$work/profile" 2>/dev/null || true
  browser=0
}
wait_file() {  # path, seconds
  local i=0
  until [ -s "$1" ]; do
    sleep 0.5; i=$((i + 1)); [ $i -lt $(($2 * 2)) ] || { echo "timed out waiting for $1"; return 1; }
  done
}
store="$work/store"
"$tgd" --store "$store" init --repo "$mirror/meta" --assets "$mirror/assets/" --root "$repo/root.json" >/dev/null
"$tgd" --store "$store" refresh --countries np >/dev/null
pending() {  # round
  set +e
  "$tgd" --store "$store" install np --downloads "$work/upload/dl" > "$work/site/pending-$1.json"
  local code=$?
  set -e
  echo $code
}
show() { python3 -c 'import json,sys; [print(json.dumps(r)) for r in json.load(open(sys.argv[1]))["results"]]' "$1"; }

# Round 1: the table and the head.
[ "$(pending 1)" = 4 ] || { echo "expected ranges to download"; exit 1; }
open_page 1
wait_file "$work/upload/results-1.json" 90
close_page
show "$work/upload/results-1.json"

# Round 2: every frame; the browser is killed mid-download.
[ "$(pending 2)" = 4 ] || { echo "expected the frames to download"; exit 1; }
open_page 2
wait_file "$work/upload/started-2" 90
sleep 2
pkill -9 -f "user-data-dir=$work/profile"; browser=0
sleep 1
echo "round 2: browser killed 2 s into the download"
[ -s "$work/upload/results-2.json" ] && { echo "finished before the kill: lower RATE"; exit 1; }
cp "$work/site/pending-2.json" "$work/site/pending-2b.json"
open_page 2b
wait_file "$work/upload/results-2b.json" 120
close_page
show "$work/upload/results-2b.json"
python3 - "$work/upload/results-2b.json" <<'EOF'
import json, sys
r = json.load(open(sys.argv[1]))["results"]
assert all(x["ok"] for x in r), r
assert any(x.get("resumedFrom") for x in r), "the second session resumed no range from OPFS"
print("resumed from OPFS:", [x["resumedFrom"] for x in r if x.get("resumedFrom")])
EOF

# Round 3: commit.
code=$(pending 3)
[ "$code" = 0 ] || { echo "install did not commit: $code"; cat "$work/site/pending-3.json"; exit 1; }
"$tgd" --store "$store" status --deep
echo "mirror saw:"
python3 - "$work/requests.jsonl" <<'EOF'
import collections, json, sys
seen = collections.Counter()
for line in open(sys.argv[1]):
    r = json.loads(line)
    if "/assets/" in r["path"] or "/blob/" in r["path"]:
        seen[(r["method"], r["status"], "Range" if r["range"] else "no Range")] += 1
for (m, status, rng), n in sorted(seen.items()):
    print(f"  {n} asset requests: {m} {status}, {rng}")
EOF
