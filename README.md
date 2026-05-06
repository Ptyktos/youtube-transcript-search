<div align="center">
  <img src="assets/logo.png" alt="YouTube Transcript MCP Logo" width="200"/>

  # YouTube Transcript MCP Server

  A Model Context Protocol (MCP) server that extracts transcripts from YouTube
  videos. Written in Rust. Runs as a native binary or as a Cloudflare Worker
  (via WebAssembly) — same logic, two deployment targets.
</div>

## Features

- **Single tool**: `get_transcript(url, language?)` — works with any YouTube
  URL format.
- **Three ways to call it**:
  - **MCP** over stdio (native) or JSON-RPC over HTTP / SSE (native + Worker)
  - **Raw HTTP**: `GET /transcript?url=…&language=…` returns plain text — curl-friendly
  - **CLI one-shot**: `youtube-transcript-mcp --url <URL>` prints to stdout and exits
- **Native binary**: stdio transport for Claude Desktop / local clients, plus
  an HTTP server with `/transcript`, `/mcp`, and `/sse`.
- **WASM Worker**: same Rust code, deployed to Cloudflare Workers. Exposes the
  same `/transcript`, `/mcp`, `/sse` endpoints.
- **Whisper fallback** (native only): when a video has no captions and
  `WHISPER_URL` is set, the server downloads the audio stream and transcribes
  it via a [faster-whisper-server](https://github.com/fedirz/faster-whisper-server)
  compatible endpoint.
- **URL flexibility**: `watch?v=`, `youtu.be/`, `/shorts/`, `/live/`, `/embed/`,
  international domains (`youtube.co.uk`, `youtube.de`, …), with or without scheme.
- **Language selection**: BCP-47 codes (`en`, `es`, `fr`, …) or `auto` (default).
  Falls back to English with an explanatory note when the requested language
  is unavailable.

## Repository layout

```
crates/
  core/      # pure transcript logic (URL parsing, Innertube/XML decoding, language selection)
  native/    # rmcp-based binary (stdio + HTTP/SSE)
  worker/    # Cloudflare Worker (compiled to wasm32-unknown-unknown via worker-build)
```

`crates/core` is I/O-free and is consumed by both the native binary and the
Worker. The Worker crate declares its own `[workspace]` so wrangler can build
it for `wasm32-unknown-unknown` without affecting the host workspace.

## Benchmarks

Measured on a single Linux x86_64 VM (Intel Xeon 8-core, AVX-512), apples-to-apples
through the same MCP-stdio harness, against the same canned Innertube + 80 KiB
caption XML fixtures. Full methodology, raw numbers and reproducibility scripts
in [BENCHMARKS.md](BENCHMARKS.md).

|                                     | this repo (Rust) | TS port (Node)¹ | jdepoix (Python lib)¹ | nabid-pf (Node) | anaisbetts (Node + yt-dlp)² | spinalshock (Go + yt-dlp)³ |
|-------------------------------------|---:|---:|---:|---:|---:|---:|
| **LAN p50 latency**                 | **1.86 ms** | 6.44 ms | 7.11 ms | 6.63 ms | 6.96 ms | 2821 ms |
| **LAN throughput (req/s)**          | **478** | 123 | 135 | 108 | 133 | 0.4 |
| **PROD-sim p50** (80 ms RTT)        | **164 ms** | — | — | 170 ms | 505 ms | ~2900 ms |
| **Cold start**                      | **9 ms** (2.6 ms raw) | 89 ms | 11 ms | 337 ms | 141 ms | 2510 ms |
| **Peak RSS**                        | **5.5 MiB** | 111 MiB | 30 MiB | 117 MiB | 73 MiB | 13 MiB |
| **Deployable artifact**             | **3.3 MiB binary** | 25 MiB node_modules | pip + Python | 25 MiB node_modules | npm + yt-dlp | Go binary + yt-dlp |
| **Runtime needed**                  | **none** | Node 18+ | Python 3.x | Node 18+ | Node + yt-dlp | Go binary + yt-dlp |

¹ Algorithm-equivalent reference, not an actual MCP server. Numbers from `bench/run_v2.py`.
² Measured with a fake yt-dlp shim returning canned subtitles instantly (best case for the wrapping MCP server). The PROD-sim row uses a realistic yt-dlp simulator with Python's ~240 ms startup tax + 3× RTT.
³ Spinalshock ships a `randomSleep(1500, 3000)` ms rate-limit before every request, which dominates every per-request number regardless of network or yt-dlp speed.

**Headline:** ~3.5× faster than other in-process implementations on CPU-bound work,
~270× faster than yt-dlp-based servers in production, ~13–20× less memory.

XML parsing alone is **19–20× faster** than `fast-xml-parser` (Node) and
`defusedxml` (Python) on the same 80 KiB transcript — `quick-xml` streams at
~409 MiB/s end-to-end vs ~21 MiB/s for either alternative.

## Self-hosting (native binary)

### From source

```bash
git clone https://github.com/twn-systems/youtube-transcript-mcp-rust
cd youtube-transcript-mcp-rust
cargo build --release -p youtube-transcript-mcp
# binary at target/release/youtube-transcript-mcp
```

### Run

```bash
# One-shot CLI — print transcript to stdout and exit
./target/release/youtube-transcript-mcp --url 'https://youtu.be/dQw4w9WgXcQ'
./target/release/youtube-transcript-mcp --url '…' --language es

# stdio MCP (Claude Desktop, local MCP clients)
./target/release/youtube-transcript-mcp --stdio

# HTTP server (raw API + MCP) — defaults to 127.0.0.1:3000
./target/release/youtube-transcript-mcp
./target/release/youtube-transcript-mcp --host 0.0.0.0 --port 8080
```

HTTP endpoints:

| Method | Path                                     | Purpose                                                   |
|--------|------------------------------------------|-----------------------------------------------------------|
| GET    | `/`                                      | Server info JSON                                          |
| GET    | `/transcript?url=…&language=…`           | Raw transcript as `text/plain`                            |
| POST   | `/mcp`                                   | MCP JSON-RPC (Streamable HTTP)                            |
| GET    | `/sse`                                   | SSE handshake                                             |
| POST   | `/sse`                                   | MCP JSON-RPC over SSE (single-shot)                       |

```bash
curl 'http://127.0.0.1:3000/transcript?url=https://youtu.be/dQw4w9WgXcQ&language=en'
```

### Claude Desktop (stdio)

Add to `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "youtube-transcript": {
      "command": "/absolute/path/to/youtube-transcript-mcp",
      "args": ["--stdio"]
    }
  }
}
```

### Whisper fallback (optional)

When a video has no captions, the native server can fall back to ASR.
Point it at any OpenAI-compatible `/v1/audio/transcriptions` endpoint (for
example, [faster-whisper-server](https://github.com/fedirz/faster-whisper-server)):

```bash
WHISPER_URL=http://localhost:8000 ./target/release/youtube-transcript-mcp --stdio
```

The transcript is prefixed with `[AI-generated transcript — no captions
available]`.

## Cloudflare Workers (WASM)

The Worker uses the [`worker`](https://crates.io/crates/worker) crate and is
compiled to WebAssembly by [`worker-build`](https://crates.io/crates/worker-build)
during `wrangler deploy`.

### Prerequisites

- Rust toolchain with `wasm32-unknown-unknown`:
  `rustup target add wasm32-unknown-unknown`
- [`wrangler`](https://developers.cloudflare.com/workers/wrangler/install-and-update/)
- A Cloudflare account

### Deploy

```bash
cd crates/worker
wrangler deploy
```

`wrangler dev` runs the Worker locally on `http://127.0.0.1:8787`.

The Worker exposes the same endpoints as the native HTTP server:

- `GET /` — server info JSON
- `GET /transcript?url=…&language=…` — raw transcript as `text/plain`
- `POST /mcp` — MCP JSON-RPC (Streamable HTTP)
- `GET /sse` / `POST /sse` — MCP JSON-RPC over Server-Sent Events

```bash
curl 'https://your-worker.workers.dev/transcript?url=https://youtu.be/dQw4w9WgXcQ'
```

The `[build]` section of `crates/worker/wrangler.toml` runs
`cargo install -q worker-build && worker-build --release` automatically.

## API reference

### `GET /transcript` (raw)

| Param      | In    | Required | Description                                              |
|------------|-------|----------|----------------------------------------------------------|
| `url`      | query | yes      | YouTube video URL in any supported format.               |
| `language` | query | no       | BCP-47 code (`en`, `es`, …). Defaults to `auto`.         |

Responds with `text/plain` on success. Status codes: `200`, `400` (invalid URL),
`404` (no transcript / unavailable language), `502` (network), `500` (parse).

### MCP tool `get_transcript`

Same parameters, returned as MCP tool content (`{"content":[{"type":"text", …}]}`).

**Direct MCP call against the Worker / HTTP server:**

```bash
curl -X POST https://your-worker.workers.dev/mcp \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "tools/call",
    "params": {
      "name": "get_transcript",
      "arguments": {
        "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        "language": "en"
      }
    }
  }'
```

### Supported URL formats

- `https://www.youtube.com/watch?v=VIDEO_ID`
- `https://youtu.be/VIDEO_ID`
- `https://m.youtube.com/watch?v=VIDEO_ID`
- `https://www.youtube.com/shorts/VIDEO_ID`
- `https://www.youtube.com/live/VIDEO_ID`
- `https://www.youtube.com/embed/VIDEO_ID`
- International TLDs (`youtube.co.uk`, `youtube.de`, …)

Tracking parameters (`si`, `t`, …) are ignored.

## Development

```bash
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo test --workspace

# Worker (separate workspace, wasm32 target)
cargo check -p youtube-transcript-mcp-worker \
  --manifest-path crates/worker/Cargo.toml \
  --target wasm32-unknown-unknown
```

CI runs the same commands on every push. Pre-built native binaries for Linux
and macOS are attached to GitHub Releases when a `v*` tag is pushed.

## License

[MIT](LICENSE)
