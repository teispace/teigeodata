// Parity: the Swift binding gives the results bindings/parity.json expects (checked there
// against Rust). Run: swift run -c release Parity ../parity.json
import Foundation
import TeiGeodata

func check(_ ok: Bool, _ what: @autoclosure () -> String) {
    if !ok {
        FileHandle.standardError.write(Data("swift parity failed: \(what())\n".utf8))
        exit(1)
    }
}

/// A fixed-width column of a v1 blob, read in place.
func column<T: FixedWidthInteger>(_ blob: Data, _ name: String, as _: T.Type) -> [T] {
    blob.withUnsafeBytes { raw -> [T] in
        let u32 = { (at: Int) in UInt32(littleEndian: raw.loadUnaligned(fromByteOffset: at, as: UInt32.self)) }
        for i in 0..<Int(u32(8)) {
            let d = 16 + 40 * i
            let n = String(decoding: raw[d..<d + 16].prefix { $0 != 0 }, as: UTF8.self)
            if n == name {
                let at = Int(u32(d + 20)), len = Int(u32(d + 24))
                return (0..<len / MemoryLayout<T>.size).map {
                    T(littleEndian: raw.loadUnaligned(fromByteOffset: at + $0 * MemoryLayout<T>.size, as: T.self))
                }
            }
        }
        check(false, "no column \(name)")
        return []
    }
}

/// A string column (UTF-8 or a dictionary) of a v1 blob, null rows as nil.
func strings(_ blob: Data, _ name: String) -> [String?] {
    blob.withUnsafeBytes { raw -> [String?] in
        let u32 = { (at: Int) in Int(UInt32(littleEndian: raw.loadUnaligned(fromByteOffset: at, as: UInt32.self))) }
        let text = { (at: Int, end: Int) in String(decoding: raw[at..<end], as: UTF8.self) }
        let rows = u32(4)
        for i in 0..<u32(8) {
            let d = 16 + 40 * i
            guard String(decoding: raw[d..<d + 16].prefix { $0 != 0 }, as: UTF8.self) == name else { continue }
            let type = u32(d + 16), at = u32(d + 20), aux = u32(d + 28), nulls = u32(d + 36)
            return (0..<rows).map { r -> String? in
                if nulls != 0 && (raw[nulls + r >> 3] >> (r & 7)) & 1 == 0 { return nil }
                if type == 7 { return text(aux + u32(at + 4 * r), aux + u32(at + 4 * r + 4)) }
                let code = type == 11
                    ? Int(UInt16(littleEndian: raw.loadUnaligned(fromByteOffset: at + 2 * r, as: UInt16.self)))
                    : u32(at + 4 * r)
                let base = aux + 8 + 4 * u32(aux)
                return text(base + u32(aux + 4 + 4 * code), base + u32(aux + 8 + 4 * code))
            }
        }
        check(false, "no column \(name)")
        return []
    }
}

func expect(_ code: TeiGeodataError.Code, _ call: () throws -> Void) {
    do {
        try call()
        check(false, "no error, expected \(code)")
    } catch {
        check((error as? TeiGeodataError)?.code == code, "\(error), expected \(code)")
    }
}

let path = CommandLine.arguments.dropFirst().first ?? "../parity.json"
let p = try JSONSerialization.jsonObject(with: Data(contentsOf: URL(fileURLWithPath: path))) as! [String: Any]

check(TeiGeodata.version() == p["version"] as! String, "version")
TeiGeodata.noop()
for case let pair as [String] in p["fold"] as! [Any] {
    check(TeiGeodata.fold(pair[0]) == pair[1], "fold \(pair[0])")
}
let s = p["sample_rows"] as! [String: Any]
let blob = try TeiGeodata.sampleRows(UInt32(s["rows"] as! Int))
check(blob.count == s["bytes"] as! Int, "blob length \(blob.count)")
let lat = column(blob, "lat", as: UInt64.self).reduce(0.0) { $0 + Double(bitPattern: $1) }
check(lat == s["lat_sum"] as! Double, "lat sum \(lat)")
let elevation = column(blob, "elevation", as: UInt32.self).reduce(0.0) { $0 + Double(Float(bitPattern: $1)) }
check(elevation == s["elevation_sum"] as! Double, "elevation sum \(elevation)")
check(Int(TeiGeodata.sampleRowsMax) == p["sample_rows_max"] as! Int, "max")

