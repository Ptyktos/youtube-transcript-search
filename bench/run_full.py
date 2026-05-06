#!/usr/bin/env python3
"""Multi-implementation benchmark, 2 network profiles."""
import json
import os
import signal
import statistics as stats
import subprocess
import time
from pathlib import Path

PORT = 18080
DRIVE = "/tmp/bench/mcp-client/drive.js"
RUST_BIN = "/tmp/bench/rust-client/target/release/rust-bench-client"
ANAIS = "/tmp/anaisbetts/package/dist/index.js"
NABID = "/tmp/nabid/package/dist/index.js"
URL = "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
FAKE_PATH = "/tmp/bench/fake-yt-dlp:" + os.environ.get("PATH", "")


def start_mock(delay_ms: int):
    subprocess.run(["pkill", "-f", f"server.py {PORT}"], capture_output=True)
    time.sleep(0.8)
    env = {**os.environ, "MOCK_DELAY_MS": str(delay_ms)}
    p = subprocess.Popen(
        ["python3", "/tmp/bench/mock/server.py", str(PORT)],
        env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    time.sleep(0.5)
    return p


def stop_mock(p):
    p.terminate()
    try: p.wait(timeout=2)
    except subprocess.TimeoutExpired: p.kill()


def percentile(xs, q):
    xs = sorted(xs)
    if not xs: return float("nan")
    k = (len(xs) - 1) * q / 100
    f, c = int(k), min(int(k) + 1, len(xs) - 1)
    return xs[f] + (xs[c] - xs[f]) * (k - f)


def run_drive(mode, iters, tool, child_argv, arg_name="url", env=None):
    cmd = ["node", DRIVE, mode, str(iters), tool, URL]
    if arg_name != "url":
        cmd.extend(["--arg-name", arg_name])
    cmd.append("--")
    cmd.extend(child_argv)
    proc = subprocess.run(cmd, capture_output=True, text=True, env=env or os.environ)
    if proc.returncode != 0:
        raise RuntimeError(f"drive failed: {proc.stderr or proc.stdout}")
    return json.loads(next(ln for ln in proc.stdout.splitlines() if ln.startswith("{")))


def bench_one(label, tool, child_argv, arg_name="url", env=None):
    colds = []
    for _ in range(3):
        r = run_drive("cold", 1, tool, child_argv, arg_name, env)
        if r.get("error"):
            raise RuntimeError(f"{label} returned isError:true (response: {r['len']} bytes)")
        colds.append(r["ms"])
    cold_med = stats.median(colds)

    e2e = run_drive("e2e", 50, tool, child_argv, arg_name, env)
    samples = e2e["samples"]
    sum_ms = sum(samples)
    tput = 50000 / sum_ms if sum_ms > 0 else 0

    mem = run_drive("memory", 50, tool, child_argv, arg_name, env)
    rss = mem["rssKib"]

    return {
        "label": label,
        "cold_ms": cold_med,
        "sum_ms": sum_ms,
        "tput": tput,
        "p50": percentile(samples, 50),
        "p95": percentile(samples, 95),
        "p99": percentile(samples, 99),
        "rss_kib": rss,
    }


def main():
    profiles = [
        ("LAN (mock returns instantly)", 0),
        ("PROD-sim (80 ms RTT per response — typical YouTube edge)", 80),
    ]

    rust_env = os.environ.copy()
    # fake_env gets PATH and MOCK_DELAY_MS so the fake yt-dlp shim can simulate
    # real yt-dlp under the PROD profile.
    def fake_env_for(delay):
        return {**os.environ, "PATH": FAKE_PATH, "MOCK_DELAY_MS": str(delay)}

    out = {"profiles": []}
    for prof_label, delay in profiles:
        SPINAL = "/tmp/youtube-transcript-mcp/spinalshock-mcp"
        impls = [
            ("this repo — Rust, MCP stdio",                                     "get_transcript",                      [RUST_BIN, "serve"], "url",      rust_env),
            ("anaisbetts/mcp-youtube (Node + simulated yt-dlp)",                "download_youtube_url",                ["node", ANAIS],     "url",      fake_env_for(delay)),
            ("spinalshock (Go + simulated yt-dlp)",                             "get_transcript",                      [SPINAL],            "url",      fake_env_for(delay)),
            ("nabid-pf summarizer (Node + youtube-caption-extractor)",          "get-video-info-for-summary-from-url", ["node", NABID],     "videoUrl", rust_env),
        ]
        p = start_mock(delay)
        try:
            results = []
            for label, tool, argv, an, env in impls:
                try:
                    r = bench_one(label, tool, argv, an, env)
                    results.append(r)
                    print(f"  {label:60s}  cold={r['cold_ms']:.1f}ms  p50={r['p50']:.2f}ms  p99={r['p99']:.2f}ms  tput={r['tput']:.0f}/s  rss={r['rss_kib']/1024:.1f}MiB")
                except Exception as e:
                    results.append({"label": label, "error": str(e)})
                    print(f"  {label}: ERROR {e}")
            out["profiles"].append({"profile": prof_label, "delay_ms": delay, "rows": results})
        finally:
            stop_mock(p)

    Path("/tmp/bench/results/full.json").write_text(json.dumps(out, indent=2))
    print("\nwrote /tmp/bench/results/full.json")


if __name__ == "__main__":
    main()
