#!/usr/bin/env bash
# The iOS prototype (D-184): the app in ios/ on a simulator of its own, with a background
# URLSession. The big round is cut short on purpose: the app is killed while the download runs
# in the system's daemon, the download finishes without it, and the relaunched app reattaches to
# the session and files what finished. Then `tgd install --downloads` commits the pack.
#
# Usage: run-ios.sh REPO_DIR WORK_DIR [--redirect]
#   TGD (env): the tgd binary (default target/debug/tgd)
#   RATE (env): the server's bytes a second for the big round (default 300000)
#   RUNTIME, DEVICE_TYPE (env): the simulator to create (default iOS 27.0, iPhone 17 Pro)
set -euo pipefail
repo=$1; work=$2; shift 2
redirect=""
[ "${1:-}" = "--redirect" ] && redirect="--redirect"
here=$(cd "$(dirname "$0")" && pwd)
tgd=${TGD:-target/debug/tgd}
rate=${RATE:-300000}
runtime=${RUNTIME:-com.apple.CoreSimulator.SimRuntime.iOS-27-0}
devtype=${DEVICE_TYPE:-com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro}
bundle=org.teigeodata.probe
rm -rf "$work"; mkdir -p "$work"

# The app, built without an Xcode project.
app="$work/Probe.app"; mkdir -p "$app"
sdk=$(xcrun --sdk iphonesimulator --show-sdk-path)
xcrun --sdk iphonesimulator swiftc -parse-as-library -O -sdk "$sdk" \
  -target "$(uname -m)-apple-ios17.0-simulator" \
  -o "$app/Probe" "$here/ProbeCore.swift" "$here/ios/App.swift"
cat > "$app/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>$bundle</string>
  <key>CFBundleExecutable</key><string>Probe</string>
  <key>CFBundleName</key><string>Probe</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  <key>MinimumOSVersion</key><string>17.0</string>
  <key>LSRequiresIPhoneOS</key><true/>
  <key>UIDeviceFamily</key><array><integer>1</integer></array>
  <key>UILaunchScreen</key><dict/>
  <key>UIApplicationSceneManifest</key><dict>
    <key>UIApplicationSupportsMultipleScenes</key><false/>
  </dict>
  <key>NSAppTransportSecurity</key><dict><key>NSAllowsLocalNetworking</key><true/></dict>
</dict></plist>
EOF
codesign --force --sign - "$app" >/dev/null

# A simulator of its own, removed at the end.
dev=$(xcrun simctl create tgd-probe "$devtype" "$runtime")
log="$work/requests.jsonl"
cleanup() {
  kill "${server:-0}" 2>/dev/null || true
  xcrun simctl shutdown "$dev" 2>/dev/null || true
  xcrun simctl delete "$dev" 2>/dev/null || true
}
trap cleanup EXIT
xcrun simctl boot "$dev"
xcrun simctl bootstatus "$dev" -b >/dev/null
xcrun simctl install "$dev" "$app"
docs="$(xcrun simctl get_app_container "$dev" "$bundle" data)/Documents"
mkdir -p "$docs"
# The files the app downloaded, copied out of its container for the next pass.
collect() { [ -d "$docs/dl" ] && mkdir -p "$work/dl" && cp -R "$docs/dl/." "$work/dl/" || true; }

python3 "$here/../serve.py" "$repo" $redirect --log "$log" --rate "$rate" > "$work/url" &
server=$!
disown "$server"
until [ -s "$work/url" ]; do sleep 0.1; done
url=$(cat "$work/url")
store="$work/store"
"$tgd" --store "$store" init --repo "$url/meta" --assets "$url/assets/" --root "$repo/root.json" >/dev/null
"$tgd" --store "$store" refresh --countries np >/dev/null

wait_for() {  # file, pattern, seconds
  local i=0
  until grep -q "$2" "$1" 2>/dev/null; do
    sleep 0.5; i=$((i + 1))
    [ $i -lt $(($3 * 2)) ] || { echo "timed out waiting for $2 in $1"; return 1; }
  done
}
pending() {
  set +e
  "$tgd" --store "$store" install np --downloads "$work/dl" > "$work/pending.json"
  local code=$?
  set -e
  echo $code
}

res="$docs/results.jsonl"
# Launches the app, retrying while a previous instance is still exiting.
launch() {
  local i=0
  until xcrun simctl launch "$dev" "$bundle" "$@" >/dev/null 2>&1; do
    sleep 1; i=$((i + 1)); [ $i -lt 15 ] || { echo "could not launch the app"; return 1; }
  done
}
finished() { grep -c '"event":"finished"' "$res" 2>/dev/null || echo 0; }
wait_finished() {  # count, seconds
  local i=0
  until [ "$(finished)" -ge "$1" ]; do
    sleep 0.5; i=$((i + 1)); [ $i -lt $(($2 * 2)) ] || return 1
  done
}

# Round 1: the table and the head, the app left to finish.
[ "$(pending)" = 4 ] || { echo "expected ranges to download"; exit 1; }
cp "$work/pending.json" "$docs/pending1.json"
launch start pending1.json
wait_finished 1 60 || { echo "round 1 did not finish"; cat "$res"; exit 1; }
echo "round 1: $(grep -c '"ok":true' "$res") ranges filed"
collect

# Round 2: every frame; the app is killed while the daemon downloads.
[ "$(pending)" = 4 ] || { echo "expected the frames to download"; exit 1; }
big=$(python3 -c 'import json,sys; a=json.load(open(sys.argv[1])); print(sum(r["end"]-r["start"] for p in a for r in p["ranges"]))' "$work/pending.json")
cp "$work/pending.json" "$docs/pending2.json"
launch start pending2.json
wait_for "$res" '"tasks"' 60
sleep 2
xcrun simctl terminate "$dev" "$bundle"
echo "round 2: $big bytes; app killed after 2 s"
[ "$(finished)" -ge 2 ] && { echo "the download finished before the kill: lower RATE"; exit 1; }
i=0
until python3 -c 'import json,sys
n=int(sys.argv[2])
sys.exit(0 if any(json.loads(l)["status"]==206 and json.loads(l)["bytes"]==n for l in open(sys.argv[1])) else 1)' "$log" "$big"; do
  sleep 1; i=$((i + 1)); [ $i -lt 120 ] || { echo "the daemon did not finish the download"; exit 1; }
done
echo "the daemon finished the download while the app was dead"
# iOS relaunches the app in the background to deliver the events; if it does not within 30 s,
# a user's launch reattaches.
if wait_finished 2 30; then
  echo "delivered by the system's relaunch"
else
  echo "no system relaunch in 30 s: launching as a user would"
  launch
  wait_finished 2 60 || { echo "round 2 never delivered"; cat "$res"; exit 1; }
fi
grep -E '"event"|"file"' "$res" | tail -6
collect

# Round 3: commit.
code=$(pending)
[ "$code" = 0 ] || { echo "install did not commit: $code"; cat "$work/pending.json"; exit 1; }
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
