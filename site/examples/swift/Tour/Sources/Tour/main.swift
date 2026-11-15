// The tour: every query of the guide, run on a released pack.
//
// Usage: swift run -c release Tour world-np-lite.tgd (in site/examples/swift/Tour)

// #region import
import Foundation
import TeiGeodata
// #endregion import

/// `value` with `digits` decimals, as the other languages print it.
func fixed(_ value: Double?, _ digits: Int) -> String {
    String(format: "%.\(digits)f", value ?? .nan)
}

let pack = CommandLine.arguments[1]

do {
    print("=== open")
    // #region open
    let engine = Engine()
    _ = try engine.openFile(pack)  // the layer number
    print("layers:", engine.layers())
    // #endregion open

    print("=== search")
    // #region search
    for p in try Reader(engine.search("Pokhara", limit: 3)) {
        print("\(p["id"]!)  \(p["name"]!)  \(p["kind"]!)  \(fixed(p.double("lat"), 4)), \(fixed(p.double("lon"), 4))  score \(p["score"]!)")
    }
    // #endregion search

    print("=== search-parent")
    // #region search-parent
    for p in try Reader(engine.search("Kathmandu, Bagmati", limit: 2)) {
        print("\(p["id"]!)  \(p["name"]!)  \(p["kind"]!) \(p["level"]!)")
    }
    // #endregion search-parent

    print("=== search-focus")
    // #region search-focus
    // Many places share a name; a focus (the user's location) ranks the near ones first.
    let options = SearchOptions(focusLat: 28.7, focusLon: 80.6, focusScaleM: 30_000)
    for p in try Reader(engine.searchWith("Shivapur", limit: 3, options: options)) {
        print("\(p["id"]!)  \(p["name"]!)  \(fixed(p.double("lat"), 4)), \(fixed(p.double("lon"), 4))")
    }
    // #endregion search-focus

    print("=== get")
    // #region get
    let place = try Reader(engine.get("tgd:np/1xhb")).records()[0]
    print(place["name"]!, place["kind"]!)
    print("at \(fixed(place.double("lat"), 6)), \(fixed(place.double("lon"), 6)), \(place["elevation"]!) m")
    print("timezone", place["tz"]!, "parent", place["parent"]!)
    // #endregion get

    print("=== hierarchy")
    // #region hierarchy
    for p in try Reader(engine.ancestors("tgd:np/1xhb")) {
        let line = "\(p["kind"]!) \(p["level"]!)  \(p["name"]!)  \(p.string("iso") ?? "")"
        print(line.replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression))
    }
    let children = try Reader(engine.children("tgd:np/295c"))
    print("Kaski has", children.rows, "children")
    // #endregion hierarchy

    print("=== reverse")
    // #region reverse
    for p in try Reader(engine.reverse(27.7172, lon: 85.3240, limit: 3)) {
        print("\(p["name"]!)  \(fixed(p.double("meters"), 0)) m at \(fixed(p.double("bearing"), 0))°")
    }
    // #endregion reverse

    print("=== radius")
    // #region radius
    let within = try Reader(engine.withinRadius(27.7172, lon: 85.3240, meters: 500, limit: 10)).records()
    print(within.count, "places within 500 m; farthest", fixed(within.last?.double("meters"), 0), "m")
    // #endregion radius

    print("=== nearby")
    // #region nearby
    for p in try Reader(engine.nearby("tgd:np/2cn", limit: 3)) {
        print("\(p["name"]!)  \(fixed(p.double("meters"), 0)) m")
    }
    // #endregion nearby

    print("=== admin")
    // #region admin
    for p in try Reader(engine.adminAt(28.2096, lon: 83.9856)) {
        let line = "\(p["level"]!)  \(p["name"]!)  \(p.string("iso") ?? "")"
        print(line.replacingOccurrences(of: "\\s+$", with: "", options: .regularExpression))
    }
    let state = try Reader(engine.byIso("NP-P4")).records()[0]
    print("NP-P4 is", state["name"]!, state["id"]!)
    // #endregion admin

    print("=== timezone")
    // #region timezone
    let zone = try Reader(engine.tzAt(27.7172, lon: 85.3240)).records()[0].string("tz")!
    let offset = try Reader(engine.tzOffsetAt(zone, unixSeconds: 1_700_000_000)).records()[0]
    print(zone, offset["seconds"]!, "s east of UTC,", offset["abbr"]!, "dst", offset["dst"]!)
    let local = try Reader(engine.tzLocal("America/New_York", year: 2026, month: 3, day: 8, hour: 2, minute: 30, second: 0)).records()[0]
    print("02:30 on 2026-03-08 in New York:", local["kind"]!, local["before"]!, "->", local["after"]!)
    print("tzdb", engine.tzVersion())
    // #endregion timezone

    print("=== elevation")
    // #region elevation
    let height = try Reader(engine.elevationAt(27.7172, lon: 85.3240)).records()[0]
    print("\(fixed(height.double("meters"), 1)) m above \(height["datum"]!) (\(height["source"]!), \(fixed(height.double("resolution_m"), 0)) m grid)")
    // #endregion elevation

    print("=== distance")
    // #region distance
    let meters = try TeiGeodata.distanceM(27.7172, lon1: 85.3240, lat2: 28.2096, lon2: 83.9856)
    let heading = try TeiGeodata.bearingDeg(27.7172, lon1: 85.3240, lat2: 28.2096, lon2: 83.9856)
    print("Kathmandu to Pokhara: \(fixed(meters / 1000, 1)) km, heading \(fixed(heading, 0))°")
    let apart = try Reader(engine.distance("tgd:np/2cn", to: "tgd:np/1xhb")).records()[0]
    print("place to place: \(fixed(apart.double("meters")! / 1000, 1)) km at \(fixed(apart.double("bearing"), 0))°")
    // #endregion distance

    print("=== boundary")
    // #region boundary
    let shape = try Reader(engine.boundary("tgd:np/2gm", lod: 0)).records()[0]
    let geojson = shape["geojson"]!.description // a GeoJSON Feature, for any map library
    let feature = try JSONSerialization.jsonObject(with: Data(geojson.utf8)) as! [String: Any]
    let type = (feature["geometry"] as! [String: Any])["type"] as! String
    print("\(type) of \(shape["vertices"]!) vertices at level \(shape["lod"]!)")
    print("tolerance \(fixed(shape.double("tolerance_m"), 0)) m, \(geojson.utf8.count) bytes of GeoJSON")
    // #endregion boundary

    print("=== errors")
    // #region errors
    do {
        _ = try engine.reverse(91, lon: 0, limit: 1)
    } catch where error.code == .invalidArgument {
        print("invalid argument:", error.message)
    }
    // #endregion errors
} catch {
    FileHandle.standardError.write(Data("\(error)\n".utf8))
    exit(1)
}
