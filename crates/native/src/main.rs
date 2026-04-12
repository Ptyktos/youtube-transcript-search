use anyhow::{Context as _, Result};
use clap::Parser;
use rmcp::ServiceExt as _;

mod fetch;
mod tool;

use fetch::build_client;
use tool::TranscriptServer;

#[derive(Parser)]
#[command(
    name = "youtube-transcript-mcp",
    about = "YouTube Transcript MCP Server",
    long_about = "Exposes a single MCP tool `get_transcript` that fetches the full \
                  transcript of any YouTube video.\n\n\
                  Use --stdio for Claude Desktop and local MCP clients.\n\
                  Default: HTTP/SSE server."
)]
struct Cli {
    /// Use stdio transport (for Claude Desktop and local MCP clients)
    #[arg(long)]
    stdio: bool,

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
    let server = TranscriptServer::new(client);

    if cli.stdio {
        tracing::info!("Starting in stdio mode");
        let service = server
            .serve(rmcp::transport::stdio())
            .await
            .context("Failed to initialise stdio transport")?;
        service
            .waiting()
            .await
            .context("stdio server exited with error")?;
    } else {
        let addr = format!("{}:{}", cli.host, cli.port);
        tracing::info!("Starting HTTP/SSE server on http://{addr}");
        run_sse_server(server, &addr).await?;
    }

    Ok(())
}

async fn run_sse_server(server: TranscriptServer, addr: &str) -> Result<()> {
    use rmcp::transport::sse_server::{SseServer, SseServerConfig};
    use tokio_util::sync::CancellationToken;

    let bind: std::net::SocketAddr = addr.parse().context("Invalid bind address")?;
    let ct = CancellationToken::new();

    let config = SseServerConfig {
        bind,
        sse_path: "/sse".to_string(),
        post_path: "/message".to_string(),
        ct: ct.clone(),
    };

    let sse_server = SseServer::serve_with_config(config)
        .await
        .context("Failed to start SSE server")?;

    tracing::info!("Listening on http://{addr} — SSE: /sse, messages: /message");

    // Holds the service guard alive until shutdown; dropping it cancels the service.
    let _service_guard = sse_server.with_service(move || server.clone());

    // Wait for cancellation (Ctrl-C or signal)
    tokio::signal::ctrl_c()
        .await
        .context("Failed to listen for ctrl-c")?;
    tracing::info!("Shutting down");
    ct.cancel();

    Ok(())
}
