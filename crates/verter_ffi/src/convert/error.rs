//! Typed error for FFI → host conversion failures.
//!
//! Each consumer crate (`verter_napi`, `verter_wasm`) converts these errors
//! into its native error type via the `Display` impl.

/// Typed error for FFI → host conversion failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FfiConversionError {
    /// Invalid `compileErrorPolicy` string.
    InvalidCompileErrorPolicy(String),
    /// Invalid `analysisLevel` string.
    InvalidAnalysisLevel(String),
    /// Invalid `hmrStrategy` string.
    InvalidHmrStrategy(String),
    /// `delimiters` array must have exactly 2 elements.
    InvalidDelimiters(usize),
    /// Invalid `file_kind` string.
    InvalidFileKind(String),
    /// `file_kind` was absent and the request carried no canonical path
    /// to classify.
    MissingFileLanguagePath,
    /// The path's extension is a project-gated candidate row; FFI-time
    /// classification is static-only, so an explicit `file_kind` string
    /// is required.
    GatedFileLanguageRequiresExplicitKind(String),
    /// Invalid virtual node `kind` string.
    InvalidNodeKind(String),
    /// Invalid `target` string.
    InvalidTarget(String),
    /// Invalid projection-mode tag.
    InvalidProjectionMode(String),
    /// Invalid `requestedMode` compile-cache-mode string.
    InvalidCompileCacheMode(String),
    /// Invalid `getPublicApi` mode string.
    InvalidPublicApiMode(String),
    /// An opaque block-content token failed its bounded wire-envelope check.
    InvalidBlockContentToken(&'static str),
    /// Invalid preprocessor-diagnostic `severity` string.
    InvalidPreprocessorDiagnosticSeverity(String),
    /// A span could not be converted exactly to its wire encoding (no source
    /// snapshot, off-boundary, past-end or inverted). Refused, never clamped.
    SpanConversion(&'static str),
    /// A request offset or position carried an unknown or missing encoding tag.
    InvalidEncodingTag(String),
    /// A request position is not a valid position of the content it indexes.
    InvalidPosition(&'static str),
}

impl std::fmt::Display for FfiConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCompileErrorPolicy(v) => {
                write!(
                    f,
                    "invalid compileErrorPolicy '{v}' (expected 'strict' or 'dev')"
                )
            }
            Self::InvalidAnalysisLevel(v) => {
                write!(
                    f,
                    "invalid analysisLevel '{v}' (expected 'none', 'essential', or 'full')"
                )
            }
            Self::InvalidHmrStrategy(v) => {
                write!(
                    f,
                    "invalid hmrStrategy '{v}' (expected 'vite', 'webpack', or 'none')"
                )
            }
            Self::InvalidDelimiters(len) => {
                write!(f, "delimiters must have exactly 2 elements, got {len}")
            }
            Self::InvalidFileKind(v) => write!(f, "invalid file_kind '{v}'"),
            Self::MissingFileLanguagePath => write!(
                f,
                "file_kind absent and no canonical path available to classify the file language"
            ),
            Self::GatedFileLanguageRequiresExplicitKind(path) => write!(
                f,
                "'{path}' matches a project-gated language row; FFI classification is \
                 static-only, pass an explicit file_kind"
            ),
            Self::InvalidNodeKind(v) => write!(f, "invalid virtual node kind '{v}'"),
            Self::InvalidTarget(v) => write!(
                f,
                "invalid target '{v}' (expected 'bundler', 'ide', 'analysis', or 'full')"
            ),
            Self::InvalidProjectionMode(v) => write!(
                f,
                "invalid projection mode '{v}' (expected 'identity', 'navigate', 'shallow', \
                 'expanded', or 'skeleton')"
            ),
            Self::InvalidCompileCacheMode(v) => write!(
                f,
                "invalid requestedMode '{v}' (expected 'stateless', 'content', or 'session')"
            ),
            Self::InvalidPublicApiMode(v) => write!(
                f,
                "invalid public api mode '{v}' (expected 'public', 'testing', or 'declaration')"
            ),
            Self::InvalidBlockContentToken(field) => {
                write!(f, "invalid sealed block-content token in '{field}'")
            }
            Self::SpanConversion(code) => {
                write!(f, "span cannot be converted to its wire encoding: {code}")
            }
            Self::InvalidEncodingTag(v) => write!(
                f,
                "invalid coordinate encoding tag '{v}' (expected 'utf8.v1' or 'utf16.v1')"
            ),
            Self::InvalidPosition(code) => write!(f, "invalid position: {code}"),
            Self::InvalidPreprocessorDiagnosticSeverity(v) => write!(
                f,
                "invalid preprocessor diagnostic severity '{v}' (expected 'error', 'warning', or 'info')"
            ),
        }
    }
}

impl std::error::Error for FfiConversionError {}

impl From<FfiConversionError> for String {
    fn from(e: FfiConversionError) -> String {
        e.to_string()
    }
}

impl From<super::wire_span::SpanConversionError> for FfiConversionError {
    fn from(error: super::wire_span::SpanConversionError) -> Self {
        Self::SpanConversion(error.code())
    }
}
