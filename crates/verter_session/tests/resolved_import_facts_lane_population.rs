//! Discriminating test: every admitted
//! [`verter_session::resolved_import_facts::ResolvedImportClauseEntry`]
//! must carry a `fact: Arc<Fact>` with BOTH `semantic_hash` AND
//! `display_hash` populated (non-default `[0u8; 16]` — R13 lane
//! contract).
//!
//! **Discrimination:** Pre-`1.f` state has no producer, so the
//! cache slot is empty and no `Fact` exists to inspect — the
//! `expect("entry...")` calls FAIL. Post-GREEN the producer
//! constructs the lanes from the resolved-clause structural hash
//! (semantic) plus a salted display variant (display) and the
//! assertions pass.

use std::sync::Arc;

use verter_session::session_view::{HostView, SessionView};
use verter_session::{
    CompileErrorPolicy, DependencyResolution, FileKind, HostConfig, UpsertRequest, VerterHost,
};

const ZERO_HASH: [u8; 16] = [0u8; 16];

#[test]
fn admitted_fact_has_both_lanes_populated_and_distinct() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    }));

    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/util.ts".to_string(),
            source: Arc::from("export function helper() { return 1; }"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("util upsert");
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/lane.ts".to_string(),
            source: Arc::from("import { helper } from './util';\nexport const k = helper();\n"),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("lane upsert");

    host.set_import_dependencies(
        "/lane.ts",
        vec![DependencyResolution {
            specifier: "./util".to_string(),
            resolved_canonical_id: Some("/util.ts".to_string()),
            possible_canonical_ids: vec!["/util.ts".to_string()],
        }],
    );

    let view = HostView::new(Arc::clone(&host));
    let payload = view
        .resolved_import_facts("/lane.ts")
        .expect("producer admitted the resolved-import-facts payload");

    let entry = payload
        .import_clauses
        .iter()
        .find(|e| e.binding.as_ref() == "helper")
        .expect("`helper` binding admitted");

    // R13 lane contract: BOTH lanes must be populated (non-zero).
    assert_ne!(
        entry.fact.semantic_hash, ZERO_HASH,
        "semantic_hash must be populated (non-zero) on the admitted Fact",
    );
    assert_ne!(
        entry.fact.display_hash, ZERO_HASH,
        "display_hash must be populated (non-zero) on the admitted Fact",
    );

    // The two lanes must be DISTINCT — otherwise we have not
    // populated separate lanes (collapsing display = semantic
    // means we cannot discriminate cosmetic-only edits later).
    assert_ne!(
        entry.fact.semantic_hash, entry.fact.display_hash,
        "semantic_hash and display_hash must be distinct so the two lanes can carry different observations under R13",
    );
}
