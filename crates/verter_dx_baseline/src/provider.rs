//! Provider discovery, strict tool-root enforcement, and the `TypeProvider`
//! spawn wrapper.
//!
//! This is the only place tsgo/tsserver discovery lives — the TS runner never
//! re-implements it. The strict-CI contract here is narrower than
//! `verter_type_runtime::discovery::find_tsserver`, which would happily fall
//! back to a workspace or global-npm TypeScript: the bridge passes the explicit
//! `--tsdk` into discovery and then REFUSES any discovered path that is not
//! exactly `expected_tsserver_js`, so a baseline run can never silently drift
//! onto an ambient TypeScript.

use std::path::Path;
use std::sync::Arc;

use verter_span::path::canonicalize_path;
use verter_type_runtime::protocol::TypeProviderError;
use verter_type_runtime::tsgo::TsgoTypeProvider;
use verter_type_runtime::tsserver::TsserverTypeProvider;
use verter_type_runtime::TypeProvider;

use crate::protocol::{ErrorKind, ProviderName, ToolRoot};

/// A failure resolving or enforcing the pinned tool root.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProviderInitError {
    /// A required `toolRoot` field was absent in strict CI.
    #[error("missing required tool-root field: {0}")]
    MissingToolRootField(&'static str),
    /// A required tool (tsgo / node / tsserver.js) could not be found.
    #[error("required tool not found: {0}")]
    ToolNotFound(&'static str),
    /// `expected_tsserver_js` does not exist on disk.
    #[error("expected tsserver.js does not exist: {0}")]
    ExpectedMissing(String),
    /// Discovery resolved a tsserver.js other than `expected_tsserver_js`.
    #[error("tsserver path mismatch: expected {expected}, discovered {discovered}")]
    PathMismatch {
        expected: String,
        discovered: String,
    },
}

impl ProviderInitError {
    /// Map to the wire error kind.
    pub fn kind(&self) -> ErrorKind {
        match self {
            ProviderInitError::PathMismatch { .. } => ErrorKind::BaselineToolRootMismatch,
            _ => ErrorKind::BaselineToolRootMissing,
        }
    }
}

/// How to spawn the resolved provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnPlan {
    Tsgo { bin: String },
    Tsserver { node: String, tsserver_js: String },
}

/// Outcome of tool-root resolution under the strict/non-strict policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// The provider is ready to spawn against `tool_root_used`.
    Ready {
        tool_root_used: String,
        plan: SpawnPlan,
    },
    /// A non-strict run gracefully declined to run the baseline, with a reason.
    Skipped { reason: String },
}

/// Refuse a discovered tsserver.js that is not exactly `expected`.
///
/// Both paths are canonicalized (slash/drive-case normalized) before
/// comparison. This is the gate that rejects an ambient workspace/global-npm
/// `tsserver.js`.
pub fn enforce_tsserver_path_match(
    expected: &str,
    discovered: &str,
) -> Result<(), ProviderInitError> {
    let e = canonicalize_path(expected);
    let d = canonicalize_path(discovered);
    if e == d {
        Ok(())
    } else {
        Err(ProviderInitError::PathMismatch {
            expected: e,
            discovered: d,
        })
    }
}

/// Resolve a tsgo binary.
///
/// An explicit, existing `tsgo_bin` always wins. The treatment of an
/// absent/invalid pin then mirrors the strict tsserver path-pinning:
///
/// - Strict CI requires the pinned path. An absent `tsgo_bin` is a missing
///   tool-root field; a pinned path that does not exist on disk is
///   `ExpectedMissing`. Strict mode NEVER falls back to discovery — a baseline
///   run can no more silently drift onto an ambient tsgo than onto an ambient
///   tsserver.
/// - Non-strict keeps the lenient fallback: an absent/invalid pin falls back to
///   the injected discovery so local dev is not blocked.
pub fn resolve_tsgo_with(
    tsgo_bin: Option<&str>,
    strict: bool,
    discover_tsgo: &dyn Fn() -> Option<String>,
) -> Result<String, ProviderInitError> {
    if let Some(bin) = tsgo_bin {
        if Path::new(bin).exists() {
            return Ok(bin.to_string());
        }
        // A pinned-but-absent path: strict refuses the discovery fallback.
        if strict {
            return Err(ProviderInitError::ExpectedMissing(bin.to_string()));
        }
    } else if strict {
        // Strict CI requires the pin to be present.
        return Err(ProviderInitError::MissingToolRootField("tsgoBin"));
    }
    match discover_tsgo() {
        Some(found) => Ok(found),
        None => Err(ProviderInitError::ToolNotFound("tsgo")),
    }
}

