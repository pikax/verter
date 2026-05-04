//! Typed error for FFI → host conversion failures.
//!
//! Each consumer crate (`verter_napi`, `verter_wasm`) converts these errors
//! into its native error type via the `Display` impl.

/// Typed error for FFI → host conversion failures.
#[derive(Debug, Clone)]
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
    /// Invalid virtual node `kind` string.
    InvalidNodeKind(String),
    /// Invalid `target` string.
    InvalidTarget(String),
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
            Self::InvalidNodeKind(v) => write!(f, "invalid virtual node kind '{v}'"),
            Self::InvalidTarget(v) => write!(
                f,
                "invalid target '{v}' (expected 'bundler', 'ide', 'analysis', or 'full')"
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
