//! Discriminating test for negative (unresolved) facts emitted by
//! the production producer for
//! [`verter_session::resolved_import_facts::ResolvedImportFactsDb`].
//!
//! **Discrimination:** Triggers the producer with a specifier that
//! has NO `resolved_canonical_id` and NO candidates. Asserts:
//!
//! 1. `resolved_import_facts_negative_admissions` advances by at
//!    least one against pre-state.
//! 2. The admitted entry's `resolved_canonical` is `None`
//!    (negative fact preserved).
//! 3. `resolved_source_name` is the original requested name (the
//!    binding name), not `Option::None` — non-optional contract
//!    holds for negative facts.
//!
//! Against pre-`1.f` state the producer is absent, so the negative
//! counter stays at 0 and the test FAILS. Post-GREEN the producer
//! admits the negative fact and the counter advances.

use std::sync::Arc;

use verter_session::session_view::{HostView, SessionView};
use verter_session::{
    CompileErrorPolicy, DependencyResolution, FileLanguage, HostConfig, UpsertRequest, VerterHost,
};

#[test]
fn producer_admits_negative_entry_for_unresolved_specifier() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    }));

    // Owner imports from a missing module — no canonical at all.
    let owner_source = "import { Missing } from './gone';\nexport const o = 1;\n";
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/owner.ts".to_string(),
            source: Arc::from(owner_source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("owner upsert");

    let before = host
        .project_type_store()
        .resolved_import_facts()
        .resolved_import_facts_negative_admissions();

    // Pass an unresolved route — no canonical, no candidates. The
    // producer must admit this as a NEGATIVE fact (entry with
    // `resolved_canonical: None`).
    host.set_import_dependencies(
        "/owner.ts",
        vec![DependencyResolution {
            specifier: "./gone".to_string(),
            resolved_canonical_id: None,
            possible_canonical_ids: Vec::new(),
        }],
    );

    let after = host
        .project_type_store()
        .resolved_import_facts()
        .resolved_import_facts_negative_admissions();
    assert!(
        after > before,
        "negative admission counter must advance after admitting an unresolved import (before={before}, after={after})",
    );

    let view = HostView::new(Arc::clone(&host));
    let payload = view
        .resolved_import_facts("/owner.ts")
        .expect("producer must admit a resolved-import-facts payload");

    let missing_entry = payload
        .import_clauses
        .iter()
        .find(|e| e.binding.as_ref() == "Missing")
        .expect("`Missing` binding must be admitted as a negative fact");

    assert!(
        missing_entry.resolved_canonical.is_none(),
        "negative fact must keep `resolved_canonical: None`",
    );
    assert_eq!(
        missing_entry.resolved_source_name.as_ref(),
        "Missing",
        "resolved_source_name is non-optional; for unresolved imports it preserves the originally requested name",
    );
}
