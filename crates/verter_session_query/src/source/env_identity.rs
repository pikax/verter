//! The view-current source-environment identity of one canonical's artifact.

/// The view-current source-env identity of one canonical's artifact:
/// `parse_key` / `file_language_id` from the
/// [`verter_session_query::source::artifact_key::FileArtifactKey`] identity, plus the
/// canonical's LIVE `parse_env_hash` dimension (content validity stays
/// on the `FileWholeHash` rail). Snapshot value backing the strict
/// `FileSourceEnv` validation branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEnvIdentity {
    pub parse_env_hash: crate::facts::fact_cache::ParseEnvHash,
    pub parse_key: verter_language::ParseKey,
    pub file_language_id: verter_language::FileLanguage,
}
