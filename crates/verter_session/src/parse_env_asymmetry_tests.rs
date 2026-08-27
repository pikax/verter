//! Why the `ResolvedImportFactsKey` `parse_env_hash` asymmetry is not a
//! live bug — and the guard that fails the moment it becomes one.
//!
//! ## The asymmetry
//!
//! The producer keys on the LIVE PER-CANONICAL env
//! (`VerterHost::host_view_env_hashes_for`: resolve the canonical's owning
//! project, read that project's env array, else the workspace default).
//! The validator composes its `ResolvedImportFactsKey` from the
//! view-captured WORKSPACE-LEVEL
//! `project_env_root.env_hashes.parse_env_hash` — NOT from the
//! per-canonical `ProjectEnvRoot::parse_env_hash_for`, which exists and
//! mirrors the producer exactly. Read as source, that is a
//! producer/validator divergence: a canonical whose owning project
//! carried a different `parse_env_hash` than the workspace default would
//! be ADMITTED under one key and LOOKED UP under another, so the
//! validator would miss a bundle the producer just wrote.
//!
//! ## Why it cannot happen
//!
//! `parse_env_hash` does not depend on the project at all.
//! `IdeProjectConfig::parse_env_hash` folds exactly two things — a
//! constant salt and `EnvHashInputs::parser_flags` — and never reads
//! `&self`. Both `compose_env_hash_tables` and
//! `compose_env_hash_tables_from_configs` build `EnvHashInputs` with the
//! workspace-wide `WORKSPACE_PARSER_FLAGS` and pass the SAME `inputs` to
//! every project. So every project's `parse_env_hash` is byte-identical
//! to every other project's and to the workspace default, and the two
//! keying paths cannot disagree.
//!
//! The asymmetry is therefore a READABILITY defect, not a correctness
//! one: the validator reads a field that happens to equal the value it
//! should conceptually be reading.
//!
//! ## What this test is for
//!
//! It pins the mechanism, not the conclusion. `parse_env_hash` is the
//! ONLY one of the four env dimensions with this property —
//! `resolve_env_hash` folds `base_url`, `paths`, aliases and references,
//! all per-project — so a future change that folds any per-project state
//! into `parse_env_hash` would be entirely reasonable in isolation and
//! would silently make the asymmetry a live cache-miss bug. This test
//! fails at that moment, and the fix is to route the validator through
//! `ProjectEnvRoot::parse_env_hash_for`, which already exists.
//!
//! Mutation recipe: fold anything per-project into
//! `IdeProjectConfig::parse_env_hash` — e.g. append `self.root` to `buf`
//! before `compute_hash16`. The two projects' hashes diverge and this
//! fails, naming the consequence.

use std::sync::Arc;

use crate::{HostConfig, UpsertRequest, VerterHost};

fn upsert(host: &VerterHost, path: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(source),
            file_language: crate::LanguageRegistry::global()
                .classify_static(path)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|e| panic!("upsert {path} failed: {e:?}"));
}

#[test]
fn parse_env_hash_is_project_independent_so_the_key_asymmetry_is_unreachable() {
    let host = VerterHost::new_standalone(HostConfig::default());
    host.configure_projects(vec![
        verter_workspace::ide_project_config(
            "/ws/a".to_string(),
            "/ws".to_string(),
            Some("/ws/a/tsconfig.json".to_string()),
        ),
        verter_workspace::ide_project_config(
            "/ws/b".to_string(),
            "/ws".to_string(),
            Some("/ws/b/tsconfig.json".to_string()),
        ),
    ]);
    upsert(&host, "/ws/a/one.ts", "export const a = 1;\n");
    upsert(&host, "/ws/b/two.ts", "export const b = 2;\n");

    let owned_by_a = host.host_view_env_hashes_for("/ws/a/one.ts").parse_env_hash;
    let owned_by_b = host.host_view_env_hashes_for("/ws/b/two.ts").parse_env_hash;
    // No owning project → the workspace-default array, which is the value
    // the VALIDATOR keys on for every canonical.
    let workspace_default = host
        .host_view_env_hashes_for("/outside/three.ts")
        .parse_env_hash;

    assert_eq!(
        owned_by_a, owned_by_b,
        "two DISTINCT configured projects must still produce the same parse_env_hash — \
         `IdeProjectConfig::parse_env_hash` folds only a constant salt and the \
         workspace-wide parser flags, never `&self`"
    );
    assert_eq!(
        owned_by_b, workspace_default,
        "and a canonical's owning-project parse_env_hash must equal the workspace \
         default. THIS is what makes the ResolvedImportFactsKey asymmetry unreachable: \
         the producer keys per-canonical (`host_view_env_hashes_for`) while the \
         validator keys on the captured workspace-level \
         `project_env_root.env_hashes.parse_env_hash`. If this equality ever breaks, \
         those two compose DIFFERENT keys for the same fact and every admitted \
         resolved-import bundle for a non-default project becomes unfindable — route \
         the validator through `ProjectEnvRoot::parse_env_hash_for` instead"
    );
}
