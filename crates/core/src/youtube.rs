use crate::error::TranscriptError;
use crate::language::{Language, AUTO_DETECT_ORDER};
use crate::url::VideoId;
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::Deserialize;

pub const INNERTUBE_URL: &str = "https://www.youtube.com/youtubei/v1/player";

pub const USER_AGENT: &str =
    "com.google.android.youtube/20.10.38 (Linux; U; Android 11) gzip";

/// Returns the `User-Agent` string used for all `YouTube` requests.
#[must_use]
pub fn user_agent() -> &'static str {
    USER_AGENT
}

/// Builds the JSON body for a POST to [`INNERTUBE_URL`].
#[must_use]
pub fn innertube_body(video_id: &VideoId) -> serde_json::Value {
    serde_json::json!({
        "context": {
            "client": {
                "clientName": "ANDROID",
                "clientVersion": "20.10.38",
                "androidSdkVersion": 30,
                "hl": "en",
                "gl": "US"
            }
        },
        "videoId": video_id.as_str()
    })
}

/// A single caption track available for a video.
#[derive(Debug, Clone, Deserialize)]
pub struct CaptionTrack {
    #[serde(rename = "baseUrl")]
    pub base_url: String,
    #[serde(rename = "languageCode")]
    pub language_code: String,
}

/// The output of a successful transcript fetch.
#[derive(Debug, Clone)]
pub struct TranscriptResult {
    /// Transcript text (with optional fallback note prepended if language fell back).
    pub text: String,
    /// The language code that was actually used.
    pub language: String,
}

/// The best available audio-only stream for a video, used as Whisper input.
#[derive(Debug, Clone)]
pub struct AudioStream {
    /// Direct URL to the audio stream.
    pub url: String,
    /// MIME type, e.g. `"audio/mp4; codecs=\"mp4a.40.2\""`.
    pub mime_type: String,
    /// Average bitrate in bits per second.
    pub bitrate: u64,
}

/// Parsed output of the Innertube `/youtubei/v1/player` response.
#[derive(Debug, Clone)]
pub struct InnertubeData {
    /// Available caption tracks (empty if none).
    pub caption_tracks: Vec<CaptionTrack>,
    /// Audio-only streams, sorted by bitrate descending.
    pub audio_streams: Vec<AudioStream>,
}

#[derive(Deserialize)]
struct CaptionsContainer {
    #[serde(rename = "playerCaptionsTracklistRenderer")]
    renderer: Option<TracklistRenderer>,
}

#[derive(Deserialize)]
struct TracklistRenderer {
    #[serde(rename = "captionTracks")]
    caption_tracks: Option<Vec<CaptionTrack>>,
}

/// Parse the JSON response from the Innertube `/youtubei/v1/player` endpoint.
///
/// Returns caption tracks (empty if none) and audio-only streams sorted by bitrate descending
/// (mp4 preferred over webm for wider compatibility).
///
/// # Errors
/// - `TranscriptError::Parse` if the JSON is malformed.
/// - `TranscriptError::Parse` if playabilityStatus is not OK.
pub fn parse_innertube_response(json: &str) -> Result<InnertubeData, TranscriptError> {
    #[derive(Deserialize)]
    struct InnertubeResponse {
        #[serde(rename = "playabilityStatus")]
        playability_status: Option<PlayabilityStatus>,
        captions: Option<CaptionsContainer>,
        #[serde(rename = "streamingData")]
        streaming_data: Option<StreamingData>,
    }

    #[derive(Deserialize)]
    struct PlayabilityStatus {
        status: String,
        reason: Option<String>,
    }

    #[derive(Deserialize)]
    struct StreamingData {
        #[serde(rename = "adaptiveFormats")]
        adaptive_formats: Option<Vec<AdaptiveFormat>>,
    }

    #[derive(Deserialize)]
    struct AdaptiveFormat {
        #[serde(rename = "mimeType")]
        mime_type: Option<String>,
        url: Option<String>,
        #[serde(rename = "averageBitrate")]
        average_bitrate: Option<u64>,
    }

    let response: InnertubeResponse = serde_json::from_str(json).map_err(|e| {
        TranscriptError::Parse(format!("Failed to decode Innertube response: {e}"))
    })?;

    if let Some(ps) = &response.playability_status {
        if ps.status != "OK" {
            let reason = ps.reason.as_deref().unwrap_or("unknown");
            return Err(TranscriptError::Parse(format!(
                "Video not playable: {} — {reason}",
                ps.status
            )));
        }
    }

    let caption_tracks = response
        .captions
        .and_then(|c| c.renderer)
        .and_then(|r| r.caption_tracks)
        .unwrap_or_default();

    let mut audio_streams: Vec<AudioStream> = response
        .streaming_data
        .and_then(|sd| sd.adaptive_formats)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|f| {
            let mime_type = f.mime_type?;
            let url = f.url?;
            if !mime_type.starts_with("audio/") {
                return None;
            }
            Some(AudioStream {
                url,
                bitrate: f.average_bitrate.unwrap_or(0),
                mime_type,
            })
        })
        .collect();

    // Highest bitrate first — prefer mp4 audio over webm for wider compatibility
    audio_streams.sort_by(|a, b| {
        let a_mp4 = a.mime_type.contains("mp4");
        let b_mp4 = b.mime_type.contains("mp4");
        b_mp4.cmp(&a_mp4).then(b.bitrate.cmp(&a.bitrate))
    });

    Ok(InnertubeData {
        caption_tracks,
        audio_streams,
    })
}

