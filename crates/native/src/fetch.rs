use anyhow::Context as _;
use reqwest::Client;
use youtube_transcript_mcp_core::{
    extract_video_id, innertube_body, parse_innertube_response, parse_transcript_xml, select_track,
    AudioStream, InnertubeData, Language, TranscriptError, TranscriptResult, INNERTUBE_URL,
    USER_AGENT,
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
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|e| TranscriptError::Network(format!("GET {url}: {e}")))?
        .error_for_status()
        .map_err(|e| TranscriptError::Network(format!("GET {url}: {e}")))?
        .text()
        .await
        .map_err(|e| TranscriptError::Network(format!("GET {url}: {e}")))
}

async fn get_bytes(client: &Client, url: &str) -> Result<Vec<u8>, TranscriptError> {
    Ok(client
        .get(url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|e| TranscriptError::Network(format!("GET {url}: {e}")))?
        .error_for_status()
        .map_err(|e| TranscriptError::Network(format!("GET {url}: {e}")))?
        .bytes()
        .await
        .map_err(|e| TranscriptError::Network(format!("GET {url}: {e}")))?
        .to_vec())
}

/// Send an audio stream to a faster-whisper-server instance for transcription.
///
/// # Errors
/// Returns `TranscriptError::Network` on HTTP failures.
async fn whisper_transcribe(
    client: &Client,
    whisper_url: &str,
    stream: &AudioStream,
) -> Result<String, TranscriptError> {
    let audio_bytes = get_bytes(client, &stream.url).await?;

    let filename = if stream.mime_type.contains("mp4") {
        "audio.mp4"
    } else {
        "audio.webm"
    };

    let mime = stream
        .mime_type
        .split(';')
        .next()
        .unwrap_or("audio/mp4")
        .trim()
        .to_string();

    let part = reqwest::multipart::Part::bytes(audio_bytes)
        .file_name(filename)
        .mime_str(&mime)
        .map_err(|e| TranscriptError::Network(e.to_string()))?;

    let form = reqwest::multipart::Form::new()
        .text("model", "small")
        .text("response_format", "text")
        .part("file", part);

    let endpoint = format!("{whisper_url}/v1/audio/transcriptions");
    let text = client
        .post(&endpoint)
        .header("User-Agent", USER_AGENT)
        .multipart(form)
        .send()
        .await
        .map_err(|e| TranscriptError::Network(format!("POST {endpoint}: {e}")))?
        .error_for_status()
        .map_err(|e| TranscriptError::Network(format!("POST {endpoint}: {e}")))?
        .text()
        .await
        .map_err(|e| TranscriptError::Network(format!("POST {endpoint}: {e}")))?;

    Ok(text.trim().to_string())
}

/// Fetch the transcript for a `YouTube` URL with language selection and fallback.
///
/// `language_str` accepts `"auto"` (default) or any BCP-47 language code.
///
/// When no caption tracks are available and the `WHISPER_URL` environment variable is set,
/// falls back to Whisper ASR transcription via a faster-whisper-server instance.
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

    // Step 2: parse caption tracks + audio streams
    let InnertubeData {
        caption_tracks,
        audio_streams,
    } = parse_innertube_response(&innertube_json)?;

    // Step 3: no captions — try Whisper fallback
    if caption_tracks.is_empty() {
        if let Ok(whisper_url) = std::env::var("WHISPER_URL") {
            if let Some(stream) = audio_streams.first() {
                let text = whisper_transcribe(client, &whisper_url, stream).await?;
                return Ok(TranscriptResult {
                    text: format!("[AI-generated transcript — no captions available]\n\n{text}"),
                    language: "en".into(),
                });
            }
        }
        return Err(TranscriptError::NoTranscriptAvailable);
    }

    // Step 4: select caption track
    let (track, fallback_note) = select_track(&caption_tracks, &language)?;

    // Step 5: fetch XML
    let xml = get_text(client, &track.base_url).await?;

    // Step 6: parse to plain text
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
