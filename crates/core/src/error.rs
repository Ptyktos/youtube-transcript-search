use thiserror::Error;

#[derive(Debug, Error)]
pub enum TranscriptError {
    #[error("Invalid YouTube URL")]
    InvalidUrl,

    #[error("Invalid video ID: must be 11 alphanumeric/underscore/hyphen characters")]
    InvalidVideoId,

    #[error("No transcripts are available for this video")]
    NoTranscriptAvailable,

    #[error("Language '{requested}' not available; available languages: {}", available.join(", "))]
    LanguageNotAvailable {
        requested: String,
        available: Vec<String>,
    },

    #[error("Network error: {0}")]
    Network(String),

    #[error("Parse error: {0}")]
    Parse(String),
}
