//! @ai-generated — `.vue`-import recursion through the shared semantic
//! `Instantiate(.vue default)` query.
//!
//! A `.vue` component's PUBLIC instance surface (`{ $props, $emit, $slots }`)
//! is a first-class `SemanticQueryKey::Instantiate { base: DeclIdentity(canonical,
//! whole_hash, "default"), args: [] }` query — the SAME keyed identity both
//! [`crate::VerterHost::resolve_vue_public_type`] (the public API) and a
//! `.vue`-importing-`.vue` reference resolve through. There is NO second resolver
//! and NO depth bound: termination is by query identity (the memo's
//! same-key recursion sentinel returns `Opaque(RecursiveRef)` and the
//! `push_instantiate_active` discipline catches same-identity re-entry during
//! body lowering), so a CIRCULAR `A.vue ↔ B.vue` import cannot hang.
//!
//! These tests are discriminating: they exercise the chain `C → B → A`, prove
//! the circular `A ↔ B` import terminates, and read an imported component's
//! `$props` through the keyed query.

use std::sync::Arc;

use verter_type_expr::{PrimitiveName, TypeExpr};

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    DeclIdentity, PathSegment, ProjectionMode, ProjectionReductionContext, QueryResult,
    SemanticNodeData, SemanticQueryApi, SemanticQueryKey,
};
use crate::typeinfo::types::TypeInfoQueryLevel;
use crate::types::{FileKind, HostConfig, UpsertRequest};
use crate::VerterHost;

fn make_host_with_files(files: &[(&str, &str)]) -> Arc<VerterHost> {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    for (path, source) in files {
        workspace.inject_file((*path).to_string(), Arc::from(*source));
    }
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    host.configure_projects(vec![
        verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
            "/w".to_string(),
            "/w".to_string(),
            Some("/w/tsconfig.json".to_string()),
        ),
    ]);
    // Upsert each file with its REAL content so the synthesized `default`
    // symbol + import routes are populated (the workspace injection above lets
    // cross-file import resolution find the targets).
    for (path, source) in files {
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some((*path).to_string()),
            input_id: (*path).to_string(),
            source: Arc::from(*source),
            file_kind: FileKind::from_path(path),
            aliases: Vec::new(),
        });
        host.ensure_indexed_ready(path);
    }
    host
}

/// Member names on a `.vue`'s public instance surface, sorted.
fn public_member_names(host: &VerterHost, canonical_id: &str) -> Vec<String> {
    let surface = host
        .resolve_vue_public_type(canonical_id, TypeInfoQueryLevel::PublicType)
        .unwrap_or_else(|| panic!("{canonical_id} must have a public component type"));
    let mut names: Vec<String> = surface
        .members
        .iter()
        .map(|m| m.name.as_ref().to_string())
        .collect();
    names.sort();
    names
}

/// Resolve the keyed `Instantiate(.vue default)` to its object surface member
/// names (sorted). Panics if the query does not resolve to an `Object`.
fn vue_default_object_members(host: &VerterHost, canonical_id: &str) -> Vec<String> {
    let store_view = host.resolver_store_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);

    let whole_hash = host
        .ensure_indexed_ready(canonical_id)
        .expect("indexed ready")
        .whole_hash;
    let node = match dispatch.execute(SemanticQueryKey::Instantiate {
        base: DeclIdentity {
            canonical_id: Arc::from(canonical_id),
            whole_hash,
            decl_name: Arc::from("default"),
        },
        args: Arc::from(Vec::new().into_boxed_slice()),
        context: ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
    }) {
        QueryResult::Value(node) | QueryResult::Recursive(node) => node,
        QueryResult::Error(e) => {
            panic!("Instantiate(.vue default) for {canonical_id} errored: {e:?}")
        }
    };
    let graph = {
        use crate::resolver_core::ResolverContext;
        host_ctx.project_type_store().semantic_graph()
    };
    match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::Object(view)) => {
            let mut names: Vec<String> = view
                .members
                .iter()
                .map(|m| m.name.as_ref().to_string())
                .collect();
            names.sort();
            names
        }
        other => panic!(
            "Instantiate(.vue default) for {canonical_id} must resolve to an Object surface; got {other:?}"
        ),
    }
}

