//! Leaf support helpers for the SHARED tsgo provider: path/URI normalization,
//! editor-binding resolution, and carrier-companion language-id classification.
//!
//! Free-standing, dependency-light utilities factored out of
//! [`super::shared`] so the provider module stays focused on the attach/inject/
//! diagnostics flow. Behaviour is identical to the inline definitions; only the
//! file location and `pub(super)` visibility differ.

use verter_session::external_ts::EditorBindingFact;
use verter_session::file_artifact_store::ProjectIdentity;
use verter_span::path::canonicalize_path;

/// Forward-slash-normalize a path for engine comparison.
pub(super) fn slash(p: &str) -> String {
    p.replace('\\', "/")
}

/// A distinct identity from `id` (one byte flipped) — the fail-closed
/// editor-binding mismatch witness (never a forged match).
fn distinct_identity(id: ProjectIdentity) -> ProjectIdentity {
    let mut bytes = id.0;
    bytes[0] ^= 0xFF;
    ProjectIdentity(bytes)
}

/// The editor-binding-identity fact + the bound identity, keyed on the resolved
/// PROJECT identity — never a bare workspace-root hash.
///
/// The editor-binding EVIDENCE is the initialize witness `root_uri`: the editor bound
/// the carrier to the workspace Verter resolved iff the witness `rootUri` canonicalizes
/// to the resolved `workspace_root`. When it matches, the fact is
/// `Matched(project_identity)`; a missing witness root, or a DIFFERENT workspace,
/// yields a distinct identity ⇒ `Mismatch` (fail closed — never a forged match).
///
/// Because the fact is keyed on `project_identity`, two DISTINCT configured projects
/// under the SAME `rootUri` produce DISTINCT `Matched` facts, so SHARED eligibility
/// established for one project can never spill to a sibling project of the same
/// workspace. Keying on the workspace-root hash (the prior behaviour) made those two
/// facts EQUAL — the eligibility-spill defect this closes.
pub(super) fn resolve_editor_binding(
    project_identity: ProjectIdentity,
    workspace_root: &str,
    witness_root_uri: Option<&str>,
) -> (EditorBindingFact, ProjectIdentity) {
    let editor_bound = match witness_root_uri {
        Some(root_uri)
            if canonicalize_path(&file_uri_to_path(root_uri))
                == canonicalize_path(workspace_root) =>
        {
            project_identity
        }
        _ => distinct_identity(project_identity),
    };
    (
        EditorBindingFact::evaluate(&project_identity, &editor_bound),
        editor_bound,
    )
}

/// The parent directory of a forward-slashed path (the tsconfig dir base).
pub(super) fn parent_dir(path: &str) -> String {
    let slashed = slash(path);
    match slashed.rfind('/') {
        Some(i) => slashed[..i].to_string(),
        None => String::new(),
    }
}

/// Minimal `file://` URI decode (drive-form + POSIX). Shared shape with the rest
/// of the carrier path handling; the shim's egress layer canonicalizes on match.
fn file_uri_to_path(uri: &str) -> String {
    verter_span::uri::file_uri_to_path(uri)
}

/// Convert a forward-slashed path to a `file://` URI.
pub(super) fn path_to_file_uri(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    }
}

/// The LSP language id for a carrier companion by extension: `.tsx`/`.jsx` are
/// the JSX IDE carriers, `.ts`/`.js` the plain companions.
pub(super) fn language_id_for(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".tsx") {
        "typescriptreact"
    } else if lower.ends_with(".jsx") {
        "javascriptreact"
    } else if lower.ends_with(".js") || lower.ends_with(".mjs") || lower.ends_with(".cjs") {
        "javascript"
    } else {
        "typescript"
    }
}
