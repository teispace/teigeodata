// The macOS command line over ProbeCore (D-184).
//
// Usage: probe <pending.json> [--background]
// Prints one JSON line per range; exits 0 when every range was filed, 1 otherwise.

import Foundation

let args = CommandLine.arguments
guard args.count >= 2 else {
    FileHandle.standardError.write("usage: probe <pending.json> [--background]\n".data(using: .utf8)!)
    exit(2)
}
let wants = try loadWants(args[1])
let done = DispatchSemaphore(value: 0)
let probe = Probe { print($0) }
probe.finished = { done.signal() }
let config: URLSessionConfiguration = args.contains("--background")
    ? .background(withIdentifier: "org.teigeodata.probe.\(ProcessInfo.processInfo.processIdentifier)")
    : .default
let queue = OperationQueue()
queue.maxConcurrentOperationCount = 1
let session = URLSession(configuration: config, delegate: probe, delegateQueue: queue)
queue.addOperation { probe.start(wants, on: session) }
if done.wait(timeout: .now() + 300) == .timedOut {
    FileHandle.standardError.write("timed out\n".data(using: .utf8)!)
    exit(1)
}
session.finishTasksAndInvalidate()
exit(probe.failed == 0 ? 0 : 1)
