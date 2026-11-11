// The iOS app over ProbeCore (D-184): a background URLSession, run in the simulator by
// `run-ios.sh`. Files live in the app's own container, as in a real app: `Documents/` holds the
// listing the script copies in, `results.jsonl`, and `dl/<asset>/<start>-<end>` for the script
// to copy out.
//
// Every launch recreates the background session, as a real app must: when a task ends while
// the app is dead, iOS relaunches it in the background to deliver the events, with no launch
// arguments. `start <pending.json>` also starts a task per range. Results are appended to
// `results.jsonl` as JSON lines; `{"event": ...}` lines record each launch (and whether the
// system or a user made it) and when no task is left.

import UIKit

let sessionID = "org.teigeodata.probe"
let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]

@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    var probe: Probe!
    var session: URLSession!
    var results: FileHandle?

    func log(_ line: String) {
        results?.seekToEndOfFile()
        results?.write((line + "\n").data(using: .utf8)!)
        try? results?.synchronize()
    }

    func application(_ application: UIApplication,
                     didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
        let args = ProcessInfo.processInfo.arguments
        let out = documents.appendingPathComponent("results.jsonl").path
        if !FileManager.default.fileExists(atPath: out) {
            FileManager.default.createFile(atPath: out, contents: nil)
        }
        results = FileHandle(forWritingAtPath: out)
        probe = Probe { [weak self] in self?.log($0) }
        probe.directory = { w in
            documents.appendingPathComponent("dl")
                .appendingPathComponent(URL(fileURLWithPath: w.dir).lastPathComponent)
        }
        probe.finished = { [weak self] in
            self?.log("{\"event\":\"finished\"}")
            // Events of tasks that ended while the app was dead arrive in a burst: let them land.
            DispatchQueue.main.asyncAfter(deadline: .now() + 2) { exit(0) }
        }
        let state = application.applicationState == .background ? "background" : "foreground"
        log("{\"event\":\"launched\",\"state\":\"\(state)\",\"start\":\(args.count >= 3 && args[1] == "start")}")
        let config = URLSessionConfiguration.background(withIdentifier: sessionID)
        config.sessionSendsLaunchEvents = true
        let queue = OperationQueue()
        queue.maxConcurrentOperationCount = 1
        session = URLSession(configuration: config, delegate: probe, delegateQueue: queue)
        if args.count >= 3 && args[1] == "start" {
            let wants = (try? loadWants(documents.appendingPathComponent(args[2]).path)) ?? []
            queue.addOperation {
                self.probe.start(wants, on: self.session)
                self.log("{\"event\":\"started\",\"tasks\":\(wants.count)}")
            }
        } else {
            probe.reconnect(session) { running in
                self.log("{\"event\":\"reconnected\",\"running\":\(running)}")
            }
        }
        return true
    }

    func application(_ application: UIApplication, configurationForConnecting session: UISceneSession,
                     options: UIScene.ConnectionOptions) -> UISceneConfiguration {
        let c = UISceneConfiguration(name: nil, sessionRole: session.role)
        c.delegateClass = SceneDelegate.self
        return c
    }

    func application(_ application: UIApplication, handleEventsForBackgroundURLSession identifier: String,
                     completionHandler: @escaping () -> Void) {
        probe?.eventsDone = completionHandler
    }
}

final class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(_ scene: UIScene, willConnectTo session: UISceneSession,
               options connectionOptions: UIScene.ConnectionOptions) {
        guard let scene = scene as? UIWindowScene else { return }
        window = UIWindow(windowScene: scene)
        window?.rootViewController = UIViewController()
        window?.makeKeyAndVisible()
    }
}
