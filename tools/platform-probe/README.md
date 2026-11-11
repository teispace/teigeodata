# Platform prototypes (sync gate 14)

Prototypes of the platform side of an install (D-184, [research note](../../docs/02-research/notes/platform-transports.md)).
The installer runs in passes over the files a platform downloaded: `tgd install --downloads DIR`
installs from them or, with exit code 4, prints the byte ranges still needed as JSON. Each
prototype downloads those ranges with its platform's own service and runs install again.

- `serve.py`: a static range host on localhost (206, `ETag`, CORS, an optional 302 redirect
  like GitHub's release host, a JSON log of every request).
- `apple/`: URLSession download tasks, foreground or background (`run.sh ... --background
  --redirect`). Runs on macOS; the same code is the iOS prototype.

A repository to serve:

```sh
TGD_REPO_OUT=/tmp/repo cargo test -p teigeodata-cli --test sync -- --ignored write_repo
cargo build -p teigeodata-cli
tools/platform-probe/apple/run.sh /tmp/repo /tmp/apple --background --redirect
```

Results and dates are in the [phase-4 benches](../../docs/02-research/benches/phase-4.md#platform-prototypes-d-184).