/// Like [`vue_default_object_members`] but returns `None` when the keyed query
/// does NOT resolve to a synthesized instance `Object` (an error/miss, or any
/// non-`Object` node). Used by the provenance-gate negatives: a `.vue`/`.ts`
/// whose `default` is a USERLAND value (not the synthesized public instance)
/// must NOT yield a synthesized instance object here.
fn vue_default_query_object_members(host: &VerterHost, canonical_id: &str) -> Option<Vec<String>> {
    let store_view = host.resolver_store_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);

    let whole_hash = host
        .ensure_indexed_ready(canonical_id)
        .expect("indexed ready")
        .whole_hash;
    let node = match dispatch.execute(SemanticQueryKey::Instantiate {
        base: DeclIdentity {
            canonical_id: Arc::from(canonical_id),
            whole_hash,
            decl_name: Arc::from("default"),
        },
        args: Arc::from(Vec::new().into_boxed_slice()),
        context: ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
    }) {
        QueryResult::Value(node) | QueryResult::Recursive(node) => node,
        QueryResult::Error(_) => return None,
    };
    let graph = {
        use crate::resolver_core::ResolverContext;
        host_ctx.project_type_store().semantic_graph()
    };
    match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::Object(view)) => {
            let mut names: Vec<String> = view
                .members
                .iter()
                .map(|m| m.name.as_ref().to_string())
                .collect();
            names.sort();
            Some(names)
        }
        _ => None,
    }
}

/// Project an EXPANDED member `path` rooted at the keyed
/// `Instantiate(.vue default)` of `canonical_id` and raise the terminal to a
/// [`TypeExpr`]. Exercises the SHARED resolver end-to-end: each `$props` /
/// `child` / `peer` hop drills through the synthesized instance object and any
/// `InstanceType<typeof Import>` member it carries — the exact recursive
/// `.vue`-import expansion the convergence guarantees terminates by query
/// identity.
fn project_vue_default_path(host: &VerterHost, canonical_id: &str, path: &[&str]) -> TypeExpr {
    let store_view = host.resolver_store_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);

    let whole_hash = host
        .ensure_indexed_ready(canonical_id)
        .expect("indexed ready")
        .whole_hash;
    let base = match dispatch.execute(SemanticQueryKey::Instantiate {
        base: DeclIdentity {
            canonical_id: Arc::from(canonical_id),
            whole_hash,
            decl_name: Arc::from("default"),
        },
        args: Arc::from(Vec::new().into_boxed_slice()),
        context: ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
    }) {
        QueryResult::Value(node) | QueryResult::Recursive(node) => node,
        QueryResult::Error(e) => {
            panic!("Instantiate(.vue default) base for {canonical_id} errored: {e:?}")
        }
    };
    let segments: Arc<[PathSegment]> = path
        .iter()
        .map(|s| PathSegment::Member(Arc::from(*s)))
        .collect::<Vec<_>>()
        .into();
    let terminal = match dispatch.execute(SemanticQueryKey::ProjectPath {
        base,
        path: segments,
        context: ProjectionReductionContext::published(ProjectionMode::Expanded),
    }) {
        QueryResult::Value(node) | QueryResult::Recursive(node) => node,
        QueryResult::Error(e) => {
            panic!("ProjectPath {path:?} for {canonical_id} errored: {e:?}")
        }
    };
    dispatch
        .raise_node_to_type_expr(terminal)
        .unwrap_or_else(|| panic!("terminal of {path:?} for {canonical_id} must raise to TypeExpr"))
}

const A_VUE: &str = r#"<script setup lang="ts">
defineProps<{ a: number }>();
defineEmits<{ (e: 'aEvent', v: string): void }>();
</script>
"#;

// ---------------------------------------------------------------------------
// (new-1) The keyed `Instantiate(.vue default)` query resolves a `.vue`'s public
//         instance surface to a normal `Object` carrying `$props`/`$emit`.
//
//         Discriminating: before the `build_instantiate` `.vue default` branch,
//         `Instantiate{.vue, "default", []}` fell through `resolve_prepared_type_decl`
//         (a `.vue` has no userland `default` TYPE decl, only a synthesized VALUE
//         symbol) → `Opaque(Miss)`. The members assertion fails pre-fix (Miss is
//         not an Object) and passes post-fix.
// ---------------------------------------------------------------------------

