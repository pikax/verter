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
    /// Invalid named-import variant tag.
    InvalidNamedImportKind(String),
    /// Invalid `requestedMode` compile-cache-mode string.
    InvalidCompileCacheMode(String),
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
            Self::InvalidNamedImportKind(v) => write!(
                f,
                "invalid named-import kind '{v}' (expected 'default', 'named', or 'namespace')"
            ),
            Self::InvalidCompileCacheMode(v) => write!(
                f,
                "invalid requestedMode '{v}' (expected 'stateless', 'content', or 'session')"
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
