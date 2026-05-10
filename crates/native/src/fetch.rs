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
    get_transcript_via(client, INNERTUBE_URL, url, language_str).await
}

/// Like [`get_transcript`] but with an injectable Innertube endpoint, for testing.
async fn get_transcript_via(
    client: &Client,
    innertube_url: &str,
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
        .post(innertube_url)
        .header("User-Agent", USER_AGENT)
        .header("Content-Type", "application/json")
        .body(body_bytes)
        .send()
        .await
        .map_err(|e| TranscriptError::Network(format!("POST {innertube_url}: {e}")))?
        .error_for_status()
        .map_err(|e| TranscriptError::Network(format!("POST {innertube_url}: {e}")))?
        .text()
        .await
        .map_err(|e| TranscriptError::Network(format!("POST {innertube_url}: {e}")))?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const CAPTION_XML: &str = r#"<?xml version="1.0" encoding="utf-8" ?><transcript><text start="0" dur="1">Hello world</text><text start="1" dur="2">This is a test</text></transcript>"#;

    fn innertube_ok(caption_base_url: &str) -> String {
        serde_json::json!({
            "playabilityStatus": { "status": "OK" },
            "captions": {
                "playerCaptionsTracklistRenderer": {
                    "captionTracks": [
                        { "baseUrl": caption_base_url, "languageCode": "en" }
                    ]
                }
            }
        })
        .to_string()
    }

    /// Regression guard: the caption-XML GET must carry the Android `User-Agent`.
    /// Without it YouTube returns truncated/empty bodies, which previously
    /// surfaced as a successful but empty transcript.
    #[tokio::test]
    async fn caption_get_sends_user_agent() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/caption"))
            .and(header("user-agent", USER_AGENT))
            .respond_with(ResponseTemplate::new(200).set_body_string(CAPTION_XML))
            .expect(1)
            .mount(&server)
            .await;

        let client = build_client().expect("client");
        let body = get_text(&client, &format!("{}/caption", server.uri()))
            .await
            .expect("caption GET should succeed when UA header is present");
        assert!(body.contains("Hello world"));
    }

    /// If the UA header is dropped the mock above stops matching; mirror that
    /// path explicitly so the failure mode is documented even if the matcher
    /// changes: an unmatched request yields a 4xx, which must become a
    /// `TranscriptError::Network`.
    #[tokio::test]
    async fn caption_get_propagates_http_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/caption"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = build_client().expect("client");
        let err = get_text(&client, &format!("{}/caption", server.uri()))
            .await
            .expect_err("500 should be an error");
        assert!(matches!(err, TranscriptError::Network(_)));
    }

    /// End-to-end through the same code path that shipped broken: Innertube POST
    /// → parse → caption GET → parse. Both upstream requests are required to
    /// carry the `User-Agent`; if either drops it the mocks won't match.
    #[tokio::test]
    async fn full_pipeline_through_mock_server() {
        let server = MockServer::start().await;
        let caption_url = format!("{}/caption", server.uri());

        Mock::given(method("POST"))
            .and(path("/youtubei/v1/player"))
            .and(header("user-agent", USER_AGENT))
            .respond_with(ResponseTemplate::new(200).set_body_string(innertube_ok(&caption_url)))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/caption"))
            .and(header("user-agent", USER_AGENT))
            .respond_with(ResponseTemplate::new(200).set_body_string(CAPTION_XML))
            .expect(1)
            .mount(&server)
            .await;

        let client = build_client().expect("client");
        let innertube_url = format!("{}/youtubei/v1/player", server.uri());
        let result = get_transcript_via(
            &client,
            &innertube_url,
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "auto",
        )
        .await
        .expect("full pipeline should succeed");

        assert_eq!(result.language, "en");
        assert_eq!(result.text, "Hello world This is a test");
    }
}
