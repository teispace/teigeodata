#!/usr/bin/env bash
# The Android prototype (D-184): the app in this directory, built without Gradle, on an
# emulator of its own. Each pass of `tgd install --downloads` is downloaded by a user-initiated
# data transfer job scheduled from an activity that removes its own task at once. In the big
# round the app's process is killed mid-download, to see the system restart the job. Then
# `tgd install --downloads` commits the pack. The emulator and its AVD are deleted at the end.
#
# Usage: run-android.sh REPO_DIR WORK_DIR [--redirect]
#   TGD (env): the tgd binary (default target/debug/tgd)
#   RATE (env): the server's bytes a second (default 300000)
#   ANDROID_HOME (env): the SDK (default ~/Library/Android/sdk)
set -euo pipefail
repo=$1; work=$2; shift 2
redirect=""
[ "${1:-}" = "--redirect" ] && redirect="--redirect"
here=$(cd "$(dirname "$0")" && pwd)
tgd=${TGD:-target/debug/tgd}
rate=${RATE:-300000}
sdk=${ANDROID_HOME:-$HOME/Library/Android/sdk}
bt="$sdk/build-tools/$(ls "$sdk/build-tools" | sort -V | tail -1)"
jar="$sdk/platforms/android-35/android.jar"
image="system-images;android-35;google_apis_playstore;arm64-v8a"
avd=tgd-probe
port=5584
serial="emulator-$port"
pkg=org.teigeodata.probe
adb="$sdk/platform-tools/adb -s $serial"
rm -rf "$work"; mkdir -p "$work"

# The APK, built without Gradle: aapt2, javac, d8, zipalign, apksigner.
b="$work/build"; mkdir -p "$b/classes" "$b/dex"
"$bt/aapt2" link -o "$b/base.apk" -I "$jar" --manifest "$here/AndroidManifest.xml"
javac --release 17 -nowarn -classpath "$jar" -d "$b/classes" $(find "$here/src" -name '*.java')
"$bt/d8" --release --min-api 34 --lib "$jar" --output "$b/dex" $(find "$b/classes" -name '*.class')
cp "$b/base.apk" "$b/unsigned.apk"
(cd "$b/dex" && zip -q "$b/unsigned.apk" classes.dex)
"$bt/zipalign" -f 4 "$b/unsigned.apk" "$b/aligned.apk"
keytool -genkeypair -keystore "$b/test.jks" -storepass testpass -keypass testpass -alias test \
  -keyalg RSA -keysize 2048 -validity 1 -dname "CN=teigeodata probe" >/dev/null 2>&1
"$bt/apksigner" sign --ks "$b/test.jks" --ks-pass pass:testpass --out "$work/probe.apk" "$b/aligned.apk"

# An emulator of its own.
echo no | "$sdk/cmdline-tools/latest/bin/avdmanager" -s create avd -n "$avd" -k "$image" --force >/dev/null
cleanup() {
  kill "${server:-0}" 2>/dev/null || true
  $adb emu kill >/dev/null 2>&1 || true
  sleep 2
  "$sdk/cmdline-tools/latest/bin/avdmanager" -s delete avd -n "$avd" >/dev/null 2>&1 || true
}
trap cleanup EXIT
"$sdk/emulator/emulator" -avd "$avd" -port "$port" -no-window -no-audio -no-snapshot \
  -no-boot-anim -memory 2048 -gpu swiftshader_indirect > "$work/emulator.log" 2>&1 &
disown $!
$adb wait-for-device
until [ "$($adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ]; do sleep 2; done
$adb install -r "$work/probe.apk" >/dev/null
$adb shell pm grant "$pkg" android.permission.POST_NOTIFICATIONS

python3 "$here/../serve.py" "$repo" $redirect --log "$work/requests.jsonl" --rate "$rate" > "$work/url" &
server=$!
disown "$server"
until [ -s "$work/url" ]; do sleep 0.1; done
url=$(cat "$work/url")
$adb reverse "tcp:${url##*:}" "tcp:${url##*:}" >/dev/null
store="$work/store"
"$tgd" --store "$store" init --repo "$url/meta" --assets "$url/assets/" --root "$repo/root.json" >/dev/null
"$tgd" --store "$store" refresh --countries np >/dev/null

results() { $adb exec-out run-as "$pkg" cat files/results.jsonl 2>/dev/null || true; }
count() { results | grep -c "$1" || true; }
wait_count() {  # pattern, count, seconds
  local i=0
  until [ "$(count "$1")" -ge "$2" ]; do
    sleep 1; i=$((i + 1)); [ $i -lt "$3" ] || return 1
  done
}
collect() {
  rm -rf "$work/out"; mkdir -p "$work/out" "$work/dl"
  $adb exec-out run-as "$pkg" tar cf - files/dl 2>/dev/null | tar xf - -C "$work/out" 2>/dev/null || true
  [ -d "$work/out/files/dl" ] && cp -R "$work/out/files/dl/." "$work/dl/"
}
pending() {
  set +e
  "$tgd" --store "$store" install np --downloads "$work/dl" > "$work/pending.json"
  local code=$?
  set -e
  echo $code
}
schedule() {
  local json bytes
  json=$(python3 -c 'import json,sys; print(json.dumps(json.load(open(sys.argv[1])), separators=(",",":")))' "$work/pending.json")
  bytes=$(python3 -c 'import json,sys; a=json.load(open(sys.argv[1])); print(sum(r["end"]-r["start"] for p in a for r in p["ranges"]))' "$work/pending.json")
  $adb shell "am start -W -n $pkg/.MainActivity --es pending '$json' --el bytes $bytes" >/dev/null
  echo "$bytes"
}

# Round 1: the table and the head.
[ "$(pending)" = 4 ] || { echo "expected ranges to download"; exit 1; }
schedule >/dev/null
wait_count '"event":"finished"' 1 90 || { echo "round 1 did not finish"; results; exit 1; }
echo "round 1: $(count '"file".*"ok":true') ranges filed"
collect

# Round 2: every frame; the process is killed mid-download.
[ "$(pending)" = 4 ] || { echo "expected the frames to download"; exit 1; }
big=$(schedule)
wait_count '"event":"started"' 2 60
sleep 2
pid=$($adb shell pidof "$pkg" | tr -d '\r')
$adb shell run-as "$pkg" kill -9 "$pid"
echo "round 2: $big bytes; process $pid killed after 2 s"
if wait_count '"event":"finished"' 2 180; then
  echo "the job ran again and finished"
else
  echo "the job did not finish within 180 s"; results; exit 1
fi
results | tail -6
collect

# Round 3: commit.
code=$(pending)
[ "$code" = 0 ] || { echo "install did not commit: $code"; cat "$work/pending.json"; exit 1; }
"$tgd" --store "$store" status --deep
echo "server saw:"
python3 - "$work/requests.jsonl" <<'EOF'
import collections, json, sys
seen = collections.Counter()
for line in open(sys.argv[1]):
    r = json.loads(line)
    if "/assets/" in r["path"] or "/blob/" in r["path"]:
        seen[(r["status"], "Range" if r["range"] else "no Range")] += 1
for (status, rng), n in sorted(seen.items()):
    print(f"  {n} asset requests: {status}, {rng}")
EOF
