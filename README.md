<div align="center">
  <img src="assets/logo.png" alt="YouTube Transcript MCP Logo" width="200"/>
  
  # YouTube Transcript Remote MCP Server

  The **first remote** Model Context Protocol (MCP) server that enables Claude AI to extract transcripts from YouTube videos. This server offers zero-setup access for users on any platform including mobile devices.
</div>

[![Deploy to Cloudflare Workers](https://deploy.workers.cloudflare.com/button)](https://deploy.workers.cloudflare.com/?url=https://github.com/ergut/youtube-transcript-mcp)

## 🌟 Features

- **Two deployment targets**: Native binary (stdio + HTTP/SSE for local MCP clients) and Cloudflare Worker (remote MCP)
- **Multi-language Support**: BCP-47 language selection with automatic fallback to the first available track
- **URL Flexibility**: Handles every documented YouTube URL form (`watch`, `youtu.be`, `shorts`, `live`, `embed`, mobile, international TLDs); tracking parameters stripped automatically
- **Whisper fallback** *(native only)*: When `WHISPER_URL` is set, videos with no captions fall back to a faster-whisper-server instance
- **Linear-time XML parser**: ~150–200 MB/s on real caption tracks (see [Performance](#-performance))

## 🚀 Quick Start

### Option 1: Use Our Hosted Server (Recommended)

The easiest way to get started - just add our public server to your Claude Desktop:

#### For Claude Desktop Users

1. **Open Claude Desktop Settings**
   - Click on "Claude" in the menu bar → "Settings"
   - Navigate to "Developer" tab
   - Click "Edit Config"

2. **Add the MCP Server Configuration**
   
   Add this to your `claude_desktop_config.json`:

   ```json
   {
     "mcpServers": {
       "youtube-transcript": {
         "command": "npx",
         "args": [
           "mcp-remote",
           "https://youtube-transcript-mcp.ergut.workers.dev/sse"
         ]
       }
     }
   }
   ```

3. **Restart Claude Desktop**
   
   After saving the config file, restart Claude Desktop to load the server.

4. **Verify Installation**
   
   Look for the tools icon (🔧) in the chat interface. You should see the `get_transcript` tool available.

### Option 2: Deploy Your Own

Want to run your own instance? Deploy to Cloudflare Workers in one click:

[![Deploy to Cloudflare Workers](https://deploy.workers.cloudflare.com/button)](https://deploy.workers.cloudflare.com/?url=https://github.com/ergut/youtube-transcript-mcp)

Or manually:

```bash
git clone https://github.com/ergut/youtube-transcript-mcp
cd youtube-transcript-mcp
npm install
npm run deploy
```

### For Other MCP Clients

The server supports the standard MCP protocol and can be used with any compatible client:

- **Public Server URL**: `https://youtube-transcript-mcp.ergut.workers.dev/sse`
- **Transport**: Server-Sent Events (SSE) or HTTP
- **Authentication**: None required (public server)

## 📖 Usage Examples

### Basic Transcript Extraction

```
Extract the transcript from this YouTube video: https://www.youtube.com/watch?v=dQw4w9WgXcQ
```

### Multi-language Support

```
Can you get the transcript of this video in Turkish: https://youtu.be/VIDEO_ID
```

```
Extract the Spanish transcript from: https://www.youtube.com/watch?v=VIDEO_ID
```

### Supported URL Formats

The server automatically handles all YouTube URL formats:

- `https://www.youtube.com/watch?v=VIDEO_ID`
- `https://youtu.be/VIDEO_ID`
- `https://m.youtube.com/watch?v=VIDEO_ID`
- `https://www.youtube.com/live/VIDEO_ID`
- `https://www.youtube.com/embed/VIDEO_ID`
- `https://www.youtube.com/shorts/VIDEO_ID`
- International domains (`youtube.co.uk`, `youtube.de`, etc.)

All tracking parameters (like `?si=`, `&t=`, etc.) are automatically removed.

## 🛠 Available Tools

### `get_transcript`

Extracts the transcript from a YouTube video URL.

**Parameters:**
- `url` (required): YouTube video URL in any format
- `language` (optional): Language code for the transcript (e.g., 'en', 'es', 'fr'). Defaults to 'en'.

**Example Usage:**
```json
{
  "name": "get_transcript",
  "arguments": {
    "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
    "language": "en"
  }
}
```

## 🔧 Advanced Configuration

### Debug Logging

To enable detailed logging for troubleshooting:

```json
{
  "mcpServers": {
    "youtube-transcript": {
      "command": "npx",
      "args": [
        "mcp-remote",
        "https://youtube-transcript-mcp.ergut.workers.dev/sse",
        "--debug"
      ]
    }
  }
}
```

Debug logs will be created in `~/.mcp-auth/{server_hash}_debug.log`.

### HTTP Transport Only

To force HTTP transport instead of SSE:

```json
{
  "mcpServers": {
    "youtube-transcript": {
      "command": "npx",
      "args": [
        "mcp-remote",
        "https://youtube-transcript-mcp.ergut.workers.dev/mcp",
        "--transport",
        "http-only"
      ]
    }
  }
}
```

## 📊 Server Information

- **Hosting**: Cloudflare Workers (worker crate) or self-hosted (native crate)
- **Uptime**: Inherits the underlying platform's availability — Cloudflare's global network for the worker, your host for the native binary
- **Response Time**: Measured 1.4–2.0 s end-to-end for cold requests across short and long-form videos (see [Performance](#-performance) for the methodology and raw numbers)

> ⚠️ **Known gaps relative to the upstream TypeScript project**
>
> The Rust rewrite does **not yet** implement the following features the upstream README advertised:
>
> - **No KV caching.** Every request re-fetches Innertube + caption XML from YouTube. The worker also sends `Cache-Control: no-cache` on its responses.
> - **No retry / exponential backoff.** First network failure propagates to the caller. The recommended client-side workaround is to retry idempotent calls with a small delay.
> - **No analytics / request tracking.**
>
> These are tracked as future work; if you need them today, deploy the upstream TypeScript implementation instead.

## 📈 Performance

Numbers below were measured on the native binary (release, x86_64, residential link) against live YouTube on 2026-05-09. Each video was hit five times with a fresh `reqwest::Client` per call to defeat connection-pool reuse — i.e. every measurement is a true cold path.

| Video | Length | Caption XML | Output | Total p50 |
|---|---|---|---|---|
| `dQw4w9WgXcQ` (Rick Astley) | 3 m 33 s | 4 KB | 2,335 ch | 1.36 s |
| `fNk_zzaMoSs` (3Blue1Brown) | ~10 m | 14 KB | 10,174 ch | 1.56 s |
| `-QFHIoCo-Ko` (Pocock talk) | 1 h 26 m | 550 KB | 89,753 ch | 1.82 s |
| `kCc8FmEb1nY` (Karpathy GPT) | 2 h+ | 785 KB | 108,990 ch | 1.76 s |

Stage breakdown (median across all runs):

- **Innertube POST**: ~1.0–1.4 s — dominated by the YouTube round-trip
- **Caption XML GET**: 0.2–0.9 s — scales with body size
- **XML → text parse**: ≤ 4 ms even for the 785 KB lecture

### Parser microbenchmarks

`crates/core` ships a Criterion benchmark that exercises `parse_transcript_xml` across synthetic srv3 inputs. To run it:

```bash
cargo bench -p youtube-transcript-mcp-core --bench parser
```

Representative throughput on the same hardware:

| Input | Time | Throughput |
|---|---|---|
| 14 KB | ~75 µs | ~185 MB/s |
| 150 KB | ~1.2 ms | ~120 MB/s |
| 1.5 MB | ~7.5 ms | ~200 MB/s |
| 8 MB | ~45 ms | ~170 MB/s |

The parser is linear in input size; its cost is well under the network round-trip for any caption track YouTube actually serves.

## 🚨 Error Handling

The server provides clear error messages for common issues:

- **"Invalid YouTube URL provided"**: The URL format is not recognized
- **"No transcript available for this video"**: Video has no captions/transcript
- **"Video not found or private"**: Video is private, deleted, or doesn't exist
- **"Network error: ..."**: Transient YouTube failure or transport error. The server does not retry — clients should retry idempotent calls with their own backoff.
- **"Transcripts are disabled for this video"**: Creator has disabled captions

## 🌍 Language Support

The server supports any language that YouTube provides transcripts for. Common language codes:

- `en` - English (default)
- `tr` - Turkish
- `es` - Spanish
- `fr` - French
- `de` - German
- `it` - Italian
- `pt` - Portuguese
- `ja` - Japanese
- `ko` - Korean
- `zh` - Chinese

## 🔒 Privacy & Security

- **No Authentication Required**: Public server for ease of use (configure your own auth in front if needed)
- **No Data Storage**: The server is stateless — no caching, no persistence; each request re-fetches from YouTube
- **No Personal Information**: Only YouTube video IDs and transcripts are processed in-memory for the duration of the request
- **HTTPS Only**: All communications are encrypted
- **CORS Enabled**: Supports web-based MCP clients

## 🚀 API Endpoints

For developers who want to integrate directly:

### HTTP POST `/mcp`
Standard MCP JSON-RPC endpoint for direct integration.

### Server-Sent Events `/sse`
MCP SSE transport endpoint for real-time communication.

### Info `/`
Server information and status endpoint.

**Example Direct API Call:**
```bash
curl -X POST https://youtube-transcript-mcp.ergut.workers.dev/mcp \
  -H "Content-Type: application/json" \
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

## 🐛 Troubleshooting

### Common Issues

1. **"Connection Error" in Claude Desktop**
   - Ensure you have the latest Claude Desktop version
   - Check that `mcp-remote` is properly configured
   - Try adding `--debug` flag to see detailed logs

2. **"Could not attach to MCP server"**
   - Verify your internet connection
   - Check the server URL is correct
   - Restart Claude Desktop after config changes

3. **No transcript returned**
   - Verify the YouTube URL is valid and accessible
   - Check if the video has captions enabled
   - Try a different language code if available

### Getting Help

- **Check logs**: Look in `~/.mcp-auth/` for debug logs
- **Test direct connection**: Use `npx -p mcp-remote@latest mcp-remote-client https://youtube-transcript-mcp.ergut.workers.dev/sse`
- **Verify server status**: Visit `https://youtube-transcript-mcp.ergut.workers.dev/`
- **Open an issue**: [GitHub Issues](https://github.com/ergut/youtube-transcript-mcp/issues)

## 🤝 Contributing

This is an open-source project. Contributions are welcome!

- **Report Issues**: Found a bug? Please [open an issue](https://github.com/ergut/youtube-transcript-mcp/issues)
- **Feature Requests**: Have ideas for improvements? Let us know!
- **Code Contributions**: PRs welcome for enhancements

### Development Setup

```bash
git clone https://github.com/ergut/youtube-transcript-mcp
cd youtube-transcript-mcp
npm install
npm run dev
```

## 📜 License

This project is open source and available under the [MIT License](LICENSE).

## 🙏 Acknowledgments

- Built with the [Model Context Protocol](https://modelcontextprotocol.io/) by Anthropic
- Hosted on [Cloudflare Workers](https://workers.cloudflare.com/)
- Uses the [youtube-transcript](https://www.npmjs.com/package/youtube-transcript) library
- Inspired by the MCP community and existing local transcript servers

---

**Ready to supercharge Claude with YouTube transcript extraction?** Add this server to your Claude Desktop configuration and start extracting transcripts from any YouTube video instantly! 🎉
