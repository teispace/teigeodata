#!/usr/bin/env python3
"""A static host for the platform prototypes (D-184): serves a directory over HTTP on localhost.

- GET with `Range: bytes=a-b` answers 206 with `Content-Range`; without one, 200.
- `ETag` per file; `Accept-Ranges: bytes`.
- CORS for a browser (Q-014): `Range` allowed, `Content-Range` and `ETag` exposed.
- `--redirect`: `/assets/<f>` answers 302 to `/blob/<f>`, as GitHub's release host redirects to
  its storage, so a prototype shows whether its platform keeps `Range` across a redirect.
- Every request is logged as one JSON line to `--log`: method, path, range, status, bytes.
- `--rate N`: bodies are sent at about N bytes a second, so a download is still running when a
  prototype suspends or kills its app.

Standard library only. Usage: serve.py DIR [--port N] [--redirect] [--log FILE] [--rate N]
"""

import argparse
import hashlib
import http.server
import json
import os
import re
import sys
import time

RANGE = re.compile(r"^bytes=(\d+)-(\d*)$")


def make_handler(root, redirect, log, rate):
    class Handler(http.server.BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *args):
            pass

        def record(self, status, nbytes, rng):
            entry = {"method": self.command, "path": self.path, "range": rng,
                     "status": status, "bytes": nbytes}
            with open(log, "a", encoding="utf-8") as f:
                f.write(json.dumps(entry) + "\n")

        def cors(self):
            self.send_header("Access-Control-Allow-Origin", "*")
            self.send_header("Access-Control-Allow-Headers", "Range")
            self.send_header("Access-Control-Expose-Headers", "Content-Range, ETag, Content-Length")

        def do_OPTIONS(self):
            self.send_response(204)
            self.cors()
            self.send_header("Content-Length", "0")
            self.end_headers()
            self.record(204, 0, None)

        def do_GET(self):
            rng = self.headers.get("Range")
            path = self.path.split("?", 1)[0]
            if redirect and path.startswith("/assets/"):
                self.send_response(302)
                self.send_header("Location", "/blob/" + path[len("/assets/"):])
                self.cors()
                self.send_header("Content-Length", "0")
                self.end_headers()
                self.record(302, 0, rng)
                return
            if path.startswith("/blob/"):
                path = "/assets/" + path[len("/blob/"):]
            full = os.path.realpath(os.path.join(root, path.lstrip("/")))
            if not full.startswith(os.path.realpath(root) + os.sep) or not os.path.isfile(full):
                self.send_response(404)
                self.cors()
                self.send_header("Content-Length", "0")
                self.end_headers()
                self.record(404, 0, rng)
                return
            with open(full, "rb") as f:
                data = f.read()
            etag = '"' + hashlib.sha256(data).hexdigest()[:16] + '"'
            m = RANGE.match(rng or "")
            if m:
                start = int(m.group(1))
                end = min(int(m.group(2)) if m.group(2) else len(data) - 1, len(data) - 1)
                if start > end:
                    self.send_response(416)
                    self.send_header("Content-Range", f"bytes */{len(data)}")
                    self.cors()
                    self.send_header("Content-Length", "0")
                    self.end_headers()
                    self.record(416, 0, rng)
                    return
                body = data[start:end + 1]
                self.send_response(206)
                self.send_header("Content-Range", f"bytes {start}-{end}/{len(data)}")
                status = 206
            else:
                body = data
                self.send_response(200)
                status = 200
            self.send_header("Content-Length", str(len(body)))
            self.send_header("ETag", etag)
            self.send_header("Accept-Ranges", "bytes")
            self.send_header("Content-Type", "application/octet-stream")
            self.cors()
            self.end_headers()
            try:
                if rate:
                    step = max(1, rate // 20)
                    for i in range(0, len(body), step):
                        self.wfile.write(body[i:i + step])
                        time.sleep(0.05)
                else:
                    self.wfile.write(body)
            except (BrokenPipeError, ConnectionResetError):
                self.record(-1, 0, rng)
                return
            self.record(status, len(body), rng)

    return Handler


def main():
    p = argparse.ArgumentParser()
    p.add_argument("root")
    p.add_argument("--port", type=int, default=0)
    p.add_argument("--redirect", action="store_true")
    p.add_argument("--log", default=os.devnull)
    p.add_argument("--rate", type=int, default=0)
    a = p.parse_args()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", a.port),
                                             make_handler(a.root, a.redirect, a.log, a.rate))
    print(f"http://127.0.0.1:{server.server_address[1]}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        sys.exit(0)


if __name__ == "__main__":
    main()