do {
    _ = try TeiGeodata.sampleRows(TeiGeodata.sampleRowsMax + 1)
    check(false, "no error over the maximum")
} catch {
    check(error.code == .invalidArgument && error.message.contains("at most"), "\(error)")
}

let dm = p["distance_m"] as! [String: Any]
let da = dm["args"] as! [Double]
let d = try TeiGeodata.distanceM(da[0], lon1: da[1], lat2: da[2], lon2: da[3])
check(abs(d - (dm["metres"] as! Double)) < 1e-6, "distance \(d)")
let bad = dm["out_of_range"] as! [Double]
do {
    _ = try TeiGeodata.distanceM(bad[0], lon1: bad[1], lat2: bad[2], lon2: bad[3])
    check(false, "no range error")
} catch {
    check(error.code == .invalidArgument, "\(error)")
}

var seen: [UInt32] = []
check(try TeiGeodata.progress(3) { seen.append($0); return true } == 3 && seen == [1, 2, 3], "progress")
let g = p["progress"] as! [String: Any]
let after = UInt32(g["cancel_after"] as! Int)
do {
    _ = try TeiGeodata.progress(UInt32(g["steps"] as! Int)) { $0 < after }
    check(false, "no cancellation")
} catch {
    check(error.code == .cancelled && error.message == g["cancelled_message"] as! String, "\(error)")
}
// The engine handle on the fixture pack (parity.json, "engine").
let e = p["engine"] as! [String: Any]
let dir = URL(fileURLWithPath: path).deletingLastPathComponent()
do {
    let engine = Engine()
    check(try engine.openFile(dir.appendingPathComponent(e["fixture"] as! String).path) == 1, "open")
    let get = e["get"] as! [String: Any]
    let row = try engine.get(get["id"] as! String)
    check(strings(row, "name") == [get["name"] as? String], "get name")
    check(strings(row, "kind") == [get["kind"] as? String], "get kind")
    check(column(row, "lat", as: UInt64.self).map { Double(bitPattern: $0) } == [get["lat"] as! Double], "get lat")
    let q = e["search"] as! [String: Any]
    let hits = try engine.search(q["query"] as! String, limit: UInt32(q["limit"] as! Int))
    check(strings(hits, "id").first == q["first_id"] as? String, "search")
    let r = e["reverse"] as! [String: Any]
    let near = try engine.reverse(r["lat"] as! Double, lon: r["lon"] as! Double, limit: UInt32(r["limit"] as! Int))
    check(strings(near, "id") == (r["ids"] as! [String]).map { $0 }, "reverse \(strings(near, "id"))")
    let m = Double(bitPattern: column(near, "meters", as: UInt64.self)[0])
    check(abs(m - (r["first_meters"] as! Double)) < 0.1, "meters \(m)")
    let a = e["admin_at"] as! [String: Any]
    let units = try engine.adminAt(a["lat"] as! Double, lon: a["lon"] as! Double)
    check(strings(units, "id") == (a["ids"] as! [String]).map { $0 }, "admin_at")
    expect(.invalidArgument) { _ = try engine.get(e["bad_id"] as! String) }
    expect(.io) { _ = try engine.openFile("/nonexistent.tgd") }
    expect(.corrupt) { _ = try engine.openBytes(Data("not a pack".utf8)) }
    check(engine.layers() == 1, "layers")
    // Sendable: four tasks query one engine at once.
    let id = get["id"] as! String
    let names = await withTaskGroup(of: [String?].self) { group in
        for _ in 0..<4 { group.addTask { (try? strings(engine.get(id), "name")) ?? [] } }
        var all: [String?] = []
        for await names in group { all += names }
        return all
    }
    check(names == Array(repeating: get["name"] as? String, count: 4), "tasks \(names)")
}
print("swift parity ok")
