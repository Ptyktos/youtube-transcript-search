use crate::error::TranscriptError;
use url::Url;

const VALID_HOSTS: &[&str] = &[
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "youtu.be",
    "youtube.co.uk",
    "youtube.de",
    "youtube.fr",
    "youtube.jp",
    "youtube.ca",
    "youtube.es",
    "youtube.com.br",
    "youtube.co.in",
    "youtube.co.kr",
];

/// A validated `YouTube` video ID — exactly 11 chars matching `[A-Za-z0-9_-]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VideoId(String);

impl VideoId {
    /// Parse a raw string into a validated `VideoId`.
    ///
    /// # Errors
    ///
    /// Returns `InvalidVideoId` if the string is not exactly 11 `[A-Za-z0-9_-]` characters.
    pub fn parse(s: &str) -> Result<Self, TranscriptError> {
        if s.len() == 11
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            Ok(VideoId(s.to_string()))
        } else {
            Err(TranscriptError::InvalidVideoId)
        }
    }

    /// Return the inner video ID string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for VideoId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Extract and validate a `YouTube` video ID from any supported URL format.
///
/// Accepts `watch?v=`, `youtu.be/`, `/shorts/`, `/live/`, `/embed/` paths,
/// with or without scheme, across all supported `YouTube` TLDs.
///
/// # Errors
///
/// Returns `InvalidUrl` if the URL cannot be parsed, the host is not a recognised
/// `YouTube` domain, or no video ID path segment is present.
/// Returns `InvalidVideoId` if the extracted ID fails validation.
pub fn extract_video_id(raw: &str) -> Result<VideoId, TranscriptError> {
    let normalized = if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };

    let parsed = Url::parse(&normalized).map_err(|_| TranscriptError::InvalidUrl)?;
    let host = parsed.host_str().unwrap_or("");
    let bare = host.strip_prefix("www.").unwrap_or(host);

    if !VALID_HOSTS.contains(&host) && !VALID_HOSTS.contains(&bare) {
        return Err(TranscriptError::InvalidUrl);
    }

    let id_str = if host == "youtu.be" || bare == "youtu.be" {
        parsed
            .path_segments()
            .and_then(|mut s| s.next())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    } else {
        let path = parsed.path();
        if path.starts_with("/watch") {
            parsed
                .query_pairs()
                .find(|(k, _)| k == "v")
                .map(|(_, v)| v.into_owned())
        } else if path.starts_with("/shorts/")
            || path.starts_with("/live/")
            || path.starts_with("/embed/")
        {
            path.split('/')
                .nth(2)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        } else {
            None
        }
    };

    id_str
        .ok_or(TranscriptError::InvalidUrl)
        .and_then(|id| VideoId::parse(&id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_standard_watch_url() {
        let id = extract_video_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ").unwrap();
        assert_eq!(id.as_str(), "dQw4w9WgXcQ");
    }

    #[test]
    fn extracts_short_url() {
        let id = extract_video_id("https://youtu.be/dQw4w9WgXcQ").unwrap();
        assert_eq!(id.as_str(), "dQw4w9WgXcQ");
    }

    #[test]
    fn extracts_shorts_path() {
        let id = extract_video_id("https://www.youtube.com/shorts/dQw4w9WgXcQ").unwrap();
        assert_eq!(id.as_str(), "dQw4w9WgXcQ");
    }

    #[test]
    fn extracts_live_path() {
        let id = extract_video_id("https://www.youtube.com/live/dQw4w9WgXcQ").unwrap();
        assert_eq!(id.as_str(), "dQw4w9WgXcQ");
    }

    #[test]
    fn extracts_embed_path() {
        let id = extract_video_id("https://www.youtube.com/embed/dQw4w9WgXcQ").unwrap();
        assert_eq!(id.as_str(), "dQw4w9WgXcQ");
    }

    #[test]
    fn strips_tracking_params() {
        let id = extract_video_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ&si=abc123&t=30s")
            .unwrap();
        assert_eq!(id.as_str(), "dQw4w9WgXcQ");
    }

    #[test]
    fn handles_mobile_subdomain() {
        let id = extract_video_id("https://m.youtube.com/watch?v=dQw4w9WgXcQ").unwrap();
        assert_eq!(id.as_str(), "dQw4w9WgXcQ");
    }

    #[test]
    fn handles_no_scheme() {
        let id = extract_video_id("youtu.be/dQw4w9WgXcQ").unwrap();
        assert_eq!(id.as_str(), "dQw4w9WgXcQ");
    }

    #[test]
    fn handles_international_domain() {
        let id = extract_video_id("https://youtube.co.uk/watch?v=dQw4w9WgXcQ").unwrap();
        assert_eq!(id.as_str(), "dQw4w9WgXcQ");
    }

    #[test]
    fn rejects_non_youtube_host() {
        assert!(extract_video_id("https://vimeo.com/12345").is_err());
    }

    #[test]
    fn rejects_youtube_channel_url() {
        assert!(extract_video_id("https://www.youtube.com/channel/UCxyz123").is_err());
    }

    #[test]
    fn rejects_invalid_video_id_length() {
        assert!(extract_video_id("https://youtu.be/short").is_err());
    }

    #[test]
    fn rejects_video_id_with_special_chars() {
        assert!(extract_video_id("https://youtu.be/dQw4w9W!cQ@").is_err());
    }
}
