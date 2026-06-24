//! Feature-gated loading of an analysis-input config from disk.
//!
//! Parsing from an explicit string is always available ([`crate::parse_config`]).
//! Reading a real config FILE — from the `DX_HARNESS_EXTERNAL_CORPUS` env var or a
//! fixed default path — is the only I/O the campaign performs, and it is gated
//! behind the `local-analysis-corpus` feature so the canonical
//! `cargo nextest run --workspace` (feature off) NEVER touches the config. Only
//! opt-in analysis runners enable the feature.

/// The env var an opt-in runner sets (to the config path) to load a local analysis
/// corpus. Defined unconditionally so the name is a single source of truth shared
/// with the TS side; it is only READ under the `local-analysis-corpus` feature.
pub const ANALYSIS_CORPUS_ENV: &str = "DX_HARNESS_EXTERNAL_CORPUS";

#[cfg(feature = "local-analysis-corpus")]
mod gated {
    use std::path::Path;

    use crate::config::{parse_config, AnalysisProjects};
    use crate::error::AnalysisInputError;

    /// Read and parse a config from an explicit file path. Gated: only an opt-in
    /// runner reaches this. Any I/O failure becomes a PATH-FREE
    /// [`AnalysisInputError::Io`] (the path is retained privately, never formatted).
    pub fn load_from_file(path: &Path) -> Result<AnalysisProjects, AnalysisInputError> {
        let bytes = std::fs::read_to_string(path).map_err(|source| AnalysisInputError::Io {
            source,
            path: path.to_path_buf(),
        })?;
        parse_config(&bytes)
    }

    /// Load the config the `DX_HARNESS_EXTERNAL_CORPUS` env var points at. Returns
    /// [`AnalysisInputError::ConfigUnavailable`] when the var is unset, so a runner
    /// can distinguish "no corpus configured" from a malformed config.
    pub fn load_from_env() -> Result<AnalysisProjects, AnalysisInputError> {
        match std::env::var_os(super::ANALYSIS_CORPUS_ENV) {
            Some(p) if !p.is_empty() => load_from_file(Path::new(&p)),
            _ => Err(AnalysisInputError::ConfigUnavailable),
        }
    }
}

#[cfg(feature = "local-analysis-corpus")]
pub use gated::{load_from_env, load_from_file};

#[cfg(all(test, feature = "local-analysis-corpus"))]
mod tests {
    use super::*;

    #[test]
    fn load_from_file_parses_a_written_config() {
        // Write under the OS temp dir, never the repo.
        let dir = std::env::temp_dir().join(format!("verter-analysis-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("projects.local.json");
        std::fs::write(
            &path,
            r#"{
                "schema": "verter.analysis-projects.v1",
                "projects": [
                    { "id": "p0001", "root": "/path/to/p", "kind": "lib", "workstreams": ["build"] }
                ]
            }"#,
        )
        .unwrap();
        let cfg = load_from_file(&path).expect("written config loads");
        assert_eq!(cfg.projects()[0].id().as_str(), "p0001");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_a_path_free_io_error() {
        let path = std::env::temp_dir().join("verter-analysis-does-not-exist-zzz.json");
        let err = load_from_file(&path).expect_err("missing file errors");
        // The Display never spells the path.
        assert!(!format!("{err}").contains("verter-analysis-does-not-exist"));
    }
}
