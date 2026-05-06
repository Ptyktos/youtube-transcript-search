//! HTTP server: raw `/transcript` endpoint plus a minimal MCP JSON-RPC
//! dispatcher (`POST /mcp`, `GET|POST /sse`). Mirrors the worker crate so
//! both deployments expose the same wire shape.

use anyhow::{Context as _, Result};
use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use futures::stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use tower_http::cors::{Any, CorsLayer};
use youtube_transcript_mcp_core::TranscriptError;

use crate::fetch::get_transcript;

// ── State + routing ───────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    client: Client,
}

pub async fn serve(client: Client, addr: &str) -> Result<()> {
    let state = AppState { client };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(info))
        .route("/transcript", get(raw_transcript))
        .route("/mcp", post(mcp_post))
        .route("/sse", get(sse_get).post(sse_post))
        .with_state(state)
        .layer(cors);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind {addr}"))?;
    tracing::info!("Listening on http://{addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .context("HTTP server error")
}

// ── Plain endpoints ───────────────────────────────────────────────────────────

async fn info() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "name": "youtube-transcript-mcp",
        "version": env!("CARGO_PKG_VERSION"),
        "endpoints": {
            "info": "GET /",
            "transcript": "GET /transcript?url=...&language=...",
            "mcp": "POST /mcp",
            "sse": "GET|POST /sse",
        },
        "tools": ["get_transcript"],
    }))
}

#[derive(Deserialize)]
struct TranscriptQuery {
    url: String,
    language: Option<String>,
}

async fn raw_transcript(
    State(state): State<AppState>,
    Query(q): Query<TranscriptQuery>,
) -> Response {
    let lang = q.language.as_deref().unwrap_or("auto");
    match get_transcript(&state.client, &q.url, lang).await {
        Ok(r) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            r.text,
        )
            .into_response(),
        Err(e) => (error_status(&e), e.to_string()).into_response(),
    }
}

fn error_status(e: &TranscriptError) -> StatusCode {
    match e {
        TranscriptError::InvalidUrl | TranscriptError::InvalidVideoId => StatusCode::BAD_REQUEST,
        TranscriptError::NoTranscriptAvailable | TranscriptError::LanguageNotAvailable { .. } => {
            StatusCode::NOT_FOUND
        }
        TranscriptError::Network(_) => StatusCode::BAD_GATEWAY,
        TranscriptError::Parse(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

// ── MCP JSON-RPC ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RpcRequest {
    id: Option<serde_json::Value>,
    method: String,
    params: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct RpcResponse {
    jsonrpc: &'static str,
    id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

impl RpcResponse {
    fn ok(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }
    fn err(id: Option<serde_json::Value>, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError { code, message }),
        }
    }
}

async fn dispatch(state: &AppState, rpc: RpcRequest) -> RpcResponse {
    let id = rpc.id;
    match rpc.method.as_str() {
        "initialize" => RpcResponse::ok(
            id,
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "youtube-transcript-mcp",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }),
        ),

        "tools/list" => RpcResponse::ok(
            id,
            serde_json::json!({
                "tools": [{
                    "name": "get_transcript",
                    "description": "Extract the full transcript from a YouTube video URL",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "url": { "type": "string", "description": "YouTube video URL (any format)" },
                            "language": { "type": "string", "description": "Language code (e.g. 'en', 'es'). Defaults to 'auto'." }
                        },
                        "required": ["url"],
                    },
                }],
            }),
        ),

        "tools/call" => {
            let params = rpc.params.as_ref().and_then(serde_json::Value::as_object);
            let tool_name = params
                .and_then(|p| p.get("name"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");

            if tool_name != "get_transcript" {
                return RpcResponse::err(id, -32601, format!("Unknown tool: {tool_name}"));
            }

            let empty = serde_json::Value::Null;
            let args = params.and_then(|p| p.get("arguments")).unwrap_or(&empty);
            let url = args
                .get("url")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let language = args
                .get("language")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("auto");

            match get_transcript(&state.client, url, language).await {
                Ok(r) => RpcResponse::ok(
                    id,
                    serde_json::json!({
                        "content": [{ "type": "text", "text": r.text }],
                    }),
                ),
                Err(e) => {
                    let code = match &e {
                        TranscriptError::InvalidUrl | TranscriptError::InvalidVideoId => -32602,
                        _ => -1,
                    };
                    RpcResponse::err(id, code, e.to_string())
                }
            }
        }

        other => RpcResponse::err(id, -32601, format!("Method not found: {other}")),
    }
}

async fn mcp_post(State(state): State<AppState>, Json(rpc): Json<RpcRequest>) -> Json<RpcResponse> {
    Json(dispatch(&state, rpc).await)
}

// ── SSE ──────────────────────────────────────────────────────────────────────
// Single-shot JSON-RPC over SSE — same shape as the Cloudflare Worker. Real
// MCP clients should prefer `POST /mcp` (Streamable HTTP) or stdio.

fn sse_event(payload: &serde_json::Value) -> Event {
    let data = serde_json::to_string(payload).unwrap_or_else(|_| "{}".into());
    Event::default().event("message").data(data)
}

async fn sse_get() -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let stream = stream::once(async {
        Ok::<_, Infallible>(sse_event(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        })))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn sse_post(
    State(state): State<AppState>,
    Json(rpc): Json<RpcRequest>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let response = dispatch(&state, rpc).await;
    let payload = serde_json::to_value(&response).unwrap_or_else(|_| serde_json::json!({}));
    let stream = stream::once(async move { Ok::<_, Infallible>(sse_event(&payload)) });
    Sse::new(stream)
}