/// Select the best caption track for the requested language.
///
/// Returns `(track, fallback_note)` where `fallback_note` is `Some(message)` when
/// we fell back from the requested language to English.
///
/// # Errors
/// - `NoTranscriptAvailable` if `tracks` is empty.
/// - `LanguageNotAvailable` if the requested language and English are both absent.
pub fn select_track<'a>(
    tracks: &'a [CaptionTrack],
    language: &Language,
) -> Result<(&'a CaptionTrack, Option<String>), TranscriptError> {
    if tracks.is_empty() {
        return Err(TranscriptError::NoTranscriptAvailable);
    }

    match language {
        Language::Auto => {
            for lang in AUTO_DETECT_ORDER {
                if let Some(t) = tracks.iter().find(|t| t.language_code == *lang) {
                    return Ok((t, None));
                }
            }
            Ok((&tracks[0], None))
        }
        Language::Specific(requested) => {
            if let Some(t) = tracks.iter().find(|t| &t.language_code == requested) {
                return Ok((t, None));
            }
            if requested != "en" {
                if let Some(t) = tracks.iter().find(|t| t.language_code == "en") {
                    let note = format!(
                        "Requested language '{requested}' not available, showing English instead"
                    );
                    return Ok((t, Some(note)));
                }
            }
            let available = tracks.iter().map(|t| t.language_code.clone()).collect();
            Err(TranscriptError::LanguageNotAvailable {
                requested: requested.clone(),
                available,
            })
        }
    }
}

