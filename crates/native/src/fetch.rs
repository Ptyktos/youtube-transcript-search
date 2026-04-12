use anyhow::Context as _;
use reqwest::Client;
use youtube_transcript_mcp_core::{
    extract_video_id, parse_transcript_xml, parse_youtube_page, select_track, user_agent,
    watch_url, Language, TranscriptError, TranscriptResult,
};

/// Build a shared reqwest [`Client`] with the `YouTube` `User-Agent` header.
///
/// # Errors
/// Returns an error if the HTTP client cannot be constructed.
pub fn build_client() -> anyhow::Result<Client> {
    Client::builder()
        .user_agent(user_agent())
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

    let page_html = get_text(client, &watch_url(&video_id)).await?;
    let tracks = parse_youtube_page(&page_html)?;
    let (track, fallback_note) = select_track(&tracks, &language)?;

    let xml = get_text(client, &track.base_url).await?;
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
