// The Apple prototype for sync gate 14 (D-184), shared by the macOS command line (`cli/`) and
// the iOS app (`ios/`): URLSession download tasks fetch the byte ranges that
// `tgd install --downloads` lists and file them as `<dir>/<start>-<end>` for the next pass.
//
// Each response is held to its request: status 206 and a `Content-Range` naming exactly the
// range and the asset's length, and a file of exactly the range's length. One that is not is
// reported, not filed, so a platform that drops `Range` (or loses it across a redirect) shows.
//
// Each task carries its range in `taskDescription`, which the system keeps for a background
// session's tasks, so an app killed mid-download and relaunched still files what finished.

import Foundation

struct Want: Codable {
    let url: String
    let start: UInt64
    let end: UInt64  // exclusive
    let length: UInt64
    let dir: String
    let file: String
}

/// The ranges of a `tgd install --downloads` listing.
func loadWants(_ path: String) throws -> [Want] {
    let data = try Data(contentsOf: URL(fileURLWithPath: path))
    guard let assets = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] else {
        throw NSError(domain: "probe", code: 1, userInfo: [NSLocalizedDescriptionKey: "not a list"])
    }
    var out: [Want] = []
    for a in assets {
        guard let urls = a["urls"] as? [String], let url = urls.first,
              let length = (a["length"] as? NSNumber)?.uint64Value,
              let dir = a["dir"] as? String, let ranges = a["ranges"] as? [[String: Any]]
        else { continue }
        for r in ranges {
            guard let s = (r["start"] as? NSNumber)?.uint64Value,
                  let e = (r["end"] as? NSNumber)?.uint64Value,
                  let f = r["file"] as? String else { continue }
            out.append(Want(url: url, start: s, end: e, length: length, dir: dir, file: f))
        }
    }
    return out
}

final class Probe: NSObject, URLSessionDownloadDelegate {
    /// Each result as one JSON line.
    let sink: (String) -> Void
    /// Called once no task is left, from the session's queue.
    var finished: () -> Void = {}
    /// The system's completion handler after a background relaunch (iOS).
    var eventsDone: (() -> Void)?
    /// Where a range's file goes: its `dir` by default; an iOS app maps it into its container.
    var directory: (Want) -> URL = { URL(fileURLWithPath: $0.dir) }
    private(set) var failed = 0
    private var started: [Int: Date] = [:]
    private var left = 0

    init(sink: @escaping (String) -> Void) {
        self.sink = sink
    }

    /// Starts one task per range on `session`.
    func start(_ wants: [Want], on session: URLSession) {
        left += wants.count
        for w in wants {
            guard let url = URL(string: w.url) else { continue }
            var req = URLRequest(url: url)
            req.setValue("bytes=\(w.start)-\(w.end - 1)", forHTTPHeaderField: "Range")
            let task = session.downloadTask(with: req)
            task.taskDescription = String(data: try! JSONEncoder().encode(w), encoding: .utf8)
            started[task.taskIdentifier] = Date()
            task.resume()
        }
        if wants.isEmpty { finished() }
    }

    /// After a relaunch: the tasks the session still runs count as left.
    func reconnect(_ session: URLSession, then: @escaping (Int) -> Void) {
        session.getAllTasks { tasks in
            let running = tasks.filter { $0.state == .running || $0.state == .suspended }
            self.left += running.count
            then(running.count)
        }
    }

    private func want(_ task: URLSessionTask) -> Want? {
        task.taskDescription.flatMap { $0.data(using: .utf8) }
            .flatMap { try? JSONDecoder().decode(Want.self, from: $0) }
    }

    private func report(_ w: Want?, _ status: Int, _ cr: String?, _ ok: Bool, _ ms: Int?) {
        let line: [String: Any] = ["file": w?.file ?? NSNull(), "status": status,
                                   "contentRange": cr ?? NSNull(), "ok": ok, "ms": ms ?? NSNull()]
        let d = try! JSONSerialization.data(withJSONObject: line, options: [.sortedKeys])
        sink(String(data: d, encoding: .utf8)!)
        if !ok { failed += 1 }
    }

    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask,
                    didFinishDownloadingTo location: URL) {
        let w = want(downloadTask)
        let ms = started[downloadTask.taskIdentifier].map { Int(Date().timeIntervalSince($0) * 1000) }
        let http = downloadTask.response as? HTTPURLResponse
        let status = http?.statusCode ?? 0
        let cr = http?.value(forHTTPHeaderField: "Content-Range")
        guard let w else {
            report(nil, status, cr, false, ms)
            return
        }
        var ok = status == 206 && cr == "bytes \(w.start)-\(w.end - 1)/\(w.length)"
        if ok {
            // The file must be moved before this method returns (Apple's contract).
            let size = (try? FileManager.default.attributesOfItem(atPath: location.path)[.size]
                as? NSNumber)?.uint64Value ?? 0
            ok = size == w.end - w.start
            if ok {
                let dir = directory(w)
                let to = dir.appendingPathComponent(w.file)
                do {
                    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                    try? FileManager.default.removeItem(at: to)
                    try FileManager.default.moveItem(at: location, to: to)
                } catch {
                    ok = false
                }
            }
        }
        report(w, status, cr, ok, ms)
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        if let error {
            FileHandle.standardError.write("\(want(task)?.file ?? "?"): \(error.localizedDescription)\n"
                .data(using: .utf8)!)
            report(want(task), 0, nil, false, nil)
        }
        left -= 1
        if left <= 0 { finished() }
    }

    func urlSessionDidFinishEvents(forBackgroundURLSession session: URLSession) {
        DispatchQueue.main.async {
            self.eventsDone?()
            self.eventsDone = nil
        }
    }
}