/// Resolve the `node` executable through the injected discovery.
pub fn require_node(
    discover_node: &dyn Fn() -> Option<String>,
) -> Result<String, ProviderInitError> {
    discover_node().ok_or(ProviderInitError::ToolNotFound("node"))
}

/// Resolve + enforce the tsserver tool root.
///
/// `discover_tsserver(tsdk, workspace_root)` mirrors
/// `find_tsserver(Some(tsdk), Some(ws))`. The result must match
/// `expected_tsserver_js` exactly.
#[allow(clippy::too_many_arguments)]
pub fn resolve_tsserver_with(
    tool_root: &ToolRoot,
    workspace_root: &str,
    strict: bool,
    discover_node: &dyn Fn() -> Option<String>,
    discover_tsserver: &dyn Fn(&str, &str) -> Option<String>,
) -> Result<(String, SpawnPlan), ProviderInitError> {
    let tsdk = tool_root
        .tsserver_tsdk
        .as_deref()
        .ok_or(ProviderInitError::MissingToolRootField("tsserverTsdk"))?;
    let expected = tool_root.expected_tsserver_js.as_deref().ok_or(
        ProviderInitError::MissingToolRootField("expectedTsserverJs"),
    )?;

    let node = require_node(discover_node)?;

    // Strict CI asserts the pinned tsserver.js exists before the bridge starts.
    if strict && !Path::new(expected).exists() {
        return Err(ProviderInitError::ExpectedMissing(expected.to_string()));
    }

    // Pass the explicit tsdk into discovery, then refuse anything but `expected`.
    match discover_tsserver(tsdk, workspace_root) {
        Some(discovered) => enforce_tsserver_path_match(expected, &discovered)?,
        None => return Err(ProviderInitError::ToolNotFound("tsserver.js")),
    }

    Ok((
        canonicalize_path(expected),
        SpawnPlan::Tsserver {
            node,
            tsserver_js: expected.to_string(),
        },
    ))
}

/// Resolve a provider under the strict/non-strict policy with injected
/// discovery (the testable core).
///
/// Strict: any tool-root problem is a hard failure (`Err`). Non-strict: the same
/// problem becomes `Ok(Resolution::Skipped { reason })`, recording why the
/// baseline did not run — never a silent pass.
pub fn resolve_with(
    provider: ProviderName,
    tool_root: &ToolRoot,
    workspace_root: &str,
    strict: bool,
    discover_node: &dyn Fn() -> Option<String>,
    discover_tsgo: &dyn Fn() -> Option<String>,
    discover_tsserver: &dyn Fn(&str, &str) -> Option<String>,
) -> Result<Resolution, ProviderInitError> {
    let inner: Result<(String, SpawnPlan), ProviderInitError> = match provider {
        ProviderName::Tsgo => {
            resolve_tsgo_with(tool_root.tsgo_bin.as_deref(), strict, discover_tsgo)
                .map(|bin| (canonicalize_path(&bin), SpawnPlan::Tsgo { bin }))
        }
        ProviderName::Tsserver => resolve_tsserver_with(
            tool_root,
            workspace_root,
            strict,
            discover_node,
            discover_tsserver,
        ),
    };

    match inner {
        Ok((tool_root_used, plan)) => Ok(Resolution::Ready {
            tool_root_used,
            plan,
        }),
        Err(e) if strict => Err(e),
        Err(e) => Ok(Resolution::Skipped {
            reason: format!("{} baseline skipped (non-strict): {e}", provider.as_str()),
        }),
    }
}

