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

/// A fixed-width column of a v0 blob, read in place.
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
print("swift parity ok")
