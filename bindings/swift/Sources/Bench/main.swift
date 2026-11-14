// The binding benchmark's Swift rows (docs/02-research/benches/s4-bindings.md).
// Run: swift run -c release Bench. With TEIGEODATA_BENCH_PACK naming a pack, the fifth case
// runs the 1,000-row query on it.
import Foundation
import TeiGeodata

func best(_ n: Int, _ f: () -> Void) -> Double {
    var out = Double.infinity
    for _ in 0..<5 {
        let t = DispatchTime.now().uptimeNanoseconds
        for _ in 0..<n { f() }
        out = min(out, Double(DispatchTime.now().uptimeNanoseconds - t) / Double(n))
    }
    return out
}

func latSum(_ blob: Data) -> Double {
    blob.withUnsafeBytes { raw in
        let at = Int(raw.loadUnaligned(fromByteOffset: 76, as: UInt32.self))
        let len = Int(raw.loadUnaligned(fromByteOffset: 80, as: UInt32.self))
        var s = 0.0
        for i in stride(from: 0, to: len, by: 8) { s += raw.loadUnaligned(fromByteOffset: at + i, as: Double.self) }
        return s
    }
}

/// The 1,000-row query's decoding: every `name` as a Swift `String`, and the sum of `lat`.
func decode(_ blob: Data) -> Double {
    blob.withUnsafeBytes { raw -> Double in
        let u32 = { (at: Int) in Int(UInt32(littleEndian: raw.loadUnaligned(fromByteOffset: at, as: UInt32.self))) }
        func descriptor(_ name: String) -> Int {
            for i in 0..<u32(8) where String(decoding: raw[16 + 40 * i..<32 + 40 * i].prefix { $0 != 0 }, as: UTF8.self) == name {
                return 32 + 40 * i
            }
            return -1
        }
        let rows = u32(4), names = descriptor("name"), lat = descriptor("lat")
        guard names >= 0, lat >= 0 else { return .nan }
        let (offsets, text, at) = (u32(names + 4), u32(names + 12), u32(lat + 4))
        var total = 0.0
        for r in 0..<rows {
            let s = String(decoding: raw[text + u32(offsets + 4 * r)..<text + u32(offsets + 4 * r + 4)], as: UTF8.self)
            total += Double(s.utf8.count)
        }
        for r in 0..<rows { total += raw.loadUnaligned(fromByteOffset: at + 8 * r, as: Double.self) }
        return total
    }
}

var sink = 0.0
func row(_ c: String, _ ns: Double) {
    print("swift".padding(toLength: 10, withPad: " ", startingAt: 0), c.padding(toLength: 22, withPad: " ", startingAt: 0), String(format: "%10.1f ns", ns))
}
row("noop", best(10_000_000) { TeiGeodata.noop() })
row("fold", best(1_000_000) { sink += Double(try! TeiGeodata.fold("Kathmandu").utf8.count) })
row("sample_rows(1000)+sum", best(10_000) { sink += latSum(try! TeiGeodata.sampleRows(1000)) })
row("callback", best(1_000) { sink += Double(try! TeiGeodata.progress(1000) { _ in true }) } / 1000)
if let pack = ProcessInfo.processInfo.environment["TEIGEODATA_BENCH_PACK"] {
    let engine = Engine()
    _ = try engine.openFile(pack)
    let query = { try! engine.withinRadius(27.7, lon: 85.3, meters: 50_000, limit: 1000) }
    if query().withUnsafeBytes({ $0.loadUnaligned(fromByteOffset: 4, as: UInt32.self) }) == 1000 {
        row("radius(1000)+decode", best(200) { sink += decode(query()) })
    } else {
        print("swift: \(pack): the query does not return 1,000 rows")
    }
}
if sink == 42 { print() }