/// Production resolution: wires the real `verter_type_runtime` discovery.
pub fn resolve(
    provider: ProviderName,
    tool_root: &ToolRoot,
    workspace_root: &str,
    strict: bool,
) -> Result<Resolution, ProviderInitError> {
    resolve_with(
        provider,
        tool_root,
        workspace_root,
        strict,
        &|| verter_type_runtime::find_node(),
        &|| verter_type_runtime::tsgo::find_tsgo_binary().ok(),
        &|tsdk, ws| {
            verter_type_runtime::find_tsserver(Some(tsdk), Some(ws))
                .map(|p| p.to_string_lossy().to_string())
        },
    )
}

/// Spawn the resolved provider. `workspace_root` is a filesystem path.
pub async fn spawn(
    provider: ProviderName,
    plan: &SpawnPlan,
    workspace_root: &str,
) -> Result<Arc<dyn TypeProvider>, TypeProviderError> {
    match (provider, plan) {
        (ProviderName::Tsgo, SpawnPlan::Tsgo { bin }) => {
            let root_uri = verter_type_runtime::path_to_file_uri_string(workspace_root);
            let provider = TsgoTypeProvider::spawn(bin, &root_uri).await?;
            Ok(Arc::new(provider))
        }
        (ProviderName::Tsserver, SpawnPlan::Tsserver { node, tsserver_js }) => {
            let provider =
                TsserverTypeProvider::spawn(node, tsserver_js, workspace_root, None, None).await?;
            Ok(Arc::new(provider))
        }
        // The plan is always built for the requested provider in `resolve_with`.
        _ => Err(TypeProviderError::new(
            "internal: spawn plan does not match provider",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_root_tsserver(tsdk: Option<&str>, expected: Option<&str>) -> ToolRoot {
        ToolRoot {
            tsserver_tsdk: tsdk.map(String::from),
            expected_tsserver_js: expected.map(String::from),
            tsserver_version: Some("5.7.2".to_string()),
            tsgo_bin: None,
        }
    }

    // ── path-match enforcement (the global-npm rejection proof) ────────────

    #[test]
    fn matching_tsserver_paths_are_accepted() {
        assert!(enforce_tsserver_path_match(
            "/repo/node_modules/typescript/lib/tsserver.js",
            "/repo/node_modules/typescript/lib/tsserver.js",
        )
        .is_ok());
    }

    #[test]
    fn ambient_global_npm_tsserver_is_rejected() {
        // Discovery resolved a global-npm tsserver.js; expected is the pinned
        // repo tsdk. The mismatch must be refused — proof the bridge never
        // silently accepts an ambient global TypeScript.
        let err = enforce_tsserver_path_match(
            "/repo/node_modules/typescript/lib/tsserver.js",
            "/usr/local/lib/node_modules/typescript/lib/tsserver.js",
        )
        .unwrap_err();
        assert!(matches!(err, ProviderInitError::PathMismatch { .. }));
        assert_eq!(err.kind(), ErrorKind::BaselineToolRootMismatch);
    }

    // ── strict failures ────────────────────────────────────────────────────

    #[test]
    fn strict_missing_tsserver_tool_root_fields_fail() {
        let no_node = || None;
        let no_tsgo = || None;
        let no_ts = |_: &str, _: &str| None;

        let err = resolve_with(
            ProviderName::Tsserver,
            &tool_root_tsserver(None, None),
            "/ws",
            true,
            &no_node,
            &no_tsgo,
            &no_ts,
        )
        .unwrap_err();
        assert!(matches!(err, ProviderInitError::MissingToolRootField(_)));
        assert_eq!(err.kind(), ErrorKind::BaselineToolRootMissing);
    }

    #[test]
    fn strict_missing_node_fails() {
        let no_node = || None;
        // tsserver fields present; node missing.
        let err = resolve_tsserver_with(
            &tool_root_tsserver(Some("/repo/tsdk"), Some("/repo/tsdk/tsserver.js")),
            "/ws",
            false, // skip the existence assert so we exercise the node gate
            &no_node,
            &|_, _| Some("/repo/tsdk/tsserver.js".to_string()),
        )
        .unwrap_err();
        assert_eq!(err, ProviderInitError::ToolNotFound("node"));
    }

    #[test]
    fn strict_missing_tsgo_fails() {
        let some_node = || Some("/usr/bin/node".to_string());
        let no_tsgo = || None;
        let no_ts = |_: &str, _: &str| None;
        let err = resolve_with(
            ProviderName::Tsgo,
            &ToolRoot::default(),
            "/ws",
            true,
            &some_node,
            &no_tsgo,
            &no_ts,
        )
        .unwrap_err();
        // Strict CI with no pinned tsgoBin is a missing-tool-root field — the pin
        // is required, never discovered (mirrors strict tsserver pinning).
        assert_eq!(err, ProviderInitError::MissingToolRootField("tsgoBin"));
        assert_eq!(err.kind(), ErrorKind::BaselineToolRootMissing);
    }

    #[test]
    fn strict_mismatched_expected_tsserver_is_rejected() {
        // Real temp tsdk exists, discovery returns a DIFFERENT existing path.
        let dir = tempfile::tempdir().unwrap();
        let expected = dir.path().join("tsserver.js");
        std::fs::write(&expected, "// tsserver").unwrap();
        let other = dir.path().join("other_tsserver.js");
        std::fs::write(&other, "// other").unwrap();

        let expected_s = expected.to_string_lossy().to_string();
        let other_s = other.to_string_lossy().to_string();

        let err = resolve_tsserver_with(
            &tool_root_tsserver(
                Some(dir.path().to_string_lossy().as_ref()),
                Some(&expected_s),
            ),
            "/ws",
            true,
            &|| Some("/usr/bin/node".to_string()),
            &|_, _| Some(other_s.clone()),
        )
        .unwrap_err();
        assert!(matches!(err, ProviderInitError::PathMismatch { .. }));
    }

    #[test]
    fn strict_matching_tsserver_tool_root_is_ready() {
        let dir = tempfile::tempdir().unwrap();
        let expected = dir.path().join("tsserver.js");
        std::fs::write(&expected, "// tsserver").unwrap();
        let expected_s = expected.to_string_lossy().to_string();
        let disc = expected_s.clone();

        let (used, plan) = resolve_tsserver_with(
            &tool_root_tsserver(
                Some(dir.path().to_string_lossy().as_ref()),
                Some(&expected_s),
            ),
            "/ws",
            true,
            &|| Some("/usr/bin/node".to_string()),
            &|_, _| Some(disc.clone()),
        )
        .unwrap();
        assert_eq!(used, canonicalize_path(&expected_s));
        assert!(matches!(plan, SpawnPlan::Tsserver { .. }));
    }

    // ── non-strict skip-with-reason ──────────────────────────────────────────

    #[test]
    fn non_strict_missing_provider_skips_with_recorded_reason() {
        let no_node = || None;
        let no_tsgo = || None;
        let no_ts = |_: &str, _: &str| None;
        let res = resolve_with(
            ProviderName::Tsgo,
            &ToolRoot::default(),
            "/ws",
            false,
            &no_node,
            &no_tsgo,
            &no_ts,
        )
        .unwrap();
        match res {
            Resolution::Skipped { reason } => {
                assert!(
                    reason.contains("tsgo"),
                    "reason must record the tool: {reason}"
                );
                assert!(reason.contains("non-strict"), "reason: {reason}");
            }
            Resolution::Ready { .. } => panic!("expected skip in non-strict with no tsgo"),
        }
    }

    // ── strict tsgo pinning refuses the discovery fallback ───────────────────

    #[test]
    fn strict_tsgo_refuses_discovery_fallback_for_invalid_pinned_bin() {
        // A pinned tsgo path that does NOT exist must hard-error in strict mode,
        // never silently fall back to an ambient discovered tsgo (mirrors the
        // strict tsserver path-pinning).
        let tool_root = ToolRoot {
            tsgo_bin: Some("/nonexistent/pinned/tsgo".to_string()),
            ..ToolRoot::default()
        };
        let err = resolve_with(
            ProviderName::Tsgo,
            &tool_root,
            "/ws",
            true, // strict
            &|| Some("/usr/bin/node".to_string()),
            &|| Some("/somewhere/else/tsgo".to_string()), // discovery WOULD succeed
            &|_, _| None,
        )
        .unwrap_err();
        // Pinned-but-absent → ExpectedMissing, NOT a fallback to the ambient tsgo.
        assert!(
            matches!(err, ProviderInitError::ExpectedMissing(_)),
            "strict mode must refuse the discovery fallback for an invalid pinned tsgo; got {err:?}"
        );
        assert_eq!(err.kind(), ErrorKind::BaselineToolRootMissing);
    }

    #[test]
    fn strict_tsgo_missing_pinned_field_refuses_discovery_fallback() {
        // No pinned tsgoBin at all in strict mode is a missing-tool-root field,
        // not a discovery fallback.
        let err = resolve_with(
            ProviderName::Tsgo,
            &ToolRoot::default(),
            "/ws",
            true, // strict
            &|| Some("/usr/bin/node".to_string()),
            &|| Some("/discovered/tsgo".to_string()), // discovery WOULD succeed
            &|_, _| None,
        )
        .unwrap_err();
        assert_eq!(
            err,
            ProviderInitError::MissingToolRootField("tsgoBin"),
            "strict mode with no pinned tsgoBin must be a missing-field error"
        );
        assert_eq!(err.kind(), ErrorKind::BaselineToolRootMissing);
    }

    #[test]
    fn non_strict_tsgo_invalid_pinned_bin_falls_back_to_discovery() {
        // Non-strict keeps the lenient fallback: an invalid pinned bin still
        // discovers an ambient tsgo so local dev is not blocked.
        let tool_root = ToolRoot {
            tsgo_bin: Some("/nonexistent/pinned/tsgo".to_string()),
            ..ToolRoot::default()
        };
        let res = resolve_with(
            ProviderName::Tsgo,
            &tool_root,
            "/ws",
            false, // non-strict
            &|| Some("/usr/bin/node".to_string()),
            &|| Some("/discovered/tsgo".to_string()),
            &|_, _| None,
        )
        .unwrap();
        match res {
            Resolution::Ready { plan, .. } => {
                assert_eq!(
                    plan,
                    SpawnPlan::Tsgo {
                        bin: "/discovered/tsgo".to_string()
                    }
                );
            }
            Resolution::Skipped { .. } => panic!("non-strict should discover a fallback tsgo"),
        }
    }

    #[test]
    fn ready_tsgo_uses_explicit_existing_bin_over_discovery() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("tsgo");
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        let bin_s = bin.to_string_lossy().to_string();

        let tool_root = ToolRoot {
            tsgo_bin: Some(bin_s.clone()),
            ..ToolRoot::default()
        };
        // Discovery would return something else, but the explicit existing bin wins.
        let res = resolve_with(
            ProviderName::Tsgo,
            &tool_root,
            "/ws",
            true,
            &|| None,
            &|| Some("/somewhere/else/tsgo".to_string()),
            &|_, _| None,
        )
        .unwrap();
        match res {
            Resolution::Ready { plan, .. } => {
                assert_eq!(plan, SpawnPlan::Tsgo { bin: bin_s });
            }
            Resolution::Skipped { .. } => panic!("explicit bin should be ready"),
        }
    }
}
