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
- **Native binary**: stdio transport for Claude Desktop / local clients, plus
  HTTP+SSE transport for remote clients.
- **WASM Worker**: deploy the same Rust code to Cloudflare Workers as
  WebAssembly. JSON-RPC over `/mcp` and SSE over `/sse`.
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
# stdio (Claude Desktop, local MCP clients)
./target/release/youtube-transcript-mcp --stdio

# HTTP + SSE (remote clients) — defaults to 127.0.0.1:3000
./target/release/youtube-transcript-mcp
./target/release/youtube-transcript-mcp --host 0.0.0.0 --port 8080
```

Endpoints (HTTP/SSE mode):

- `GET /sse` — SSE stream for the MCP transport
- `POST /message` — JSON-RPC messages

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

The Worker exposes:

- `GET /` — server info JSON
- `POST /mcp` — JSON-RPC (single-shot HTTP)
- `GET /sse` and `POST /sse` — JSON-RPC over Server-Sent Events

The `[build]` section of `crates/worker/wrangler.toml` runs
`cargo install -q worker-build && worker-build --release` automatically.

## Tool reference

### `get_transcript`

| Param      | Type     | Required | Description                                                      |
|------------|----------|----------|------------------------------------------------------------------|
| `url`      | string   | yes      | YouTube video URL in any supported format.                       |
| `language` | string   | no       | BCP-47 code (`en`, `es`, …). Defaults to `auto`.                 |

**Direct call against the Worker / HTTP server:**

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
