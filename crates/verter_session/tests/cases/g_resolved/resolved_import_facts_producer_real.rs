//! Discriminating test for the production producer for
//! [`verter_session::resolved_import_facts::ResolvedImportFactsDb`].
//!
//! **Discrimination:** Triggers the producer through
//! [`VerterHost::set_import_dependencies`] on a real `.ts` owner
//! that imports a resolved canonical. Asserts:
//!
//! 1. `resolved_import_facts_positive_admissions` advances by at
//!    least one (delta > 0 against pre-producer state).
//! 2. The admitted entry's `resolved_canonical` is `Some(...)` and
//!    matches the requested target.
//! 3. `resolved_source_name` is non-empty (the producer constructs
//!    it from the imported name, not from `Option::None`).
//!
//! Against pre-`1.f` state, the producer is absent — counters stay
//! at 0 (`positive_admissions == 0`) and the cache stays empty,
//! making the test FAIL. Post-GREEN the producer runs from
//! `set_import_dependencies` and the counter advances.

use std::sync::Arc;

use verter_session::session_view::{HostView, SessionView};
use verter_session::{
    CompileErrorPolicy, DependencyResolution, FileLanguage, HostConfig, UpsertRequest, VerterHost,
};

#[test]
fn producer_admits_positive_entry_with_resolved_source_name() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    }));

    // Owner file imports from a relative dep. Both are real `.ts`
    // files so `script_analysis.imports` and the workspace's
    // dependency-target map are both populated.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/dep.ts".to_string(),
            source: Arc::from("export const used = 1;"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("dep upsert");

    let owner_source = "import { used } from './dep';\nexport const o = used;\n";
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/owner.ts".to_string(),
            source: Arc::from(owner_source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("owner upsert");

    // Pre-snapshot the positive-admission counter.
    let before = host
        .project_type_store()
        .resolved_import_facts()
        .resolved_import_facts_positive_admissions();

    // Trigger the producer through the canonical wire site.
    host.set_import_dependencies(
        "/owner.ts",
        vec![DependencyResolution {
            specifier: "./dep".to_string(),
            resolved_canonical_id: Some("/dep.ts".to_string()),
            possible_canonical_ids: vec!["/dep.ts".to_string()],
        }],
    );

    let after = host
        .project_type_store()
        .resolved_import_facts()
        .resolved_import_facts_positive_admissions();

    assert!(
        after > before,
        "positive admission counter must advance after set_import_dependencies (before={before}, after={after})",
    );

    // Read back the admitted entry through the view substrate.
    let view = HostView::new(Arc::clone(&host));
    let payload = view
        .resolved_import_facts("/owner.ts")
        .expect("producer must admit a resolved-import-facts payload for the owner");

    assert!(
        !payload.import_clauses.is_empty(),
        "the admitted payload must carry at least one import-clause entry",
    );

    let used_entry = payload
        .import_clauses
        .iter()
        .find(|e| e.binding.as_ref() == "used")
        .expect("the `used` binding must be admitted");

    assert_eq!(
        used_entry.resolved_canonical.as_ref().map(|c| c.as_ref()),
        Some("/dep.ts"),
        "positive entry must carry the resolved canonical",
    );
    assert!(
        !used_entry.resolved_source_name.as_ref().is_empty(),
        "resolved_source_name is NON-OPTIONAL and must be populated",
    );
    assert_eq!(
        used_entry.resolved_source_name.as_ref(),
        "used",
        "named-import binding maps to the original exported name",
    );
}
