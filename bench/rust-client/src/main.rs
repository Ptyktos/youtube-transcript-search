// Mirror of the TS benchmark driver. Same modes, same JSON output shape, so
// the harness can compare directly.

use anyhow::Result;
use reqwest::Client;
use std::time::Instant;
use youtube_transcript_mcp_core::{
    extract_video_id, innertube_body, parse_innertube_response, parse_transcript_xml, select_track,
    InnertubeData, Language,
};

const SAMPLE_URL: &str = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";

fn mock_url() -> String {
    std::env::var("MOCK_URL").unwrap_or_else(|_| "http://127.0.0.1:18080".into())
}

fn read_self_vmhwm_kib() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            return rest
                .trim()
                .split_whitespace()
                .next()
                .and_then(|n| n.parse().ok());
        }
    }
    None
}

fn build_client() -> Result<Client> {
    Ok(Client::builder()
        .pool_idle_timeout(std::time::Duration::from_secs(60))
        .build()?)
}

async fn fetch_once(client: &Client, base: &str) -> Result<String> {
    use std::str::FromStr;
    let video_id = extract_video_id(SAMPLE_URL)?;
    let body = innertube_body(&video_id);

    let innertube_url = format!("{base}/youtubei/v1/player");
    let resp = client
        .post(&innertube_url)
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let InnertubeData { caption_tracks, .. } = parse_innertube_response(&resp)?;
    let lang = Language::from_str("auto")?;
    let (track, _note) = select_track(&caption_tracks, &lang)?;

    let xml = client
        .get(&track.base_url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let text = parse_transcript_xml(&xml)?;
    Ok(text)
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "cold".into());
    let iters: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(100);
    let base = mock_url();

    match mode.as_str() {
        "parse" => {
            let xml = std::fs::read_to_string("/tmp/bench/canned/caption.xml")?;
            let t0 = Instant::now();
            let mut bytes_out: usize = 0;
            for _ in 0..iters {
                bytes_out += parse_transcript_xml(&xml)?.len();
            }
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            println!(
                "{}",
                serde_json::json!({"mode":"parse","iters":iters,"ms":ms,"bytesIn":xml.len(),"bytesOut":bytes_out})
            );
        }
        "parse-json" => {
            let txt = std::fs::read_to_string("/tmp/bench/canned/innertube.json")?;
            let t0 = Instant::now();
            let mut n: usize = 0;
            for _ in 0..iters {
                let data = parse_innertube_response(&txt)?;
                n += data.caption_tracks.len();
            }
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            println!(
                "{}",
                serde_json::json!({"mode":"parse-json","iters":iters,"ms":ms,"n":n})
            );
        }
        "url" => {
            let t0 = Instant::now();
            let mut n: usize = 0;
            for _ in 0..iters {
                n += extract_video_id(SAMPLE_URL)?.as_str().len();
            }
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            println!(
                "{}",
                serde_json::json!({"mode":"url","iters":iters,"ms":ms,"n":n})
            );
        }
        "e2e" => {
            let client = build_client()?;
            let mut samples: Vec<f64> = Vec::with_capacity(iters);
            for i in 0..iters {
                let t0 = Instant::now();
                let r = fetch_once(&client, &base).await?;
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                samples.push(ms);
                if i == 0 && r.is_empty() {
                    anyhow::bail!("empty result")
                }
            }
            println!(
                "{}",
                serde_json::json!({"mode":"e2e","iters":iters,"samples":samples})
            );
        }
        "cold" => {
            let client = build_client()?;
            let t0 = Instant::now();
            let r = fetch_once(&client, &base).await?;
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            println!(
                "{}",
                serde_json::json!({"mode":"cold","ms":ms,"len":r.len()})
            );
        }
        "memory" => {
            let client = build_client()?;
            for _ in 0..iters {
                let _ = fetch_once(&client, &base).await?;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            let vm_hwm_kib = read_self_vmhwm_kib().unwrap_or(0);
            println!(
                "{}",
                serde_json::json!({"mode":"memory","iters":iters,"rssKib":vm_hwm_kib})
            );
        }
        "serve" => serve_mcp_stdio(&base).await?,
        other => anyhow::bail!("unknown mode: {other}"),
    }
    Ok(())
}

// Minimal MCP-stdio handler so we can be benchmarked through the same
// `drive.js` harness used for the Node MCPs. Implements just the methods
// the harness uses: initialize, notifications/initialized, tools/list,
// tools/call.
async fn serve_mcp_stdio(base: &str) -> Result<()> {
    use serde_json::json;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let client = build_client()?;
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();
    while let Ok(Some(line)) = reader.next_line().await {
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        let req: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let result: Option<serde_json::Value> = match method {
            "initialize" => Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "rust-bench", "version": env!("CARGO_PKG_VERSION") }
            })),
            "notifications/initialized" => None,
            "tools/list" => Some(json!({
                "tools": [{
                    "name": "get_transcript",
                    "description": "Bench tool",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "url": {"type":"string"} },
                        "required": ["url"]
                    }
                }]
            })),
            "tools/call" => {
                let _name = req.pointer("/params/name").and_then(|s| s.as_str()).unwrap_or("");
                let url = req.pointer("/params/arguments/url").and_then(|s| s.as_str()).unwrap_or(SAMPLE_URL);
                match fetch_via(&client, base, url).await {
                    Ok(text) => Some(json!({"content":[{"type":"text","text":text}]})),
                    Err(e) => {
                        let resp = json!({
                            "jsonrpc":"2.0", "id": id.clone(),
                            "result": { "content":[{"type":"text","text": e.to_string()}], "isError": true }
                        });
                        let mut s = serde_json::to_string(&resp)?;
                        s.push('\n');
                        stdout.write_all(s.as_bytes()).await?;
                        stdout.flush().await?;
                        continue;
                    }
                }
            }
            _ => None,
        };
        if let Some(result) = result {
            let resp = json!({"jsonrpc":"2.0","id":id,"result":result});
            let mut s = serde_json::to_string(&resp)?;
            s.push('\n');
            stdout.write_all(s.as_bytes()).await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}

async fn fetch_via(client: &Client, base: &str, url: &str) -> Result<String> {
    use std::str::FromStr;
    let video_id = extract_video_id(url)?;
    let body = innertube_body(&video_id);
    let resp = client
        .post(&format!("{base}/youtubei/v1/player"))
        .json(&body).send().await?.error_for_status()?.text().await?;
    let InnertubeData { caption_tracks, .. } = parse_innertube_response(&resp)?;
    let lang = Language::from_str("auto")?;
    let (track, _) = select_track(&caption_tracks, &lang)?;
    let xml = client.get(&track.base_url).send().await?.error_for_status()?.text().await?;
    Ok(parse_transcript_xml(&xml)?)
}
