// Parity: the Swift binding gives the results bindings/parity.json expects (checked there
// against Rust). Run on macOS or Linux: swift run -c release Parity ../parity.json; on an iOS
// simulator: the ParityTests test (cargo xtask package-swift --test).
import Foundation
import TeiGeodata
#if canImport(Glibc)
import Glibc
/// Glibc imports `SOCK_STREAM` as an enum, Darwin as an `Int32`.
let streamSocket = Int32(SOCK_STREAM.rawValue)
#else
let streamSocket = SOCK_STREAM
#endif

func check(_ ok: Bool, _ what: @autoclosure () -> String) {
    if !ok {
        FileHandle.standardError.write(Data("swift parity failed: \(what())\n".utf8))
        exit(1)
    }
}

/// A host on localhost that answers every request with a body of a byte every 20 ms: a job
/// on it runs, its progress moving, until it is cancelled.
func trickle() -> String {
    // A write to a socket the client closed raises SIGPIPE; ignore it on every platform
    // (Linux has no `SO_NOSIGPIPE`), so the write fails instead.
    signal(SIGPIPE, SIG_IGN)
    let fd = socket(AF_INET, streamSocket, 0)
    var addr = sockaddr_in()
    addr.sin_family = sa_family_t(AF_INET)
    addr.sin_addr.s_addr = inet_addr("127.0.0.1")
    var len = socklen_t(MemoryLayout<sockaddr_in>.size)
    withUnsafeMutablePointer(to: &addr) {
        $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
            _ = bind(fd, $0, len)
            _ = getsockname(fd, $0, &len)
        }
    }
    listen(fd, 16)
    Thread.detachNewThread {
        while case let c = accept(fd, nil, nil), c >= 0 {
            Thread.detachNewThread {
                var request = [UInt8](repeating: 0, count: 4096)
                _ = read(c, &request, request.count)
                let head = Array("HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n".utf8)
                _ = head.withUnsafeBytes { write(c, $0.baseAddress, $0.count) }
                for _ in 0..<1000 {
                    var byte: UInt8 = 0x20
                    if write(c, &byte, 1) != 1 { break }
                    usleep(20_000)
                }
                close(c)
            }
        }
    }
    return "http://127.0.0.1:\(UInt16(bigEndian: addr.sin_port))/"
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