#[test]
fn instantiate_vue_default_resolves_public_instance_object() {
    const A: &str = "/w/A.vue";
    let host = make_host_with_files(&[(A, A_VUE)]);
    assert_eq!(
        vue_default_object_members(&host, A),
        vec!["$emit".to_string(), "$props".to_string()],
        "Instantiate(.vue default) must resolve to the synthesized instance object"
    );

    // Concrete MEMBER TYPES (not just names): `$props.a` is the primitive
    // `number` carried verbatim from `defineProps<{ a: number }>()`. A
    // regression that produced a broad object / opaque shell here (instead of
    // navigating into the synthesized props object) would NOT raise to
    // `Primitive(Number)`.
    assert_eq!(
        project_vue_default_path(&host, A, &["$props", "a"]),
        TypeExpr::Primitive(PrimitiveName::Number),
        "$props.a must be the number primitive from defineProps<{{ a: number }}>()"
    );

    // `$emit` carries the declared event signature `{ (e: 'aEvent', v: string):
    // void }`. A bare call-signature object raises to the canonical
    // `TypeExpr::Function` form, whose FIRST parameter is the `'aEvent'`
    // event-name literal and whose SECOND is the `string` payload. Asserting the
    // signature structure (not just the `$emit` name) proves the emit type
    // survived verbatim into the instance surface — a regression that flattened
    // `$emit` to a broad object / opaque shell would not expose these params.
    let emit = project_vue_default_path(&host, A, &["$emit"]);
    let TypeExpr::Function(emit_fn) = &emit else {
        panic!("$emit must raise to the event call-signature Function; got {emit:?}");
    };
    assert_eq!(
        emit_fn.parameters.len(),
        2,
        "the emit signature has the event name + payload parameters"
    );
    assert_eq!(
        emit_fn.parameters[0].ty,
        TypeExpr::Literal(verter_type_expr::LiteralValue::String("aEvent".to_string())),
        "first emit parameter is the 'aEvent' event-name literal"
    );
    assert_eq!(
        emit_fn.parameters[1].ty,
        TypeExpr::Primitive(PrimitiveName::String),
        "second emit parameter is the string payload"
    );
    assert_eq!(
        emit_fn.return_type.as_deref(),
        Some(&TypeExpr::Primitive(PrimitiveName::Void)),
        "the emit signature returns void"
    );
}

// ---------------------------------------------------------------------------
// (gate-1) `.vue` USERLAND-default provenance gate (NEW). A `.vue` whose
//          `<script lang="ts">` (NOT setup) declares a USERLAND `export default`
//          whose value type superficially LOOKS like a public instance
//          (`(): { $props: ... } => ...`) must NOT be mistreated as the
//          synthesized public instance: the synthesis injection is a no-op
//          (userland `default` already present), so the resolved
//          `value_symbol("default")` is the userland Const with
//          `is_synthesised_vue_default == false`, and the `.vue default` branch
//          / `resolve_vue_public_type` must NOT fire.
//
//          DISCRIMINATING: pre-fix the consumer proof was the FILE classifier
//          `is_synthesis_candidate(canonical)`, which is TRUE for any `.vue`.
//          Because the userland arrow default carries a
//          `function_signature.return_type` ({ $props: { a: number } }), the
//          pre-fix `build_vue_default_instance` happily lowered THAT userland
//          return into a synthesized-looking `{ $props }` instance object — so
//          the keyed query resolved to an `Object` carrying `$props`
//          (`Some([..])`) AND `resolve_vue_public_type` returned that surface.
//          Post-fix both gate on the structural provenance flag (false here), so
//          the keyed query no longer yields a synthesized instance object
//          (`None`) and `resolve_vue_public_type` returns `None`.
// ---------------------------------------------------------------------------

const VUE_USERLAND_DEFAULT: &str = r#"<script lang="ts">
export default (): { $props: { a: number } } => ({ $props: { a: 1 } });
</script>
"#;

#[test]
fn vue_userland_default_is_not_treated_as_synthesized_instance() {
    const FILE: &str = "/w/UserlandDefault.vue";
    let host = make_host_with_files(&[(FILE, VUE_USERLAND_DEFAULT)]);

    // Structural provenance fact: the resolved `default` value symbol is the
    // USERLAND arrow (not the synthesized construct-signature symbol), so its
    // provenance flag is false. This is the direct fact both consumers now gate
    // on — asserting it pins the producer side.
    let indexed = host.ensure_indexed_ready(FILE).expect("indexed");
    let default_symbol = indexed
        .shallow_state
        .value_symbol("default")
        .expect("the userland export default binds a `default` value symbol");
    assert!(
        !default_symbol.is_synthesised_vue_default,
        "a userland export default must NOT carry the synthesized-default provenance flag"
    );

    // The `.vue default` branch must NOT fire: the keyed query no longer yields a
    // synthesized instance object built from the userland return type.
    assert_eq!(
        vue_default_query_object_members(&host, FILE),
        None,
        "Instantiate(.vue default) must NOT synthesize an instance from a userland default"
    );

    // ...and the public-type API agrees: no synthesized public component type.
    assert!(
        host.resolve_vue_public_type(FILE, TypeInfoQueryLevel::PublicType)
            .is_none(),
        "resolve_vue_public_type must return None for a userland-default .vue"
    );
}

