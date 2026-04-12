pub const AUTO_DETECT_ORDER: &[&str] = &[
    "en", "es", "fr", "de", "tr", "pt", "ja", "ko", "zh", "it", "ru", "ar",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Language {
    Auto,
    Specific(String),
}

impl std::str::FromStr for Language {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim() {
            "" | "auto" => Language::Auto,
            other => Language::Specific(other.to_lowercase()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn auto_from_literal() {
        assert_eq!(Language::from_str("auto").unwrap(), Language::Auto);
    }

    #[test]
    fn auto_from_empty() {
        assert_eq!(Language::from_str("").unwrap(), Language::Auto);
    }

    #[test]
    fn specific_lowercased() {
        assert_eq!(
            Language::from_str("EN").unwrap(),
            Language::Specific("en".into())
        );
        assert_eq!(
            Language::from_str("Es").unwrap(),
            Language::Specific("es".into())
        );
    }

    #[test]
    fn auto_detect_order_starts_with_english() {
        assert_eq!(AUTO_DETECT_ORDER[0], "en");
    }

    #[test]
    fn auto_detect_order_has_twelve_entries() {
        assert_eq!(AUTO_DETECT_ORDER.len(), 12);
    }
}