/// Runs every check of `bindings/parity.json` at `path`; the fixtures are found beside it. A
/// failed check prints what failed and exits with status 1.
public func runParity(_ path: String) async throws {
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
    // The zone rules (parity.json, tz) on the rules-only fixture.
    do {
        let t = p["tz"] as! [String: Any]
        let base = URL(fileURLWithPath: path).deletingLastPathComponent()
        let engine = Engine()
        _ = try engine.openFile(base.appendingPathComponent(t["fixture"] as! String).path)
        check(engine.tzVersion() == t["version"] as! String, "tz version")
        check(strings(try engine.tzZones(), "zone") == (t["zones"] as! [String]), "tz zones")
        let o = t["offset"] as! [String: Any]
        let got = try engine.tzOffsetAt(o["zone"] as! String, unixSeconds: Int64(o["unix"] as! Int))
        check(column(got, "seconds", as: Int32.self) == [Int32(o["seconds"] as! Int)], "tz offset")
        check(strings(got, "abbr") == [o["abbr"] as? String], "tz abbr")
        for kind in ["gap", "overlap"] {
            let c = t[kind] as! [String: Any], v = (c["civil"] as! [Int])
            let l = try engine.tzLocal(c["zone"] as! String, year: Int32(v[0]), month: UInt32(v[1]), day: UInt32(v[2]),
                                       hour: UInt32(v[3]), minute: UInt32(v[4]), second: UInt32(v[5]))
            check(strings(l, "kind") == [kind], "tz \(kind)")
            check(column(l, "before", as: Int32.self) + column(l, "after", as: Int32.self)
                  == [Int32(c["before"] as! Int), Int32(c["after"] as! Int)], "\(kind) offsets")
        }
        let st = t["start"] as! [String: Any]
        check(column(try engine.tzRulesStart(st["zone"] as! String), "unix", as: Int64.self) == [Int64(st["unix"] as! Int)], "tz start")
        check(strings(try engine.tzOffsetAt(t["unknown"] as! String, unixSeconds: 0), "abbr").isEmpty, "unknown zone")
        expect(.invalidArgument) { _ = try engine.tzLocal(o["zone"] as! String, year: 2024, month: 13, day: 1, hour: 0, minute: 0, second: 0) }
    }

    // The engine handle on the fixture pack (parity.json, "engine").
    let e = p["engine"] as! [String: Any]
    let dir = URL(fileURLWithPath: path).deletingLastPathComponent()
    do {
        let engine = Engine()
        check(try engine.openFile(dir.appendingPathComponent(e["fixture"] as! String).path) == 0, "open")
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
        let graph: [(String, (String) throws -> Data)] = [
            ("parent", { try engine.parent($0) }), ("ancestors", { try engine.ancestors($0) }), ("children", { try engine.children($0) }),
        ]
        for (method, call) in graph {
            let m = e[method] as! [String: Any]
            check(strings(try call(m["id"] as! String), "id") == (m["ids"] as! [String]).map { $0 }, method)
        }
        let nb = e["nearby"] as! [String: Any]
        check(strings(try engine.nearby(nb["id"] as! String, limit: UInt32(nb["limit"] as! Int)), "id") == (nb["ids"] as! [String]).map { $0 }, "nearby")
        let w = e["within_radius"] as! [String: Any]
        let (wl, wo) = (w["lat"] as! Double, w["lon"] as! Double)
        check(strings(try engine.withinRadius(wl, lon: wo, meters: w["meters"] as! Double, limit: UInt32(w["limit"] as! Int)), "id") == (w["ids"] as! [String]).map { $0 }, "within_radius")
        let tz = e["tz_at"] as! [String: Any]
        check(strings(try engine.tzAt(tz["lat"] as! Double, lon: tz["lon"] as! Double), "tz") == [tz["tz"] as? String], "tz_at")
        let iso = e["by_iso"] as! [String: Any]
        check(strings(try engine.byIso(iso["code"] as! String), "id") == [iso["id"] as? String], "by_iso")
        expect(.invalidArgument) { _ = try engine.byIso(e["bad_iso"] as! String) }
        expect(.invalidArgument) { _ = try engine.withinRadius(wl, lon: wo, meters: -1, limit: 1) }
        let strs = { (v: Any?) in (v as! [String]).map { Optional($0) } }
        let sw = e["search_with"] as! [String: Any]
        let focus = SearchOptions(focusLat: sw["focus_lat"] as? Double, focusLon: sw["focus_lon"] as? Double, focusScaleM: sw["focus_scale_m"] as? Double)
        let swLimit = UInt32(sw["limit"] as! Int)
        check(strings(try engine.searchWith(sw["query"] as! String, limit: swLimit, options: focus), "id") == strs(sw["ids"]), "search_with")
        check(strings(try engine.searchWith(sw["query"] as! String, limit: swLimit), "id") != strs(sw["ids"]), "search_with defaults")
        expect(.invalidArgument) { _ = try engine.searchWith("x", limit: 1, options: SearchOptions(focusLat: 1)) }
        let ow = e["open_with"] as! [String: Any]
        let fixture = dir.appendingPathComponent(e["fixture"] as! String).path
        var bad = try Data(contentsOf: URL(fileURLWithPath: fixture))
        bad[ow["corrupt_at"] as! Int] ^= 0xff
        let scratch = Engine()
        expect(.corrupt) { _ = try scratch.openBytesWith(bad, options: OpenOptions(verify: .eager)) }
        check(try scratch.openBytesWith(bad) == 0, "lazy")
        let every = OpenOptions(priority: 2, verify: .eager, load: .read, outlineLevel: 1)
        check(try scratch.openFileWith(fixture, options: every) == 1, "open_file_with")
        expect(.invalidArgument) { _ = try scratch.openFileWith(fixture, options: OpenOptions(outlineLevel: UInt32(ow["bad_outline_level"] as! Int))) }
        let points = Engine()
        _ = try points.openFileWith(fixture, options: OpenOptions(components: ow["components"] as! String, languages: ow["languages"] as! String))
        check(strings(try points.get((e["get"] as! [String: Any])["id"] as! String), "id").count == 1, "points get")
        expect(.missingComponent) { _ = try points.search("Synthland", limit: 1) }
        expect(.invalidArgument) { _ = try points.openFileWith(fixture, options: OpenOptions(components: ow["bad_components"] as! String)) }
        let pl = e["postal_lookup"] as! [String: Any]
        let hit = try engine.postalLookup(pl["code"] as! String, scheme: pl["scheme"] as! String)
        check(strings(hit, "postal") == strs(pl["postal"]) && strings(hit, "id") == strs(pl["ids"]), "postal_lookup")
        let pp = e["postal_prefix"] as! [String: Any]
        check(strings(try engine.postalPrefix(pp["input"] as! String, scheme: pp["scheme"] as! String, limit: UInt32(pp["limit"] as! Int)), "postal") == strs(pp["postal"]), "postal_prefix")
        let pa = e["postal_at"] as! [String: Any]
        let at = try engine.postalAt(pa["lat"] as! Double, lon: pa["lon"] as! Double)
        check(strings(at, "postal") == strs(pa["postal"]) && strings(at, "code_kind") == strs(pa["code_kind"]), "postal_at")
        let po = e["postal_codes_of"] as! [String: Any]
        check(strings(try engine.postalCodesOf(po["id"] as! String), "postal") == strs(po["postal"]), "postal_codes_of")
        expect(.invalidArgument) { _ = try engine.get(e["bad_id"] as! String) }
        expect(.io) { _ = try engine.openFile("/nonexistent.tgd") }
        expect(.corrupt) { _ = try engine.openBytes(Data("not a pack".utf8)) }
        check(engine.layers() == 1, "layers")
        let h = e["elevation_at"] as! [String: Any]
        check(try engine.openFile(dir.appendingPathComponent(h["fixture"] as! String).path) == 1, "open dem")
        let height = try engine.elevationAt(h["lat"] as! Double, lon: h["lon"] as! Double)
        check(column(height, "meters", as: UInt64.self).map { Double(bitPattern: $0) } == [h["meters"] as! Double], "meters")
        check(strings(height, "source") == [h["source"] as? String], "source")
        check(engine.remove(1) && !engine.remove(1) && engine.layers() == 1, "remove")
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
    // The store (parity.json, store): the fixture root, a mirror that refuses connections.
    do {
        let st = p["store"] as! [String: Any]
        let base = URL(fileURLWithPath: path).deletingLastPathComponent()
        let root = try Data(contentsOf: base.appendingPathComponent(st["root"] as! String))
        let mirror = st["mirror"] as! String, pack = st["pack"] as! String
        let d = FileManager.default.temporaryDirectory.appendingPathComponent("tg-store-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: d) }
        expect(.refused) { _ = try Store.open(d.appendingPathComponent("missing").path) }
        expect(.refused) { _ = try Store.create(d.appendingPathComponent("bad").path, repo: mirror, assets: mirror, trustedRoot: Data("{}".utf8)) }
        _ = try Store.create(d.appendingPathComponent("s").path, repo: mirror, assets: mirror, trustedRoot: root)
        let store = try Store.open(d.appendingPathComponent("s").path)
        check(store.damaged().isEmpty, "damaged")
        check(strings(try store.catalog(), "path").isEmpty, "empty catalog")
        check(strings(try store.status(false), "path").isEmpty, "empty status")
        let installed = try store.engine()
        check(installed.layers() == 0, "no layers")
        check(strings(try store.find(installed, query: pack, limit: 3), "name").isEmpty, "find nothing")
        check(strings(try store.locate(installed, lat: 27.7, lon: 85.3, radiusM: 1000), "name").isEmpty, "locate nothing")
        expect(.invalidArgument) { _ = try store.find(installed, query: pack, limit: 10_001) }
        expect(.invalidArgument) { _ = try store.locate(installed, lat: 91, lon: 0, radiusM: 1000) }
        expect(.invalidArgument) { _ = try store.install(pack, options: InstallOptions(components: st["bad_components"] as! String)) }
        expect(.refused) { _ = try store.remove(pack) }
        expect(.invalidArgument) { _ = try store.installPass(pack, dir: "") }
        expect(.network) { _ = try store.installPass(pack, dir: d.appendingPathComponent("dl").path).result() }
        let job = store.refresh(pack)
        expect(.network) { _ = try job.result() }
        check(job.isEnded(), "ended")
        check(job.waitProgress(job.seq(), timeoutMs: 60_000) == job.seq(), "wait on an ended job")
        check(job.doneBytes() == job.totalBytes(), "done at total")
        job.cancel()
        // The asynchronous forms run off the cooperative pool and carry the same error code.
        check(strings(try await store.statusAsync(false), "path").isEmpty, "status async")
        do {
            _ = try await store.refresh(pack).resultAsync()
            check(false, "result async: no error")
        } catch {
            check(error.code == .network, "result async: \(error)")
        }
        // Progress ends with the job, at its total.
        var last: (done: UInt64, total: UInt64)? = nil
        for await p in store.refresh(pack).progress { last = p }
        check(last.map { $0.done == $0.total } == true, "progress ends at the total: \(String(describing: last))")
        // On a host that trickles, progress moves, and cancelling the awaiting task cancels the job.
        let slow = trickle()
        let slowStore = try Store.create(d.appendingPathComponent("slow").path, repo: slow, assets: slow, trustedRoot: root)
        let slowJob = slowStore.refresh(pack)
        let waiting = Task { try await slowJob.resultAsync() }
        var moves = 0
        for await _ in slowJob.progress {
            moves += 1
            if moves == 3 { break }
        }
        check(moves == 3, "progress moves")
        let at = Date()
        waiting.cancel()
        do {
            _ = try await waiting.value
            check(false, "a cancelled task: no error")
        } catch {
            check((error as? TeiGeodataError)?.code == .cancelled, "a cancelled task: \(error)")
        }
        check(Date().timeIntervalSince(at) < 2, "the cancel ends the job at once")
        check(slowJob.isEnded(), "ended")
    }
}
