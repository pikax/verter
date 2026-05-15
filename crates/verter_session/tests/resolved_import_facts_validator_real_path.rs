//! Validator-path discriminating test for
//! [`verter_session::resolved_import_facts::ResolvedImportFactsDb`].
//!
//! **Discrimination:** Warms a `ResolveImportsFactRef` against an
//! admitted producer entry and asserts:
//!
//! 1. The store-view validator (`validates_fact_signature`) returns
//!    `true` on the warm-hit path.
//! 2. After the source content is edited (which bumps
//!    `content_hash`), the SAME fact ref no longer validates
//!    (cache miss on the version-pinned key — R5/R26 short-circuit).
//!
//! Against pre-`1.f` state the producer never runs, the cache slot
//! is empty, and the validator's untracked-file optimistic-accept
//! branch returns `false` for a non-zero expected hash — the test's
//! warm-hit assertion FAILS. Post-GREEN the producer admits the
//! entry and the validator returns `true` on the warm path.

use std::sync::Arc;

use verter_semantic::facts::registry::{FactKey, FactLane, InternedName, InternedSpecifier};
use verter_session::resolver_core::{
    FactVersionRef, ResolveImportsFactRef, ResolverStore, StoreView,
};
use verter_session::session_view::{HostView, SessionView};
use verter_session::{
    CompileErrorPolicy, DependencyResolution, FileKind, HostConfig, UpsertRequest, VerterHost,
};

#[test]
#[ignore = "block-1.f RED — closed by same-block implementation"]
fn validator_warms_then_invalidates_after_source_edit() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    }));

    let _ = host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: "/dep.ts".to_string(),
        source: Arc::from("export const v = 1;"),
        file_kind: FileKind::NonSfc,
        aliases: Vec::new(),
    })
    .expect("dep upsert");

    let _ = host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: "/owner.ts".to_string(),
        source: Arc::from("import { v } from './dep';\nexport const o = v;\n"),
        file_kind: FileKind::NonSfc,
        aliases: Vec::new(),
    })
    .expect("owner upsert v1");

    // Trigger the producer.
    host.set_import_dependencies(
        "/owner.ts",
        vec![DependencyResolution {
            specifier: "./dep".to_string(),
            resolved_canonical_id: Some("/dep.ts".to_string()),
            possible_canonical_ids: vec!["/dep.ts".to_string()],
        }],
    );

    let view = HostView::new(Arc::clone(&host));
    let payload = view
        .resolved_import_facts("/owner.ts")
        .expect("producer admitted the resolved-import-facts payload");

    let entry = payload
        .import_clauses
        .iter()
        .find(|e| e.binding.as_ref() == "v")
        .expect("`v` binding admitted");

    // Build a `ResolveImportsFactRef` from the admitted entry's
    // fact lane. The validator must accept this against the warm
    // cache slot.
    let fact_ref = ResolveImportsFactRef {
        canonical_id: "/owner.ts".to_string(),
        key: FactKey::ResolvedImportClause {
            specifier: InternedSpecifier::from(entry.specifier.as_ref()),
            binding: InternedName::from(entry.binding.as_ref()),
            space: entry.space,
            resolved_canonical: entry
                .resolved_canonical
                .as_ref()
                .map(Arc::clone)
                .expect("entry must carry a resolved canonical here"),
            resolved_source_name: InternedName::from(entry.resolved_source_name.as_ref()),
        },
        lane: FactLane::Semantic,
        expected_hash: entry.fact.semantic_hash,
    };
    let warm_sig = vec![FactVersionRef::ResolveImports(fact_ref.clone())];

    let warm_view = host.snapshot_view();
    assert!(
        warm_view.validates_fact_signature(&warm_sig),
        "warm-hit path: validator must return true against the admitted entry",
    );

    // Edit the source of the OWNER. This bumps the owner's
    // `content_hash`, which moves the cache slot identity — the
    // pinned `ResolveImportsFactRef.expected_hash` is now keyed
    // under a stale `content_hash`.
    let _ = host.upsert(UpsertRequest {
        canonical_id: None,
        input_id: "/owner.ts".to_string(),
        source: Arc::from("// edited\nimport { v } from './dep';\nexport const o = v;\n"),
        file_kind: FileKind::NonSfc,
        aliases: Vec::new(),
    })
    .expect("owner upsert v2");

    let post_edit_view = host.snapshot_view();
    assert!(
        !post_edit_view.validates_fact_signature(&warm_sig),
        "after the owner's source edits, the fact ref must NOT validate (R5/R26 — content_hash changed)",
    );
}
