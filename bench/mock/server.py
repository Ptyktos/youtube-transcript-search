#!/usr/bin/env python3
"""Mock YouTube/Innertube server for apples-to-apples benchmarking.

Routes:
  POST /youtubei/v1/player  → canned innertube.json
  GET  /caption[?...]       → canned caption.xml
  GET  /watch[?v=…]         → canned watch HTML (with INNERTUBE_API_KEY)
  GET  /healthz             → 200 ok

Set MOCK_DELAY_MS=80 to simulate production round-trip latency to YouTube
edge (typical ~50-150 ms). Both endpoints respect the delay so each fetch
pays it twice (Innertube POST + caption GET), which mirrors real prod cost.

Single-threaded HTTPServer is fine for sequential benches; for concurrent
benches we run multiple client workers against it.
"""
import json
import os
import sys
import time
from http.server import BaseHTTPRequestHandler, HTTPServer

CANNED_DIR = "/tmp/bench/canned"
with open(f"{CANNED_DIR}/innertube.json", "rb") as f:
    INNERTUBE = f.read()
with open(f"{CANNED_DIR}/caption.xml", "rb") as f:
    CAPTION = f.read()

DELAY_MS = int(os.environ.get("MOCK_DELAY_MS", "0"))


class Handler(BaseHTTPRequestHandler):
    # Silence per-request logs; we time externally.
    def log_message(self, *a, **kw):
        pass

    def _ok(self, body: bytes, ctype: str):
        if DELAY_MS > 0:
            time.sleep(DELAY_MS / 1000.0)
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path.startswith("/caption") or self.path.startswith("/api/timedtext"):
            return self._ok(CAPTION, "text/xml; charset=utf-8")
        if self.path.startswith("/watch"):
            # jdepoix's library scrapes the watch page for INNERTUBE_API_KEY.
            html = b'<html><script>var ytcfg = {"INNERTUBE_API_KEY":"stub-key-123"}</script></html>'
            return self._ok(html, "text/html; charset=utf-8")
        if self.path == "/healthz":
            return self._ok(b"ok", "text/plain")
        self.send_error(404)

    def do_POST(self):
        if self.path.startswith("/youtubei/v1/player"):
            length = int(self.headers.get("Content-Length", "0") or 0)
            _ = self.rfile.read(length)  # discard body
            return self._ok(INNERTUBE, "application/json")
        self.send_error(404)


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 18080
    HTTPServer(("127.0.0.1", port), Handler).serve_forever()
