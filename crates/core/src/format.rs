//! Render transcripts in different output formats.
//!
//! The [`OutputFormat::Json`] and [`OutputFormat::Markdown`] renderers embed a
//! deep link of the form `https://www.youtube.com/watch?v=<id>&t=<secs>s` for
//! every cue, so a reader can click a timestamp and jump to that moment in the
//! video. [`OutputFormat::Srt`] and [`OutputFormat::Vtt`] emit standard
//! subtitle files instead (their grammars have no notion of a hyperlink).

use crate::url::VideoId;
use crate::youtube::Segment;
use std::fmt::Write as _;

/// Selects how [`format_transcript`] renders a transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Plain text — every cue joined by a single space (the default).
    #[default]
    Text,
    /// A JSON document: `{ video_id, language, note, segments: [{ start, dur, text, url }] }`.
    Json,
    /// `SubRip` (`.srt`) subtitles.
    Srt,
    /// `WebVTT` (`.vtt`) subtitles.
    Vtt,
    /// A Markdown list with a clickable timestamp link per cue.
    Markdown,
}

impl std::str::FromStr for OutputFormat {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim().to_lowercase().as_str() {
            "json" => OutputFormat::Json,
            "srt" => OutputFormat::Srt,
            "vtt" | "webvtt" => OutputFormat::Vtt,
            "md" | "markdown" => OutputFormat::Markdown,
            _ => OutputFormat::Text,
        })
    }
}

impl OutputFormat {
    /// The HTTP `Content-Type` that best matches this format.
    #[must_use]
    pub fn content_type(self) -> &'static str {
        match self {
            OutputFormat::Json => "application/json; charset=utf-8",
            OutputFormat::Vtt => "text/vtt; charset=utf-8",
            OutputFormat::Markdown => "text/markdown; charset=utf-8",
            OutputFormat::Text | OutputFormat::Srt => "text/plain; charset=utf-8",
        }
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "caption offsets are small and non-negative; milliseconds fit comfortably in u64"
)]
fn to_millis(secs: f64) -> u64 {
    (secs.max(0.0) * 1000.0).round() as u64
}

fn whole_seconds(secs: f64) -> u64 {
    to_millis(secs) / 1000
}

/// A deep link that opens the video at `secs` seconds.
fn timestamp_url(video_id: &VideoId, secs: f64) -> String {
    format!(
        "https://www.youtube.com/watch?v={}&t={}s",
        video_id.as_str(),
        whole_seconds(secs)
    )
}

