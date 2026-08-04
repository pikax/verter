//! Pure script-language classification.

use oxc_span::SourceType;

/// Script dialect recorded by the carrier parser.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ScriptLanguage {
    JavaScript,
    #[default]
    TypeScript,
    JSX,
    TSX,
    /// Unknown or esoteric language (for example `lang="coffee"`).
    Unknown,
}

impl ScriptLanguage {
    /// Classify an already-parsed `lang` attribute value.
    #[must_use]
    pub fn from_bytes(lang: &[u8]) -> Self {
        match lang {
            b"ts" | b"typescript" => Self::TypeScript,
            b"tsx" => Self::TSX,
            b"jsx" => Self::JSX,
            b"js" | b"javascript" => Self::JavaScript,
            _ => Self::Unknown,
        }
    }

    #[must_use]
    pub fn to_source_type(self) -> SourceType {
        match self {
            Self::JavaScript => SourceType::mjs(),
            Self::TypeScript => SourceType::ts(),
            Self::JSX => SourceType::jsx(),
            Self::TSX => SourceType::tsx(),
            Self::Unknown => SourceType::cjs(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_parser_owned_lang_values() {
        assert_eq!(
            ScriptLanguage::from_bytes(b"ts"),
            ScriptLanguage::TypeScript
        );
        assert_eq!(ScriptLanguage::from_bytes(b"tsx"), ScriptLanguage::TSX);
        assert_eq!(ScriptLanguage::from_bytes(b"jsx"), ScriptLanguage::JSX);
        assert_eq!(
            ScriptLanguage::from_bytes(b"js"),
            ScriptLanguage::JavaScript
        );
        assert_eq!(
            ScriptLanguage::from_bytes(b"coffee"),
            ScriptLanguage::Unknown
        );
    }
}
