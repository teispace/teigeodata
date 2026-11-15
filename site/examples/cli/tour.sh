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
