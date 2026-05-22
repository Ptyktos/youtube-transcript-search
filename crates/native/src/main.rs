use anyhow::{Context as _, Result};
use clap::Parser;
use rmcp::ServiceExt as _;
use std::io::Write as _;

mod fetch;
mod http;
mod tool;

use fetch::{build_client, get_transcript};
use tool::TranscriptServer;

#[derive(Parser)]
#[command(
    name = "youtube-transcript-mcp",
    about = "YouTube Transcript MCP Server",
    long_about = "Fetches the full transcript of a YouTube video.\n\n\
                  Modes:\n  \
                  --url <URL>   one-shot CLI: print transcript to stdout and exit\n  \
                  --stdio       MCP stdio transport (Claude Desktop, local clients)\n  \
                  default       HTTP server with /transcript, /mcp, /sse"
)]
struct Cli {
    /// Use MCP stdio transport (for Claude Desktop and local MCP clients)
    #[arg(long, conflicts_with = "url")]
    stdio: bool,

    /// One-shot mode: print the transcript for this URL to stdout and exit
    #[arg(long, value_name = "URL")]
    url: Option<String>,

    /// Language code (with --url). Defaults to `auto`.
    #[arg(long, default_value = "auto")]
    language: String,

    /// Output format (with --url): text, json, srt, vtt, or markdown.
    /// json and markdown include clickable timestamp links.
    #[arg(long, default_value = "text")]
    format: String,

    /// Host to bind the HTTP server to
    #[arg(long, default_value = "127.0.0.1", env = "HOST")]
    host: String,

    /// Port to bind the HTTP server to
    #[arg(long, default_value = "3000", env = "PORT")]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "youtube_transcript_mcp=info,rmcp=warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let client = build_client()?;

    if let Some(url) = cli.url.as_deref() {
        return run_oneshot(client, url, &cli.language, &cli.format).await;
    }

    if cli.stdio {
        tracing::info!("Starting in stdio mode");
        let server = TranscriptServer::new(client);
        let service = server
            .serve(rmcp::transport::stdio())
            .await
            .context("Failed to initialise stdio transport")?;
        service
            .waiting()
            .await
            .context("stdio server exited with error")?;
        return Ok(());
    }

    let addr = format!("{}:{}", cli.host, cli.port);
    tracing::info!("Starting HTTP server on http://{addr}");
    http::serve(client, &addr).await
}

async fn run_oneshot(
    client: reqwest::Client,
    url: &str,
    language: &str,
    format: &str,
) -> Result<()> {
    let result = get_transcript(&client, url, language, format)
        .await
        .with_context(|| format!("Failed to fetch transcript for {url}"))?;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(result.text.as_bytes())?;
    stdout.write_all(b"\n")?;
    Ok(())
}
