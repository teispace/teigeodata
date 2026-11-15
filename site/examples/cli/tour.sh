#!/bin/sh
# The command-line tour: every query of the CLI guide, run on a released pack.
# Usage: sh tour.sh world-np-lite.tgd   (TGD names the binary; default `tgd` on PATH)
set -eu
PACK=$1
tgd() { "${TGD:-tgd}" "$@"; }

echo "=== inspect"
# region inspect
tgd inspect "$PACK" | sed -n '1,4p'
# endregion inspect

echo "=== search"
# region search
tgd search Pokhara -k 3 --pack "$PACK"
# endregion search

echo "=== search-parent"
# region search-parent
tgd search "Kathmandu, Bagmati" -k 2 --pack "$PACK"
# endregion search-parent

echo "=== search-focus"
# region search-focus
tgd search Shivapur -k 3 --focus 28.7,80.6 --scale-km 30 --pack "$PACK"
# endregion search-focus

echo "=== get"
# region get
tgd get tgd:np/1xhb --pack "$PACK"
# endregion get

echo "=== json"
# region json
tgd get tgd:np/1xhb --json --pack "$PACK"
# endregion json

echo "=== reverse"
# region reverse
tgd reverse 27.7172 85.3240 -k 3 --pack "$PACK"
# endregion reverse

echo "=== near"
# region near
tgd near tgd:np/2cn -k 3 --pack "$PACK"
# endregion near

echo "=== admin"
# region admin
tgd admin 28.2096 83.9856 --pack "$PACK"
# endregion admin

echo "=== timezone"
# region timezone
tgd tz 27.7172 85.3240 --pack "$PACK"
# endregion timezone

echo "=== elevation"
# region elevation
tgd elevation 27.7172 85.3240 --pack "$PACK"
# endregion elevation

echo "=== geojson"
# region geojson
tgd search Pokhara -k 1 --geojson --pack "$PACK"
# endregion geojson

echo "=== nominatim"
# region nominatim
tgd reverse 28.2096 83.9856 -k 1 --nominatim --pack "$PACK"
# endregion nominatim

echo "=== extract"
# region extract
tgd extract "$PACK" --bbox 85.2,27.6,85.5,27.8 --out kathmandu.tgd
tgd search Kathmandu -k 1 --pack kathmandu.tgd
# endregion extract
rm -f kathmandu.tgd

# The HTTP server: started on a free port, asked with curl, stopped at the end.
LOG=$(mktemp)
"${TGD:-tgd}" serve --pack "$PACK" --listen 127.0.0.1:0 >"$LOG" 2>&1 &
SERVER=$!
trap 'kill "$SERVER" 2>/dev/null; rm -f "$LOG"' EXIT
URL=
for _ in $(seq 100); do
    URL=$(sed -n 's/^serving on //p' "$LOG")
    [ -n "$URL" ] && break
    sleep 0.1
done
[ -n "$URL" ] || { cat "$LOG" >&2; exit 1; }

echo "=== http-health"
# region http-health
curl -s "$URL/health"
# endregion http-health

echo "=== http-search"
# region http-search
curl -s "$URL/v1/search?q=Pokhara&limit=2"
# endregion http-search

echo "=== http-reverse"
# region http-reverse
curl -s "$URL/v1/reverse?lat=27.7172&lon=85.3240&limit=1"
# endregion http-reverse

echo "=== http-tz"
# region http-tz
curl -s "$URL/v1/tz?lat=27.7172&lon=85.3240&t=1700000000"
# endregion http-tz

echo "=== http-elevation"
# region http-elevation
curl -s "$URL/v1/elevation?lat=27.7172&lon=85.3240"
# endregion http-elevation

echo "=== http-distance"
# region http-distance
curl -s "$URL/v1/distance?from=27.7172,85.3240&to=tgd:np/1xhb"
# endregion http-distance

echo "=== http-geojson"
# region http-geojson
curl -s "$URL/v1/admin?lat=28.2096&lon=83.9856&format=geojson"
# endregion http-geojson

echo "=== http-nominatim"
# region http-nominatim
curl -s "$URL/search?q=Pokhara&format=jsonv2&limit=1"
# endregion http-nominatim

echo "=== http-error"
# region http-error
curl -s -w '%{http_code}\n' "$URL/v1/reverse?lat=91&lon=0"
# endregion http-error
