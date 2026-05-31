//! Block 6.i Transit-Shallow Publication — discriminator #2.
//!
//! Verifies that a `Mapped` whose `source` is a `DeclRef` /
//! `InstantiationRef` carrier (produced by lowering an imported
//! interface at `structural_transit_with_mode(Navigate)`) enumerates
//! through the Shallow walker's `synthesise_mapped_surface` rather
//! than returning an empty / None surface.
//!
//! The codex Transit-Shallow architecture (Q1 #2 / Q3) inserts a new
//! source-surface enumeration helper `mapped_surface_source_members_for_projection`
//! that runs ONLY inside `synthesise_mapped_surface` (not the
//! global `key_names_*` API): it dispatches
//! `ProjectPath { source, [], Published(Shallow) }` and returns a
//! `Vec<SurfaceMember>` so the synthesiser can use source member
//! modifiers and Identity-mapper values directly when available.
//!
//! ## Discrimination progression
//!
//! - **Commit 1 (no substrate):** FAIL — under `Published(Shallow)`
//!   the source enumeration fallback in `synthesise_mapped_surface`
//!   calls `key_names_from_base_node(source)` which does NOT handle
//!   `DeclRef` / `InstantiationRef`; the synthesise returns None and
//!   the empty-path Shallow walker publishes an empty surface (the
//!   `msg` member is absent).
//! - **Commit 2 (substrate added):** PASS — the new helper enables
//!   source enumeration via empty-path Published(Shallow) on the
//!   source carrier; the synthesised surface contains the expected
//!   `msg` member.
//! - **Commit 3 (atomic cutover):** PASS — substrate exercised by
//!   the publication path too.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

use std::sync::Arc;

use verter_session::semantic_query::{
    PathSegment, ProjectionMode, ProjectionReductionContext, QueryResult, SemanticNodeData,
    SemanticQueryKey,
};
use verter_session::{for_tests, FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::TypeExpr;

const SOURCE_TS: &str = r#"
export interface DeclRefSource {
  msg: string;
  count: number;
}

// Mapped where the source is the imported interface (a DeclRef under
// structural-transit lowering). The value_expr is intentionally
// non-Identity (a literal type) so the per-key substitute-and-evaluate
// path is exercised — the discriminator is the SURFACE membership, not
// the value shape.
export type MapDeclRefSource = {
  [K in keyof DeclRefSource]?: 'mapped'
};
"#;

#[test]
fn shallow_walker_enumerates_declref_source_via_source_surface_helper() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/source.ts".to_string()),
        input_id: "/source.ts".to_string(),
        source: Arc::from(SOURCE_TS),
        file_kind: FileKind::from_path("/source.ts"),
        aliases: Vec::new(),
    });

    // Step 1: lower `MapDeclRefSource` under StructuralTransit(Navigate).
    // The Mapped's source `DeclRefSource` lowers as a DeclRef carrier
    // under StructuralTransit; the Mapped node itself carrier-stops
    // (no eager keyspace reification).
    let expr = TypeExpr::Ref {
        name: Arc::from("MapDeclRefSource"),
        type_arguments: Arc::from(Vec::new().into_boxed_slice()),
    };

    let carrier = for_tests::dispatch_lower_type_expr_in_scope_with_context_for_tests(
        &host,
        "/source.ts",
        &expr,
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
    )
    .expect("lowering `MapDeclRefSource` under StructuralTransit(Navigate) must succeed");

    // Step 2: drive empty-path Published(Shallow) on the carrier. The
    // walker visits the Mapped node and calls `synthesise_mapped_surface`.
    // Pre-Commit-2 substrate, the fallback `key_names_from_base_node` does
    // NOT handle DeclRef and the surface is None / empty. Post-Commit-2,
    // the new source-surface helper enables enumeration via the source's
    // empty-path Shallow projection.
    let project_query = SemanticQueryKey::ProjectPath {
        base: carrier,
        path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Shallow),
    };
    let surface_node = match for_tests::dispatch_execute_for_tests(&host, project_query) {
        QueryResult::Value(node) => node,
        other => panic!(
            "ProjectPath {{ MapDeclRefSource, [], Published(Shallow) }} must yield a value node, \
             got {other:?}",
        ),
    };

    let graph = host.project_type_store().semantic_graph();
    let surface_data = graph
        .node_data(surface_node)
        .expect("surface node must have semantic data");

    let view = match surface_data.as_ref() {
        SemanticNodeData::Object(view) => view.clone(),
        other => panic!(
            "Block 6.i Transit-Shallow Publication contract — synthesise_mapped_surface MUST \
             produce a `SemanticNodeData::Object` for a Mapped with a DeclRef source under \
             Published(Shallow). Pre-substrate the synthesiser bails (no source enumeration for \
             DeclRef carriers) and publishes a deferred shell instead. Got: {other:?}",
        ),
    };

    let names: Vec<&str> = view.members.iter().map(|m| m.name.as_ref()).collect();
    assert!(
        names.contains(&"msg"),
        "Block 6.i Transit-Shallow Publication — synthesised surface MUST contain `msg` (one \
         of DeclRefSource's members). Pre-substrate the enumeration silently drops it; the new \
         `mapped_surface_source_members_for_projection` helper enables the DeclRef enumeration. \
         Got: {names:?}",
    );
    assert!(
        names.contains(&"count"),
        "Block 6.i Transit-Shallow Publication — synthesised surface MUST contain `count`. \
         Got: {names:?}",
    );
}
