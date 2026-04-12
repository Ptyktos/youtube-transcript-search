use crate::error::TranscriptError;
use crate::language::{Language, AUTO_DETECT_ORDER};
use crate::url::VideoId;
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::Deserialize;

pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// Returns the `YouTube` watch page URL for a given video ID.
#[must_use]
pub fn watch_url(video_id: &VideoId) -> String {
    format!("https://www.youtube.com/watch?v={}", video_id.as_str())
}

/// Returns the `User-Agent` string used for all `YouTube` requests.
#[must_use]
pub fn user_agent() -> &'static str {
    USER_AGENT
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

// Internal serde shapes for ytInitialPlayerResponse
#[derive(Deserialize)]
struct PlayerResponse {
    captions: Option<CaptionsContainer>,
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

/// Extract the list of available caption tracks from a raw `YouTube` watch page HTML body.
///
/// Returns an empty `Vec` (not an error) when the video has no captions at all.
///
/// # Errors
/// Returns `TranscriptError::Parse` if `ytInitialPlayerResponse` is not found or malformed.
pub fn parse_youtube_page(html: &str) -> Result<Vec<CaptionTrack>, TranscriptError> {
    const MARKER: &str = "var ytInitialPlayerResponse = ";

    let start = html.find(MARKER).ok_or_else(|| {
        TranscriptError::Parse("Could not find ytInitialPlayerResponse in page".into())
    })?;

    let after = &html[start + MARKER.len()..];

    let end = after
        .find(";</script>")
        .or_else(|| after.find(";var "))
        .or_else(|| after.find(";\n"))
        .unwrap_or(after.len());

    let response: PlayerResponse = serde_json::from_str(&after[..end])
        .map_err(|e| TranscriptError::Parse(format!("Failed to decode player response: {e}")))?;

    Ok(response
        .captions
        .and_then(|c| c.renderer)
        .and_then(|r| r.caption_tracks)
        .unwrap_or_default())
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
/// `XML` entities (`&amp;`, `&lt;`, etc.) are unescaped automatically by `quick-xml`.
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
            Ok(Event::Start(ref e)) if e.name().as_ref() == b"text" => {
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
            Ok(Event::End(ref e)) if e.name().as_ref() == b"text" => {
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

    // --- parse_youtube_page ---

    #[test]
    fn extracts_caption_tracks_from_page() {
        let html = concat!(
            "<html><script>",
            r#"var ytInitialPlayerResponse = {"captions":{"playerCaptionsTracklistRenderer":{"captionTracks":[{"baseUrl":"https://example.com/caps","languageCode":"en"}]}}};"#,
            "</script></html>"
        );
        let tracks = parse_youtube_page(html).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].language_code, "en");
        assert_eq!(tracks[0].base_url, "https://example.com/caps");
    }

    #[test]
    fn returns_empty_when_no_captions_key() {
        let html = concat!(
            "<html><script>",
            r#"var ytInitialPlayerResponse = {"videoDetails":{"videoId":"abc"}};"#,
            "</script></html>"
        );
        let tracks = parse_youtube_page(html).unwrap();
        assert!(tracks.is_empty());
    }

    #[test]
    fn errors_when_marker_absent() {
        assert!(parse_youtube_page("<html><script>var something = {};</script></html>").is_err());
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
}
