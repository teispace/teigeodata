// The binding benchmark's Swift rows (docs/02-research/benches/s4-bindings.md).
// Run: swift run -c release Bench
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

var sink = 0.0
func row(_ c: String, _ ns: Double) {
    print("swift".padding(toLength: 10, withPad: " ", startingAt: 0), c.padding(toLength: 22, withPad: " ", startingAt: 0), String(format: "%10.1f ns", ns))
}
row("noop", best(10_000_000) { TeiGeodata.noop() })
row("fold", best(1_000_000) { sink += Double(TeiGeodata.fold("Kathmandu").utf8.count) })
row("sample_rows(1000)+sum", best(10_000) { sink += latSum(try! TeiGeodata.sampleRows(1000)) })
row("callback", best(1_000) { sink += Double(try! TeiGeodata.progress(1000) { _ in true }) } / 1000)
if sink == 42 { print() }