/// Parse a `YouTube` transcript XML response into a single plain-text string.
///
/// Handles both `<text>` elements (legacy format) and `<p>` elements (Innertube format).
/// XML entities (`&amp;`, `&lt;`, etc.) are unescaped automatically by `quick-xml`.
///
/// # Errors
/// Returns `TranscriptError::Parse` if the XML is malformed.
pub fn parse_transcript_xml(xml: &str) -> Result<String, TranscriptError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut texts: Vec<String> = Vec::new();
    let mut in_text_element = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e))
                if e.name().as_ref() == b"p" || e.name().as_ref() == b"text" =>
            {
                in_text_element = true;
            }
            Ok(Event::Text(ref e)) if in_text_element => {
                let s = e
                    .unescape()
                    .map_err(|e| TranscriptError::Parse(e.to_string()))?;
                let trimmed = s.trim().to_string();
                if !trimmed.is_empty() {
                    texts.push(trimmed);
                }
            }
            Ok(Event::End(ref e))
                if e.name().as_ref() == b"p" || e.name().as_ref() == b"text" =>
            {
                in_text_element = false;
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(TranscriptError::Parse(e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok(texts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::Language;

    // --- parse_innertube_response ---

    #[test]
    fn parses_ok_response_with_tracks() {
        let json = r#"{
            "playabilityStatus": {"status": "OK"},
            "captions": {
                "playerCaptionsTracklistRenderer": {
                    "captionTracks": [
                        {"baseUrl": "https://example.com/caps", "languageCode": "en"}
                    ]
                }
            }
        }"#;
        let data = parse_innertube_response(json).unwrap();
        assert_eq!(data.caption_tracks.len(), 1);
        assert_eq!(data.caption_tracks[0].language_code, "en");
    }

    #[test]
    fn returns_empty_when_no_captions() {
        let json = r#"{"playabilityStatus": {"status": "OK"}}"#;
        let data = parse_innertube_response(json).unwrap();
        assert!(data.caption_tracks.is_empty());
    }

    #[test]
    fn errors_on_unplayable_status() {
        let json =
            r#"{"playabilityStatus": {"status": "UNPLAYABLE", "reason": "Video unavailable"}}"#;
        let err = parse_innertube_response(json).unwrap_err();
        assert!(err.to_string().contains("UNPLAYABLE"));
    }

    // --- select_track ---

    #[test]
    fn auto_prefers_english_over_others() {
        let tracks = vec![
            CaptionTrack {
                base_url: "fr".into(),
                language_code: "fr".into(),
            },
            CaptionTrack {
                base_url: "en".into(),
                language_code: "en".into(),
            },
        ];
        let (t, note) = select_track(&tracks, &Language::Auto).unwrap();
        assert_eq!(t.language_code, "en");
        assert!(note.is_none());
    }

    #[test]
    fn auto_falls_back_to_first_when_no_preferred_available() {
        let tracks = vec![CaptionTrack {
            base_url: "xx".into(),
            language_code: "xx".into(),
        }];
        let (t, _) = select_track(&tracks, &Language::Auto).unwrap();
        assert_eq!(t.language_code, "xx");
    }

    #[test]
    fn specific_returns_exact_match() {
        let tracks = vec![CaptionTrack {
            base_url: "es".into(),
            language_code: "es".into(),
        }];
        let (t, note) = select_track(&tracks, &Language::Specific("es".into())).unwrap();
        assert_eq!(t.language_code, "es");
        assert!(note.is_none());
    }

    #[test]
    fn specific_falls_back_to_english_with_note() {
        let tracks = vec![CaptionTrack {
            base_url: "en".into(),
            language_code: "en".into(),
        }];
        let (t, note) = select_track(&tracks, &Language::Specific("de".into())).unwrap();
        assert_eq!(t.language_code, "en");
        assert!(note.as_deref().is_some_and(|n| n.contains("de")));
    }

    #[test]
    fn specific_errors_listing_available_when_nothing_matches() {
        let tracks = vec![CaptionTrack {
            base_url: "fr".into(),
            language_code: "fr".into(),
        }];
        let err = select_track(&tracks, &Language::Specific("de".into())).unwrap_err();
        assert!(err.to_string().contains("fr"));
    }

    #[test]
    fn errors_when_no_tracks() {
        assert!(matches!(
            select_track(&[], &Language::Auto),
            Err(TranscriptError::NoTranscriptAvailable)
        ));
    }

    // --- parse_transcript_xml ---

    #[test]
    fn parses_basic_xml() {
        let xml = r#"<?xml version="1.0" encoding="utf-8" ?><transcript>
<text start="0" dur="1">Hello world</text>
<text start="1" dur="2">This is a test</text>
</transcript>"#;
        assert_eq!(
            parse_transcript_xml(xml).unwrap(),
            "Hello world This is a test"
        );
    }

    #[test]
    fn unescapes_xml_entities() {
        let xml = r#"<?xml version="1.0"?><transcript>
<text start="0" dur="1">Hello &amp; world</text>
<text start="1" dur="1">It&apos;s &lt;fine&gt;</text>
</transcript>"#;
        assert_eq!(
            parse_transcript_xml(xml).unwrap(),
            "Hello & world It's <fine>"
        );
    }

    #[test]
    fn returns_empty_string_for_empty_transcript() {
        let xml = r#"<?xml version="1.0"?><transcript></transcript>"#;
        assert_eq!(parse_transcript_xml(xml).unwrap(), "");
    }

    #[test]
    fn extracts_audio_streams() {
        let json = r#"{
            "playabilityStatus": {"status": "OK"},
            "streamingData": {
                "adaptiveFormats": [
                    {"mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "url": "https://example.com/audio.mp4", "averageBitrate": 128000},
                    {"mimeType": "video/mp4; codecs=\"avc1\"", "url": "https://example.com/video.mp4", "averageBitrate": 500000},
                    {"mimeType": "audio/webm; codecs=\"opus\"", "url": "https://example.com/audio.webm", "averageBitrate": 64000}
                ]
            }
        }"#;
        let data = parse_innertube_response(json).unwrap();
        assert_eq!(data.audio_streams.len(), 2);
        assert!(data.audio_streams[0].mime_type.contains("mp4")); // mp4 sorted first
        assert_eq!(data.audio_streams[0].bitrate, 128000);
    }

    #[test]
    fn parses_innertube_p_element_xml() {
        let xml = r#"<?xml version="1.0" encoding="utf-8" ?><timedtext format="3"><body>
<p t="1360" d="1680">Hello world</p>
<p t="3000" d="2000">This is a test</p>
</body></timedtext>"#;
        assert_eq!(parse_transcript_xml(xml).unwrap(), "Hello world This is a test");
    }
}
