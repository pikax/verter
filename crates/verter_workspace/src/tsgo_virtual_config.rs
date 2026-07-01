//! tsgo virtual-tsconfig MATERIALIZATION.
//!
//! When `verter_workspace`'s carrier-discovery decides a configured project must
//! be virtualized (its `include`/`files` do not enumerate the carrier companion
//! surface), the configured tsconfig is served to the tsgo `--api` engine with
//! the companion paths injected — Verter-computed, NEVER written to user disk.
//!
//! This module owns the two materialization concerns:
//!
//! 1. **Augmented bytes.** [`augment_tsconfig_bytes`] takes the user tsconfig
//!    JSON and the companion paths and produces the virtual config: byte-wise it
//!    is the user config with the companion paths added to `files`, and nothing
//!    else changed. The augmented bytes are served through the
//!    `verter_tsgo_api` overlay's `read_file` for the tsconfig path; a
//!    non-virtual project has no overlay entry and falls through to the real
//!    config.
//!
//! 2. **Diagnostic invisibility.** The injected companion roots are an
//!    implementation detail of membership — they must never surface in
//!    user-visible config-file-parse / options diagnostics. A virtualized config
//!    must produce the SAME config/options diagnostic set the user's real config
//!    would. [`strip_injected_root_diagnostics`] drops any config/options
//!    diagnostic that points at an injected companion path before it maps back.
//!
//! The `verter_tsgo_api` overlay seam stays policy-free: it serves whatever
//! bytes this module computes. The discovery DECISION and the virtual-config
//! IDENTITY live alongside this module in `verter_workspace`.

use std::collections::HashSet;
use std::sync::Arc;

use verter_span::path::InjectedPathKey;
use verter_tsgo_api::proto::types::Diagnostic;
use verter_tsgo_api::snapshot::{OverlaySnapshot, RealDirSource};

use crate::config::strip_json_comments;

/// The set of injected companion paths (the generated carriers + the synthetic
/// tsconfig) whose config/options diagnostics must be invisible, keyed by the
/// shared filesystem-identity key ([`InjectedPathKey`]) so membership folds the
/// SAME way the rest of the workspace compares two paths for file identity.
///
/// Building the set once and testing membership through it is what makes the
/// strip decision (here) and the diagnostic-homing decision (`verter_tsc`'s
/// `map_one`) share ONE path-identity notion — a companion the engine echoes in
/// a different separator / drive-letter / (on a case-insensitive FS) non-drive
/// case still folds to the same key and is correctly recognized, closing the
/// exact-string-equality leak.
#[derive(Debug, Clone, Default)]
pub struct InjectedPathSet {
    keys: HashSet<InjectedPathKey>,
}

impl InjectedPathSet {
    /// Build the set from the raw injected companion paths.
    pub fn from_paths<I, S>(paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self {
            keys: paths
                .into_iter()
                .map(|p| InjectedPathKey::new(p.as_ref()))
                .collect(),
        }
    }

    /// Whether `path` denotes one of the injected companions under the host's
    /// filesystem case policy.
    pub fn contains(&self, path: &str) -> bool {
        self.keys.contains(&InjectedPathKey::new(path))
    }

    /// Whether the set is empty (no injected companions).
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// Produce the virtual (augmented) tsconfig bytes for a project that must be
/// virtualized: the user config with `companion_paths` injected into `files`,
/// byte-identical otherwise.
///
/// `companion_paths` are the companion surfaces (`Foo.vue.tsx`, …) to inject.
/// `user_tsconfig_json` is the user's real tsconfig content. The companion
/// entries are added to a `files` array (created if absent), de-duplicated
/// against any existing entries; `include`/`exclude`/`compilerOptions`/`extends`
/// and every other key are preserved untouched.
pub fn augment_tsconfig_bytes(user_tsconfig_json: &str, companion_paths: &[String]) -> String {
    // No companions ⇒ a no-op (do not fabricate a `files` key). Return the user
    // bytes unchanged so a degenerate virtualization is byte-identical.
    if companion_paths.is_empty() {
        return user_tsconfig_json.to_string();
    }

    // Parse the user config (tsconfig permits comments / trailing commas, which
    // serde_json does not — strip them through the shared workspace cleaner).
    // A config we cannot parse is served unchanged: virtualization must never
    // turn a malformed-but-served config into a different parse.
    let cleaned = strip_json_comments(user_tsconfig_json);
    let mut json: serde_json::Value = match serde_json::from_str(&cleaned) {
        Ok(serde_json::Value::Object(map)) => serde_json::Value::Object(map),
        _ => return user_tsconfig_json.to_string(),
    };
    let obj = json.as_object_mut().expect("checked to be an object above");

    // Collect the existing `files` entries, then union the companions in,
    // de-duplicating against what is already present.
    let mut files: Vec<serde_json::Value> = obj
        .get("files")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for companion in companion_paths {
        let already = files.iter().any(|f| f.as_str() == Some(companion.as_str()));
        if !already {
            files.push(serde_json::Value::String(companion.clone()));
        }
    }
    obj.insert("files".to_string(), serde_json::Value::Array(files));

    serde_json::to_string_pretty(&json).unwrap_or_else(|_| user_tsconfig_json.to_string())
}

/// Build the overlay snapshot that serves the augmented tsconfig bytes for a
/// virtualized project and the companion surfaces, layered over `real`.
///
/// `tsconfig_path` is the canonical config path the engine reads; serving the
/// augmented bytes there overrides the real config for the engine only.
/// `companions` is the `(canonical_path, generated_tsx_content)` set the overlay
/// must also serve so the injected `files` resolve.
pub fn build_virtual_overlay_snapshot(
    tsconfig_path: &str,
    augmented_bytes: &str,
    companions: &[(String, String)],
    real: Arc<dyn RealDirSource>,
) -> OverlaySnapshot {
    let mut builder = OverlaySnapshot::builder()
        // Serve the augmented config in place of the real one for the engine.
        .file(tsconfig_path, augmented_bytes)
        .real_dir_source(real);
    for (path, content) in companions {
        builder = builder.file(path, content);
    }
    builder.build()
}

/// Strip config/options diagnostics that point at an injected companion root.
///
/// A diagnostic is dropped only when its `fileName` folds to one of the injected
/// companions in `injected` (membership under the shared filesystem-identity key,
/// so a separator / drive-case / case-insensitive-FS variant of a companion is
/// still recognized — NOT exact string equality). Every diagnostic with no
/// `fileName` (a global options diagnostic) and every diagnostic pointing at a
/// real user file is retained, so a virtualized config yields the SAME
/// user-visible config/options diagnostic set the real config would.
pub fn strip_injected_root_diagnostics(
    diagnostics: Vec<Diagnostic>,
    injected: &InjectedPathSet,
) -> Vec<Diagnostic> {
    diagnostics
        .into_iter()
        .filter(|d| match &d.file_name {
            // A diagnostic pointing at an injected companion is invisible; every
            // other diagnostic (a real config/source file, or a global option
            // diagnostic with no fileName) is retained verbatim.
            Some(name) => !injected.contains(name),
            None => true,
        })
        .collect()
}

#[cfg(test)]
#[path = "tsgo_virtual_config_tests.rs"]
mod tests;