// ---------------------------------------------------------------------------
// (gate-2) Plain `.ts` provenance gate. A `.ts` file whose USERLAND
//          `export default` carries a decoy `$props`-bearing return type still
//          has NO public component type — a `.ts` is never an SFC. The fixture
//          deliberately HAS a `default` value symbol (with a
//          `function_signature.return_type`) so the gate is exercised on the
//          provenance flag rather than trivially passing on a missing `default`.
//
//          NEGATIVE GUARD: `is_synthesised_vue_default == false` for the
//          userland `.ts` default, so neither the keyed `.vue default` branch
//          nor `resolve_vue_public_type` may synthesize an instance. (The prior
//          interface-only `.ts` test is caught by the missing-`default` check
//          regardless of the gate; this one is NOT — it pins that a present
//          userland default is rejected by the provenance flag.)
// ---------------------------------------------------------------------------

const TS_USERLAND_DEFAULT: &str =
    "export default (): { $props: { a: number } } => ({ $props: { a: 1 } });\n";

#[test]
fn plain_ts_userland_default_has_no_synthesized_instance() {
    const FILE: &str = "/w/ts_default.ts";
    let host = make_host_with_files(&[(FILE, TS_USERLAND_DEFAULT)]);

    // The `.ts` default IS present but is userland — flag is false.
    let indexed = host.ensure_indexed_ready(FILE).expect("indexed");
    let default_symbol = indexed
        .shallow_state
        .value_symbol("default")
        .expect("the .ts export default binds a `default` value symbol");
    assert!(
        !default_symbol.is_synthesised_vue_default,
        "a plain .ts userland default must NOT carry the synthesized-default flag"
    );

    // The `.vue default` branch must NOT fire for a `.ts` file.
    assert_eq!(
        vue_default_query_object_members(&host, FILE),
        None,
        "Instantiate(default) must NOT synthesize an instance for a plain .ts default"
    );

    // And the public-type API returns None.
    assert!(
        host.resolve_vue_public_type(FILE, TypeInfoQueryLevel::PublicType)
            .is_none(),
        "a plain .ts file has no .vue public component type"
    );
}

// ---------------------------------------------------------------------------
// (new-2) `C → B → A` chain: each `.vue` imports the next and its props embed the
//         imported component's instance. Resolving C's public type must navigate
//         the whole chain (NO miss along the way).
// ---------------------------------------------------------------------------

#[test]
fn vue_import_chain_c_b_a_resolves() {
    const A: &str = "/w/A.vue";
    const B: &str = "/w/B.vue";
    const C: &str = "/w/C.vue";
    let b_vue = r#"<script setup lang="ts">
import A from './A.vue';
defineProps<{ child: InstanceType<typeof A> }>();
</script>
"#;
    let c_vue = r#"<script setup lang="ts">
import B from './B.vue';
defineProps<{ child: InstanceType<typeof B> }>();
</script>
"#;
    let host = make_host_with_files(&[(A, A_VUE), (B, b_vue), (C, c_vue)]);

    // Each link's own public type resolves (the chain does not collapse anywhere).
    assert_eq!(public_member_names(&host, A), vec!["$emit", "$props"]);
    assert_eq!(public_member_names(&host, B), vec!["$props"]);
    assert_eq!(public_member_names(&host, C), vec!["$props"]);

    // And the keyed query for the deepest link still resolves to an Object.
    assert_eq!(
        vue_default_object_members(&host, A),
        vec!["$emit".to_string(), "$props".to_string()],
    );
}

// ---------------------------------------------------------------------------
// (new-3) CIRCULAR `A ↔ B` import — NO HANG. Each `.vue` imports the other and
//         embeds the other's instance in its props. Resolving either public type
//         must COMPLETE (the test returning at all is the proof) and yield a
//         bounded result. Termination is by query identity (the memo's same-key
//         `Instantiate` recursion sentinel), NOT a depth bound.
// ---------------------------------------------------------------------------

#[test]
fn vue_circular_import_a_b_does_not_hang() {
    const A: &str = "/w/CycA.vue";
    const B: &str = "/w/CycB.vue";
    let a_vue = r#"<script setup lang="ts">
import B from './CycB.vue';
defineProps<{ peer: InstanceType<typeof B>; a: number }>();
</script>
"#;
    let b_vue = r#"<script setup lang="ts">
import A from './CycA.vue';
defineProps<{ peer: InstanceType<typeof A>; b: string }>();
</script>
"#;
    let host = make_host_with_files(&[(A, a_vue), (B, b_vue)]);

    // The mere completion of these calls is the no-hang proof. Both resolve to a
    // bounded public surface carrying `$props` (the cyclic `peer` member is a
    // bounded opaque recursive edge, never an infinite expansion).
    assert_eq!(public_member_names(&host, A), vec!["$props"]);
    assert_eq!(public_member_names(&host, B), vec!["$props"]);

    // The keyed query for each terminates with a real Object surface.
    assert_eq!(
        vue_default_object_members(&host, A),
        vec!["$props".to_string()]
    );
    assert_eq!(
        vue_default_object_members(&host, B),
        vec!["$props".to_string()]
    );
}
