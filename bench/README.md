# bench/

Reproducibility scripts for [BENCHMARKS.md](../BENCHMARKS.md). The harness
runs apples-to-apples comparisons against a local mock so YouTube isn't
hit, and so other implementations are measured under their own best-case.

## Layout

```
bench/
├── canned/                  # canned fixtures (Innertube JSON, caption XML, VTT)
├── mock/server.py           # Python mock with MOCK_DELAY_MS support
├── fake-yt-dlp/yt-dlp       # bash shim — instant or realistic-yt-dlp mode
├── mcp-client/drive.js      # generic MCP-stdio bench driver (Node)
├── rust-client/             # this repo's core via reqwest, exposes a `serve` MCP-stdio mode
├── ts-client/               # algorithm-equivalent TS port (uses fast-xml-parser)
├── python-jdepoix/main.py   # bench harness for jdepoix/youtube-transcript-api 1.2.4
├── run_v2.py                # microbench Rust ↔ TS ↔ Python (3-way)
├── run_full.py              # cross-impl MCP-stdio bench (LAN + PROD-sim profiles)
└── results.json             # last full-run outputs
```

## Setup (one-time)

```bash
# Rust client
( cd bench/rust-client && cargo build --release )

# TS client
( cd bench/ts-client && npm install && npx tsc -p . )

# Python (jdepoix)
pip install --break-system-packages youtube-transcript-api

# Optional: third-party MCPs that we benchmark
( cd /tmp && npm pack @anaisbetts/mcp-youtube && tar -xzf *.tgz -C /tmp/anaisbetts/ )
( cd /tmp && npm pack youtube-video-summarizer-mcp && tar -xzf *.tgz -C /tmp/nabid/ )
( cd /tmp && git clone https://github.com/spinalshock/youtube-transcript-mcp \
         && cd youtube-transcript-mcp && go build -o spinalshock-mcp . )

# Then patch youtube-caption-extractor's URL constant for the nabid-pf case:
sed -i "s|https://www.youtube.com/youtubei/v1|http://127.0.0.1:18080/youtubei/v1|g" \
    /tmp/nabid/package/node_modules/youtube-caption-extractor/dist/index.js
```

## Running

```bash
# 3-way microbench: Rust core vs TS port vs jdepoix Python
python3 bench/run_v2.py
# → bench/results/report_v2.md

# Cross-impl MCP-stdio benchmark, LAN + PROD-sim profiles
python3 bench/run_full.py
# → bench/results.json
```

## Mock profiles

| `MOCK_DELAY_MS` | Effective profile                                                    |
|----------------:|----------------------------------------------------------------------|
| 0               | LAN — loopback, no network. Measures CPU-bound cost.                 |
| 80              | PROD-sim — 80 ms per response, ~typical YouTube edge RTT.            |

The mock applies the delay to every response, so each transcript fetch pays
it twice (Innertube POST + caption GET). The fake yt-dlp shim *also* honours
`MOCK_DELAY_MS`: when non-zero, it sleeps `0.24 + 3·delay/1000` s per call to
mimic real yt-dlp's Python startup + 3 internal HTTP round-trips.

## Results so far

See [../BENCHMARKS.md](../BENCHMARKS.md). Highlights:

- **CPU-bound (LAN):** Rust 1.86 ms p50 vs 6–7 ms for the other in-process
  implementations; ~270× faster p50 than yt-dlp-based servers.
- **Production-sim (80 ms RTT):** Rust 164 ms vs 505 ms for yt-dlp wrappers
  vs ~2900 ms for spinalshock's rate-limited server.
- **Memory:** 5.5 MiB peak RSS vs 73–117 MiB for Node-based servers.