/// `M:SS`, widening to `H:MM:SS` past the hour — the label shown in Markdown links.
fn clock(secs: f64) -> String {
    let total = whole_seconds(secs);
    let (s, m, h) = (total % 60, (total / 60) % 60, total / 3600);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// `HH:MM:SS,mmm` (SRT, `sep = ','`) or `HH:MM:SS.mmm` (VTT, `sep = '.'`).
fn stamp(secs: f64, sep: char) -> String {
    let ms_total = to_millis(secs);
    let ms = ms_total % 1000;
    let s = (ms_total / 1000) % 60;
    let m = (ms_total / 60_000) % 60;
    let h = ms_total / 3_600_000;
    format!("{h:02}:{m:02}:{s:02}{sep}{ms:03}")
}

/// Render `segments` for `video_id` in the requested `format`.
///
/// `note` is an optional advisory (e.g. a language-fallback message). It is
/// surfaced in every format except SRT, whose grammar has no comment syntax.
#[must_use]
pub fn format_transcript(
    segments: &[Segment],
    video_id: &VideoId,
    language: &str,
    note: Option<&str>,
    format: OutputFormat,
) -> String {
    match format {
        OutputFormat::Text => {
            let body = segments
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            match note {
                Some(n) => format!("[{n}]\n\n{body}"),
                None => body,
            }
        }
        OutputFormat::Markdown => {
            let mut out = String::new();
            if let Some(n) = note {
                let _ = writeln!(out, "> _{n}_");
                let _ = writeln!(out);
            }
            for s in segments {
                let _ = writeln!(
                    out,
                    "- [{}]({}) {}",
                    clock(s.start),
                    timestamp_url(video_id, s.start),
                    s.text
                );
            }
            out
        }
        OutputFormat::Json => {
            let cues: Vec<serde_json::Value> = segments
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "start": s.start,
                        "dur": s.dur,
                        "text": s.text,
                        "url": timestamp_url(video_id, s.start),
                    })
                })
                .collect();
            let doc = serde_json::json!({
                "video_id": video_id.as_str(),
                "language": language,
                "note": note,
                "segments": cues,
            });
            serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string())
        }
        OutputFormat::Srt => {
            let mut out = String::new();
            for (i, s) in segments.iter().enumerate() {
                let _ = writeln!(
                    out,
                    "{}\n{} --> {}\n{}",
                    i + 1,
                    stamp(s.start, ','),
                    stamp(s.start + s.dur, ','),
                    s.text
                );
                let _ = writeln!(out);
            }
            out
        }
        OutputFormat::Vtt => {
            let mut out = String::from("WEBVTT\n\n");
            if let Some(n) = note {
                let _ = writeln!(out, "NOTE {n}");
                let _ = writeln!(out);
            }
            for s in segments {
                let _ = writeln!(
                    out,
                    "{} --> {}\n{}",
                    stamp(s.start, '.'),
                    stamp(s.start + s.dur, '.'),
                    s.text
                );
                let _ = writeln!(out);
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vid() -> VideoId {
        VideoId::parse("dQw4w9WgXcQ").expect("valid id")
    }

    fn sample() -> Vec<Segment> {
        vec![
            Segment {
                start: 0.0,
                dur: 1.5,
                text: "Hello world".into(),
            },
            Segment {
                start: 65.25,
                dur: 2.0,
                text: "second line".into(),
            },
        ]
    }

    #[test]
    fn text_joins_with_spaces() {
        let out = format_transcript(&sample(), &vid(), "en", None, OutputFormat::Text);
        assert_eq!(out, "Hello world second line");
    }

    #[test]
    fn text_prepends_note() {
        let out = format_transcript(
            &sample(),
            &vid(),
            "en",
            Some("fell back"),
            OutputFormat::Text,
        );
        assert!(out.starts_with("[fell back]\n\n"));
    }

    #[test]
    fn markdown_has_clickable_timestamp_links() {
        let out = format_transcript(&sample(), &vid(), "en", None, OutputFormat::Markdown);
        assert!(
            out.contains("- [0:00](https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=0s) Hello world")
        );
        // 65.25s -> label 1:05, link rounds down to whole seconds.
        assert!(
            out.contains("- [1:05](https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=65s) second line")
        );
    }

    #[test]
    fn json_embeds_per_cue_urls() {
        let out = format_transcript(&sample(), &vid(), "en", None, OutputFormat::Json);
        let doc: serde_json::Value = serde_json::from_str(&out).expect("valid json");
        assert_eq!(doc["video_id"], "dQw4w9WgXcQ");
        assert_eq!(doc["language"], "en");
        assert!(doc["note"].is_null());
        assert_eq!(doc["segments"][0]["text"], "Hello world");
        assert_eq!(
            doc["segments"][1]["url"],
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=65s"
        );
    }

    #[test]
    fn srt_uses_comma_and_one_based_index() {
        let out = format_transcript(&sample(), &vid(), "en", None, OutputFormat::Srt);
        assert!(out.starts_with("1\n00:00:00,000 --> 00:00:01,500\nHello world\n\n"));
        assert!(out.contains("2\n00:01:05,250 --> 00:01:07,250\nsecond line"));
    }

    #[test]
    fn vtt_has_header_dot_separator_and_note() {
        let out = format_transcript(
            &sample(),
            &vid(),
            "en",
            Some("fell back"),
            OutputFormat::Vtt,
        );
        assert!(out.starts_with("WEBVTT\n\nNOTE fell back\n\n"));
        assert!(out.contains("00:00:00.000 --> 00:00:01.500\nHello world"));
    }

    #[test]
    fn format_parses_aliases_and_defaults() {
        assert_eq!("json".parse(), Ok(OutputFormat::Json));
        assert_eq!("MD".parse(), Ok(OutputFormat::Markdown));
        assert_eq!("webvtt".parse(), Ok(OutputFormat::Vtt));
        assert_eq!("".parse(), Ok(OutputFormat::Text));
        assert_eq!("nonsense".parse(), Ok(OutputFormat::Text));
    }
}
