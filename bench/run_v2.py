#!/usr/bin/env python3
"""3-way bench: this repo's Rust core vs TS port vs jdepoix/youtube-transcript-api."""
import json
import os
import resource
import statistics as stats
import subprocess
import time
import urllib.request
from pathlib import Path

ROOT = Path("/tmp/bench")
RESULTS = ROOT / "results"
RESULTS.mkdir(exist_ok=True)

RUST_BIN = ROOT / "rust-client" / "target" / "release" / "rust-bench-client"
TS_ENTRY = ROOT / "ts-client" / "dist" / "main.js"
PY_ENTRY = ROOT / "python-jdepoix" / "main.py"

MOCK_PORT = 18080
MOCK_URL = f"http://127.0.0.1:{MOCK_PORT}"
ENV = {**os.environ, "MOCK_URL": MOCK_URL, "NODE_NO_WARNINGS": "1"}


def start_mock():
    p = subprocess.Popen(["python3", str(ROOT / "mock" / "server.py"), str(MOCK_PORT)],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(50):
        try:
            with urllib.request.urlopen(f"{MOCK_URL}/healthz", timeout=0.1) as r:
                if r.read() == b"ok":
                    return p
        except Exception:
            time.sleep(0.05)
    p.terminate()
    raise RuntimeError("mock did not come up")


def stop_mock(p):
    if p.poll() is None:
        p.terminate()
        try:
            p.wait(timeout=2)
        except subprocess.TimeoutExpired:
            p.kill()


def run_cmd(cmd):
    proc = subprocess.run(cmd, env=ENV, capture_output=True, text=True)
    if proc.returncode != 0:
        raise RuntimeError(f"{cmd} failed: {proc.stderr or proc.stdout}")
    line = next((ln for ln in proc.stdout.splitlines() if ln.startswith("{")), None)
    if not line:
        raise RuntimeError(f"no JSON output from {cmd}: {proc.stdout}")
    return json.loads(line)


def run_rust(mode, iters=None):
    return run_cmd([str(RUST_BIN), mode] + ([str(iters)] if iters is not None else []))

def run_ts(mode, iters=None):
    return run_cmd(["node", str(TS_ENTRY), mode] + ([str(iters)] if iters is not None else []))

def run_py(mode, iters=None):
    return run_cmd(["python3", str(PY_ENTRY), mode] + ([str(iters)] if iters is not None else []))


def percentile(xs, p):
    if not xs: return float("nan")
    xs = sorted(xs)
    k = (len(xs) - 1) * p / 100
    f, c = int(k), min(int(k) + 1, len(xs) - 1)
    return xs[f] + (xs[c] - xs[f]) * (k - f)


def fmt_us(ms): return f"{ms*1000:.1f} µs"


REPORT = []

def line(s): REPORT.append(s)


def main():
    if not (RUST_BIN.exists() and TS_ENTRY.exists() and PY_ENTRY.exists()):
        raise SystemExit("build all clients first")

    p = start_mock()
    try:
        line("# 3-way benchmark — this repo (Rust) vs TS port vs jdepoix/youtube-transcript-api")
        line("")
        line("All three implementations hit the same local mock that serves canned Innertube + 80 KiB caption-XML responses. The Python client patches `WATCH_URL` and `INNERTUBE_API_URL` in `_settings`/`_transcripts` to point at the mock; the mock serves a stub `<script>var ytcfg = {\"INNERTUBE_API_KEY\":\"stub-key-123\"}</script>` watch page so the library's HTML scrape step succeeds.")
        line("")
        line("Versions: Rust release build · Node " + subprocess.check_output(['node','--version']).decode().strip() + " · Python " + subprocess.check_output(['python3','-c','import sys;print(sys.version.split()[0])']).decode().strip() + " · `youtube-transcript-api` " + subprocess.check_output(['pip','show','youtube-transcript-api']).decode().split('Version: ')[1].split('\n')[0])
        line("")

        # Cold start ────────────────────────────────────────────────────────
        line("## Cold start (process spawn → first transcript byte)")
        line("")
        cr = [run_rust("cold")["ms"] for _ in range(5)]
        ct = [run_ts("cold")["ms"]   for _ in range(5)]
        cp = [run_py("cold")["ms"]   for _ in range(5)]
        line("| Run | Rust (this repo) | TS port | Python (jdepoix) |")
        line("|---|---:|---:|---:|")
        for i, (r, t, p_) in enumerate(zip(cr, ct, cp)):
            line(f"| {i+1} | {r:.2f} ms | {t:.2f} ms | {p_:.2f} ms |")
        rm, tm, pm = stats.median(cr), stats.median(ct), stats.median(cp)
        line(f"| **median** | **{rm:.2f}** | **{tm:.2f}** | **{pm:.2f}** |")
        line(f"| **vs Rust** | 1× | {tm/rm:.1f}× slower | {pm/rm:.1f}× slower |")
        line("")

        # Microbench: XML parse ─────────────────────────────────────────────
        line("## Microbench — `parse_transcript_xml` (1000-segment, 80 KiB)")
        line("")
        ITERS = 5000
        pr = run_rust("parse", ITERS)
        pt = run_ts("parse", ITERS)
        pp = run_py("parse", ITERS)
        bytes_in = pr["bytesIn"]
        line("| Implementation | Total | Per iter | Throughput |")
        line("|---|---:|---:|---:|")
        line(f"| Rust (`quick-xml`) | {pr['ms']:.0f} ms | {fmt_us(pr['ms']/ITERS)} | {(bytes_in*ITERS)/(pr['ms']/1000)/(1024*1024):.1f} MiB/s |")
        line(f"| TS (`fast-xml-parser`) | {pt['ms']:.0f} ms | {fmt_us(pt['ms']/ITERS)} | {(bytes_in*ITERS)/(pt['ms']/1000)/(1024*1024):.1f} MiB/s |")
        line(f"| Python (jdepoix `_TranscriptParser`, defusedxml) | {pp['ms']:.0f} ms | {fmt_us(pp['ms']/ITERS)} | {(bytes_in*ITERS)/(pp['ms']/1000)/(1024*1024):.1f} MiB/s |")
        line(f"| **vs Rust** | 1× | — | Rust is {pt['ms']/pr['ms']:.1f}× faster than TS, {pp['ms']/pr['ms']:.1f}× faster than Python |")
        line("")

        # JSON parse ────────────────────────────────────────────────────────
        line("## Microbench — Innertube JSON parse")
        line("")
        pr = run_rust("parse-json", ITERS)
        pt = run_ts("parse-json", ITERS)
        pp = run_py("parse-json", ITERS)
        line("| Implementation | Total | Per iter |")
        line("|---|---:|---:|")
        line(f"| Rust (`serde_json`) | {pr['ms']:.0f} ms | {fmt_us(pr['ms']/ITERS)} |")
        line(f"| TS (V8 `JSON.parse`) | {pt['ms']:.0f} ms | {fmt_us(pt['ms']/ITERS)} |")
        line(f"| Python (`json` stdlib) | {pp['ms']:.0f} ms | {fmt_us(pp['ms']/ITERS)} |")
        line("")

        # E2E ────────────────────────────────────────────────────────────────
        line("## End-to-end — 100 sequential `getTranscript()` calls")
        line("")
        E2E = 100
        er = run_rust("e2e", E2E)
        et = run_ts("e2e", E2E)
        ep = run_py("e2e", E2E)
        rs, ts, ps = er["samples"], et["samples"], ep["samples"]
        line("| Metric | Rust (this repo) | TS port | Python (jdepoix) |")
        line("|---|---:|---:|---:|")
        line(f"| Sum of {E2E} samples | {sum(rs):.0f} ms | {sum(ts):.0f} ms | {sum(ps):.0f} ms |")
        line(f"| Throughput | {E2E*1000/sum(rs):.0f} req/s | {E2E*1000/sum(ts):.0f} req/s | {E2E*1000/sum(ps):.0f} req/s |")
        line(f"| p50 latency | {percentile(rs,50):.2f} ms | {percentile(ts,50):.2f} ms | {percentile(ps,50):.2f} ms |")
        line(f"| p95 latency | {percentile(rs,95):.2f} ms | {percentile(ts,95):.2f} ms | {percentile(ps,95):.2f} ms |")
        line(f"| p99 latency | {percentile(rs,99):.2f} ms | {percentile(ts,99):.2f} ms | {percentile(ps,99):.2f} ms |")
        line("")

        # Memory ────────────────────────────────────────────────────────────
        line("## Memory — peak RSS (`VmHWM`) after 100 e2e requests, settled")
        line("")
        mr = run_rust("memory", 100)["rssKib"]
        mt = run_ts("memory", 100)["rssKib"]
        mp = run_py("memory", 100)["rssKib"]
        line("| Implementation | Peak RSS | vs Rust |")
        line("|---|---:|---:|")
        line(f"| Rust | {mr/1024:.1f} MiB | 1× |")
        line(f"| TS (Node 22) | {mt/1024:.1f} MiB | {mt/mr:.1f}× more |")
        line(f"| Python (jdepoix) | {mp/1024:.1f} MiB | {mp/mr:.1f}× more |")
        line("")

    finally:
        stop_mock(p)

    out = RESULTS / "report_v2.md"
    out.write_text("\n".join(REPORT))
    print(f"=> {out}")
    print("\n".join(REPORT))


if __name__ == "__main__":
    main()
