use anyhow::Context as _;
use reqwest::Client;
use youtube_transcript_mcp_core::{
    extract_video_id, innertube_body, parse_innertube_response, parse_transcript_xml, select_track,
    Language, TranscriptError, TranscriptResult, INNERTUBE_URL, USER_AGENT,
};

/// Build a shared reqwest [`Client`] with sensible timeouts.
///
/// # Errors
/// Returns an error if the HTTP client cannot be constructed.
pub fn build_client() -> anyhow::Result<Client> {
    Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .context("Failed to build HTTP client")
}

async fn get_text(client: &Client, url: &str) -> Result<String, TranscriptError> {
    client
        .get(url)
        .send()
        .await
        .map_err(|e| TranscriptError::Network(format!("GET {url}: {e}")))?
        .error_for_status()
        .map_err(|e| TranscriptError::Network(format!("GET {url}: {e}")))?
        .text()
        .await
        .map_err(|e| TranscriptError::Network(format!("GET {url}: {e}")))
}

/// Fetch the transcript for a `YouTube` URL with language selection and fallback.
///
/// `language_str` accepts `"auto"` (default) or any BCP-47 language code.
///
/// # Errors
/// Propagates [`TranscriptError`] for invalid URLs, network failures, missing transcripts,
/// or unavailable languages.
pub async fn get_transcript(
    client: &Client,
    url: &str,
    language_str: &str,
) -> Result<TranscriptResult, TranscriptError> {
    let video_id = extract_video_id(url)?;
    let language: Language = language_str.parse().unwrap_or_default();

    // Step 1: POST to Innertube
    let body = innertube_body(&video_id);
    let body_bytes = serde_json::to_vec(&body)
        .map_err(|e| TranscriptError::Parse(format!("Failed to serialize Innertube body: {e}")))?;
    let innertube_json = client
        .post(INNERTUBE_URL)
        .header("User-Agent", USER_AGENT)
        .header("Content-Type", "application/json")
        .body(body_bytes)
        .send()
        .await
        .map_err(|e| TranscriptError::Network(format!("POST {INNERTUBE_URL}: {e}")))?
        .error_for_status()
        .map_err(|e| TranscriptError::Network(format!("POST {INNERTUBE_URL}: {e}")))?
        .text()
        .await
        .map_err(|e| TranscriptError::Network(format!("POST {INNERTUBE_URL}: {e}")))?;

    // Step 2: parse caption tracks
    let tracks = parse_innertube_response(&innertube_json)?;

    // Step 3: select track
    let (track, fallback_note) = select_track(&tracks, &language)?;

    // Step 4: fetch XML
    let xml = get_text(client, &track.base_url).await?;

    // Step 5: parse to plain text
    let raw_text = parse_transcript_xml(&xml)?;

    let text = match fallback_note {
        Some(note) => format!("[{note}]\n\n{raw_text}"),
        None => raw_text,
    };

    Ok(TranscriptResult {
        text,
        language: track.language_code.clone(),
    })
}
