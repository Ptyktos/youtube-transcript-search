use serde::{Deserialize, Serialize};
use worker::*;
use youtube_transcript_mcp_core::{
    extract_video_id, parse_transcript_xml, parse_youtube_page, select_track, user_agent,
    watch_url, Language, TranscriptError,
};

// ── JSON-RPC types ────────────────────────────────────────────────────────────

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
        Self { jsonrpc: "2.0", id, result: Some(result), error: None }
    }

    fn err(id: Option<serde_json::Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError { code, message: message.into() }),
        }
    }
}

// ── CORS helpers ──────────────────────────────────────────────────────────────

fn cors_headers() -> Headers {
    let mut h = Headers::new();
    // These are static strings that will never fail to set.
    h.set("Access-Control-Allow-Origin", "*")
        .expect("valid header name");
    h.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        .expect("valid header name");
    h.set("Access-Control-Allow-Headers", "Content-Type, Accept")
        .expect("valid header name");
    h
}

fn json_response(body: &impl Serialize) -> Result<Response> {
    let json = serde_json::to_string(body).map_err(|e| Error::RustError(e.to_string()))?;
    let mut resp = Response::from_body(ResponseBody::Body(json.into_bytes()))?;
    let headers = resp.headers_mut();
    headers.set("Content-Type", "application/json")?;
    headers.set("Access-Control-Allow-Origin", "*")?;
    Ok(resp)
}

fn sse_response(body: &impl Serialize) -> Result<Response> {
    let json = serde_json::to_string(body).map_err(|e| Error::RustError(e.to_string()))?;
    let data = format!("data: {json}\n\n");
    let mut resp = Response::from_body(ResponseBody::Body(data.into_bytes()))?;
    let headers = resp.headers_mut();
    headers.set("Content-Type", "text/event-stream")?;
    headers.set("Cache-Control", "no-cache")?;
    headers.set("Access-Control-Allow-Origin", "*")?;
    Ok(resp)
}

// ── HTTP fetch using worker-rs ─────────────────────────────────────────────────

async fn fetch_text(url: &str) -> std::result::Result<String, TranscriptError> {
    let mut headers = Headers::new();
    headers
        .set("User-Agent", user_agent())
        .map_err(|e| TranscriptError::Network(e.to_string()))?;

    let mut init = RequestInit::new();
    init.with_method(Method::Get).with_headers(headers);

    let request =
        Request::new_with_init(url, &init).map_err(|e| TranscriptError::Network(e.to_string()))?;

    let mut response = Fetch::Request(request)
        .send()
        .await
        .map_err(|e| TranscriptError::Network(e.to_string()))?;

    if response.status_code() >= 400 {
        return Err(TranscriptError::Network(format!(
            "HTTP {} fetching {url}",
            response.status_code()
        )));
    }

    response
        .text()
        .await
        .map_err(|e| TranscriptError::Network(e.to_string()))
}

// ── Tool implementation ────────────────────────────────────────────────────────

async fn handle_get_transcript(
    args: &serde_json::Value,
) -> std::result::Result<String, TranscriptError> {
    let url = args["url"]
        .as_str()
        .ok_or_else(|| TranscriptError::Parse("Missing required parameter: url".into()))?;
    let language_str = args["language"].as_str().unwrap_or("auto");

    let video_id = extract_video_id(url)?;
    let language: Language = language_str.parse().unwrap_or_default();

    let page_html = fetch_text(&watch_url(&video_id)).await?;
    let tracks = parse_youtube_page(&page_html)?;
    let (track, fallback_note) = select_track(&tracks, &language)?;

    let xml = fetch_text(&track.base_url).await?;
    let raw_text = parse_transcript_xml(&xml)?;

    Ok(match fallback_note {
        Some(note) => format!("[{note}]\n\n{raw_text}"),
        None => raw_text,
    })
}

// ── MCP dispatcher ─────────────────────────────────────────────────────────────

async fn dispatch(rpc: RpcRequest) -> RpcResponse {
    let id = rpc.id;
    match rpc.method.as_str() {
        "initialize" => RpcResponse::ok(
            id,
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "youtube-transcript-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
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
                            "url": {
                                "type": "string",
                                "description": "YouTube video URL (any format)"
                            },
                            "language": {
                                "type": "string",
                                "description": "Language code (e.g. 'en', 'es'). Defaults to 'auto'."
                            }
                        },
                        "required": ["url"]
                    }
                }]
            }),
        ),

        "tools/call" => {
            let params = rpc.params.as_ref().and_then(|p| p.as_object());
            let tool_name = params
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("");

            if tool_name != "get_transcript" {
                return RpcResponse::err(id, -32601, format!("Unknown tool: {tool_name}"));
            }

            let empty = serde_json::Value::Null;
            let args = params
                .and_then(|p| p.get("arguments"))
                .unwrap_or(&empty);

            match handle_get_transcript(args).await {
                Ok(text) => RpcResponse::ok(
                    id,
                    serde_json::json!({
                        "content": [{ "type": "text", "text": text }]
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

// ── Worker entry point ─────────────────────────────────────────────────────────

#[event(fetch)]
pub async fn main(mut req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    // CORS preflight
    if req.method() == Method::Options {
        return Ok(Response::empty()?
            .with_headers(cors_headers())
            .with_status(204));
    }

    let path = req.path();

    match (req.method(), path.as_str()) {
        (_, "/") => json_response(&serde_json::json!({
            "name": "youtube-transcript-mcp",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Remote MCP server for YouTube video transcripts",
            "endpoints": { "sse": "/sse", "mcp": "/mcp" },
            "tools": ["get_transcript"]
        })),

        (Method::Post, "/mcp") => {
            let rpc: RpcRequest = req
                .json()
                .await
                .map_err(|_| Error::RustError("Invalid JSON-RPC body".into()))?;
            let response = dispatch(rpc).await;
            json_response(&response)
        }

        (Method::Post, "/sse") => {
            let rpc: RpcRequest = req
                .json()
                .await
                .map_err(|_| Error::RustError("Invalid JSON-RPC body".into()))?;
            let response = dispatch(rpc).await;
            sse_response(&response)
        }

        (Method::Get, "/sse") => {
            // SSE handshake — send initialized notification
            sse_response(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {}
            }))
        }

        _ => Response::error("Not Found", 404),
    }
}
