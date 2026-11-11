# Platform prototypes (sync gate 14)

Prototypes of the platform side of an install (D-184, [research note](../../docs/02-research/notes/platform-transports.md)).
The installer runs in passes over the files a platform downloaded: `tgd install --downloads DIR`
installs from them or, with exit code 4, prints the byte ranges still needed as JSON. Each
prototype downloads those ranges with its platform's own service and runs install again.

- `serve.py`: a static range host on localhost (206, `ETag`, CORS, an optional 302 redirect
  like GitHub's release host, a JSON log of every request).
- `apple/`: URLSession download tasks over one shared probe (`ProbeCore.swift`).
  `run.sh ... --background --redirect` runs it on macOS; `run-ios.sh REPO WORK --redirect`
  builds the iOS app (`ios/`, no Xcode project), creates a simulator of its own, kills the app
  mid-download, and checks that iOS relaunches it to file the range; the simulator is deleted
  at the end.
- `android/`: a user-initiated data transfer job, the APK built without Gradle.
  `run-android.sh REPO WORK --redirect` creates an emulator of its own (API 35), kills the app's
  process mid-download and checks that the system restarts the job; the AVD is deleted at the end.
- `web/`: a worker fetching ranges into OPFS. `run-web.sh REPO WORK --redirect` runs headless
  Chrome on a profile of its own, the page and the mirror on two origins, kills the browser
  mid-download and checks that the next session resumes from OPFS.

A repository to serve:

```sh
TGD_REPO_OUT=/tmp/repo cargo test -p teigeodata-cli --test sync -- --ignored write_repo
cargo build -p teigeodata-cli
tools/platform-probe/apple/run.sh /tmp/repo /tmp/apple --background --redirect
```

Results and dates are in the [phase-4 benches](../../docs/02-research/benches/phase-4.md#platform-prototypes-d-184).
