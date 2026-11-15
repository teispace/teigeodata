"""The tour: every query of the guide, run on a released pack.

Usage: python tour.py world-np-lite.tgd
"""

import json
import sys

# region import
import teigeodata
from teigeodata.blob import Reader
# endregion import


# region rows
def rows(blob):
    """Each row of a result as a dict: column name to value, None where null."""
    return Reader(blob).records()
# endregion rows


def main(pack):
    print("=== open")
    # region open
    engine = teigeodata.Engine()
    engine.open_file(pack)
    print("layers:", engine.layers())
    # endregion open

    print("=== search")
    # region search
    for p in rows(engine.search("Pokhara", 3)):
        print(f"{p['id']}  {p['name']}  {p['kind']}  {p['lat']:.4f}, {p['lon']:.4f}  score {p['score']}")
    # endregion search

    print("=== search-parent")
    # region search-parent
    for p in rows(engine.search("Kathmandu, Bagmati", 2)):
        print(f"{p['id']}  {p['name']}  {p['kind']} {p['level']}")
    # endregion search-parent

    print("=== search-focus")
    # region search-focus
    # Many places share a name; a focus (the user's location) ranks the near ones first.
    options = teigeodata.SearchOptions(focus_lat=28.7, focus_lon=80.6, focus_scale_m=30_000)
    for p in rows(engine.search_with("Shivapur", 3, options)):
        print(f"{p['id']}  {p['name']}  {p['lat']:.4f}, {p['lon']:.4f}")
    # endregion search-focus

    print("=== get")
    # region get
    (place,) = rows(engine.get("tgd:np/1xhb"))
    print(place["name"], place["kind"])
    print(f"at {place['lat']:.6f}, {place['lon']:.6f}, {place['elevation']} m")
    print("timezone", place["tz"], "parent", place["parent"])
    # endregion get

    print("=== hierarchy")
    # region hierarchy
    for p in rows(engine.ancestors("tgd:np/1xhb")):
        print(f"{p['kind']} {p['level']}  {p['name']}  {p['iso'] or ''}".rstrip())
    children = rows(engine.children("tgd:np/295c"))
    print("Kaski has", len(children), "children")
    # endregion hierarchy

    print("=== reverse")
    # region reverse
    for p in rows(engine.reverse(27.7172, 85.3240, 3)):
        print(f"{p['name']}  {p['meters']:.0f} m at {p['bearing']:.0f}°")
    # endregion reverse

    print("=== radius")
    # region radius
    within = rows(engine.within_radius(27.7172, 85.3240, 500.0, 10))
    print(len(within), "places within 500 m; farthest", f"{within[-1]['meters']:.0f} m")
    # endregion radius

    print("=== nearby")
    # region nearby
    for p in rows(engine.nearby("tgd:np/2cn", 3)):
        print(f"{p['name']}  {p['meters']:.0f} m")
    # endregion nearby

    print("=== admin")
    # region admin
    for p in rows(engine.admin_at(28.2096, 83.9856)):
        print(f"{p['level']}  {p['name']}  {p['iso'] or ''}".rstrip())
    (state,) = rows(engine.by_iso("NP-P4"))
    print("NP-P4 is", state["name"], state["id"])
    # endregion admin

    print("=== timezone")
    # region timezone
    (area,) = rows(engine.tz_at(27.7172, 85.3240))
    zone = area["tz"]
    (offset,) = rows(engine.tz_offset_at(zone, 1_700_000_000))
    print(zone, offset["seconds"], "s east of UTC,", offset["abbr"], "dst", offset["dst"])
    (local,) = rows(engine.tz_local("America/New_York", 2026, 3, 8, 2, 30, 0))
    print("02:30 on 2026-03-08 in New York:", local["kind"], local["before"], "->", local["after"])
    print("tzdb", engine.tz_version())
    # endregion timezone

    print("=== elevation")
    # region elevation
    (height,) = rows(engine.elevation_at(27.7172, 85.3240))
    print(f"{height['meters']:.1f} m above {height['datum']} ({height['source']}, {height['resolution_m']:.0f} m grid)")
    # endregion elevation

    print("=== distance")
    # region distance
    meters = teigeodata.distance_m(27.7172, 85.3240, 28.2096, 83.9856)
    heading = teigeodata.bearing_deg(27.7172, 85.3240, 28.2096, 83.9856)
    print(f"Kathmandu to Pokhara: {meters / 1000:.1f} km, heading {heading:.0f}°")
    (apart,) = rows(engine.distance("tgd:np/2cn", "tgd:np/1xhb"))
    print(f"place to place: {apart['meters'] / 1000:.1f} km at {apart['bearing']:.0f}°")
    # endregion distance

    print("=== boundary")
    # region boundary
    (shape,) = rows(engine.boundary("tgd:np/2gm", 0))
    feature = json.loads(shape["geojson"])  # a GeoJSON Feature, for any map library
    print(f"{feature['geometry']['type']} of {shape['vertices']} vertices at level {shape['lod']}")
    print(f"tolerance {shape['tolerance_m']:.0f} m, {len(shape['geojson'])} bytes of GeoJSON")
    # endregion boundary

    print("=== errors")
    # region errors
    try:
        engine.reverse(91.0, 0.0, 1)
    except teigeodata.InvalidArgumentError as error:
        print("invalid argument:", error)
    # endregion errors


if __name__ == "__main__":
    main(sys.argv[1])
