//! @ai-generated — `.vue`-import recursion through the shared semantic
//! `Instantiate(.vue default)` query.
//!
//! A `.vue` component's PUBLIC instance surface (`{ $props, $emit, $slots }`)
//! is a first-class `SemanticQueryKey::Instantiate { base, args: [], context }`
//! query whose `base` is the env-bearing content-free `ResolvedDeclSlotIdentity`
//! slot for the `.vue` `"default"` decl (`defining_canonical = canonical`,
//! `merged_symbol_name = "default"`, `symbol_space = Type`). Per R6 the slot is
//! content-free — the live `whole_hash` is re-sourced at value-compute time via
//! `ensure_indexed_ready(base.defining_canonical).whole_hash`, NOT carried in the
//! key. It is the SAME keyed identity both
//! [`crate::VerterHost::resolve_vue_public_type`] (the public API) and a
//! `.vue`-importing-`.vue` reference resolve through. There is NO second resolver
//! and NO depth bound: termination is by query identity. Two distinct
//! back-edge mechanisms keep a CIRCULAR `A.vue ↔ B.vue` import from hanging,
//! and they bound DIFFERENTLY — do not conflate them:
//!
//! - **lazy bare-`Ref` / mutual route** (e.g. `defineProps<{ peer: B }>()` with a
//!   reciprocal `E ↔ F`): each `Instantiate(.vue default)` side completes and
//!   pops before the next is demanded, and the inner cyclic reference lowers in
//!   `Navigate` to a shallow `DeclRef` carrier (`Ref { name: "default" }`)
//!   instead of re-dispatching. The back-edge is a bounded SHALLOW `Object`, NOT
//!   `RecursiveRef`. This is the common cross-file cycle shape.
//! - **eager same-key re-entry** (`InstanceType<typeof Self>` projected
//!   `Published(Expanded)`): the outer `Instantiate(Self, default)` frame is
//!   STILL active when `typeof Self` re-enters the SAME `(Self, default)`
//!   identity, so `push_instantiate_active` returns `false` and the back-edge is
//!   `Opaque(RecursiveRef)`.
//!
//! These tests are discriminating: they exercise the chain `C → B → A`, prove
//! both cycle shapes terminate (shallow bound for the mutual route, the active
//! guard for the eager self-cycle), and read an imported component's `$props`
//! through the keyed query.

use std::sync::Arc;

use verter_type_expr::{PrimitiveName, TypeExpr};

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    PathSegment, ProjectionMode, ProjectionReductionContext, QueryResult, ScopeId,
    SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    ValueRootKey,
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
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);

    let _whole_hash = host
        .ensure_indexed_ready(canonical_id)
        .expect("indexed ready")
        .whole_hash;
    let node = match dispatch.execute_type_node(SemanticQueryKey::Instantiate {
        base: crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from(canonical_id),
            Arc::from("default"),
        ),
        args: Arc::from(Vec::new().into_boxed_slice()),
        context: crate::semantic_query::InstantiateContext::new(
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
            Default::default(),
        ),
    }) {
        QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
        QueryResult::Recursive(node) => node,
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
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);

    let _whole_hash = host
        .ensure_indexed_ready(canonical_id)
        .expect("indexed ready")
        .whole_hash;
    let node = match dispatch.execute_type_node(SemanticQueryKey::Instantiate {
        base: crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from(canonical_id),
            Arc::from("default"),
        ),
        args: Arc::from(Vec::new().into_boxed_slice()),
        context: crate::semantic_query::InstantiateContext::new(
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
            Default::default(),
        ),
    }) {
        QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
        QueryResult::Recursive(node) => node,
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
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);

    let _whole_hash = host
        .ensure_indexed_ready(canonical_id)
        .expect("indexed ready")
        .whole_hash;
    let base = match dispatch.execute_type_node(SemanticQueryKey::Instantiate {
        base: crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from(canonical_id),
            Arc::from("default"),
        ),
        args: Arc::from(Vec::new().into_boxed_slice()),
        context: crate::semantic_query::InstantiateContext::new(
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
            Default::default(),
        ),
    }) {
        QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
        QueryResult::Recursive(node) => node,
        QueryResult::Error(e) => {
            panic!("Instantiate(.vue default) base for {canonical_id} errored: {e:?}")
        }
    };
    let segments: Arc<[PathSegment]> = path
        .iter()
        .map(|s| PathSegment::Member(Arc::from(*s)))
        .collect::<Vec<_>>()
        .into();
    let terminal = match dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base,
        path: segments,
        context: ProjectionReductionContext::published(ProjectionMode::Expanded),
    }) {
        QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
        QueryResult::Recursive(node) => node,
        QueryResult::Error(e) => {
            panic!("ProjectPath {path:?} for {canonical_id} errored: {e:?}")
        }
    };
    dispatch
        .raise_node_to_type_expr(terminal)
        .unwrap_or_else(|| panic!("terminal of {path:?} for {canonical_id} must raise to TypeExpr"))
}

/// The raw node `Instantiate{ .vue, "default", [] }` (Navigate /
/// structural-transit) resolves to — the synthesized public instance object.
/// Dispatched on the SUPPLIED `dispatch` so the returned `SemanticNodeId` is
/// comparable to other queries run on the same graph.
fn instantiate_vue_default_node(
    host: &VerterHost,
    dispatch: &ProjectSemanticDispatch<'_>,
    canonical_id: &str,
) -> SemanticNodeId {
    let _whole_hash = host
        .ensure_indexed_ready(canonical_id)
        .expect("indexed ready")
        .whole_hash;
    match dispatch.execute_type_node(SemanticQueryKey::Instantiate {
        base: crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from(canonical_id),
            Arc::from("default"),
        ),
        args: Arc::from(Vec::new().into_boxed_slice()),
        context: crate::semantic_query::InstantiateContext::new(
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
            Default::default(),
        ),
    }) {
        QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
        QueryResult::Recursive(node) => node,
        QueryResult::Error(e) => {
            panic!("Instantiate(.vue default) for {canonical_id} errored: {e:?}")
        }
    }
}

/// The construct-signature RETURN node of `typeof default` for a synthesized
/// `.vue` `default` (`TypeOf{ value_root: (canonical, "default") }`). The result
/// of `build_typeof` is a constructor-like Object carrying exactly one construct
/// signature; this digs out that signature's `Function.return_type` node — the
/// node `InstanceType<typeof default>` ultimately extracts. Dispatched on the
/// SUPPLIED `dispatch` so the returned `SemanticNodeId` is comparable.
fn typeof_default_construct_return_node(
    host: &VerterHost,
    host_ctx: &crate::resolver_core::HostResolverContext<'_>,
    dispatch: &ProjectSemanticDispatch<'_>,
    canonical_id: &str,
) -> SemanticNodeId {
    use crate::resolver_core::ResolverContext;
    let _ = host;
    let typeof_node = match dispatch.execute_type_node(SemanticQueryKey::TypeOf {
        value_root: ValueRootKey {
            scope: ScopeId {
                canonical_id: Arc::from(canonical_id),
                local_scope: None,
            },
            name: Arc::from("default"),
        },
    }) {
        QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
        QueryResult::Recursive(node) => node,
        QueryResult::Error(e) => panic!("TypeOf(default) for {canonical_id} errored: {e:?}"),
    };
    let graph = host_ctx.project_type_store().semantic_graph();
    let SemanticNodeData::Object(view) = graph
        .node_data(typeof_node)
        .as_deref()
        .cloned()
        .unwrap_or_else(|| panic!("TypeOf(default) for {canonical_id} must be an Object surface"))
    else {
        panic!("TypeOf(default) for {canonical_id} must be a constructor-like Object surface");
    };
    assert_eq!(
        view.construct_signatures.len(),
        1,
        "the synthesized .vue default's typeof carries exactly one construct signature"
    );
    let ctor_fn = view.construct_signatures[0];
    let SemanticNodeData::Function { return_type, .. } = graph
        .node_data(ctor_fn)
        .as_deref()
        .cloned()
        .unwrap_or_else(|| panic!("construct signature of {canonical_id} must be a Function node"))
    else {
        panic!("construct signature of {canonical_id} must be a Function node");
    };
    return_type
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

// ---------------------------------------------------------------------------
// (conv-1) DEEP CHAIN through `InstanceType<typeof Import>` members. Projecting
//          the EXPANDED path `C.$props.child.$props.child.$props.a` must land on
//          the `number` primitive declared at the deepest link — each `child`
//          hop is an `InstanceType<typeof B>` / `InstanceType<typeof A>` whose
//          construct return is the imported `.vue`'s synthesized instance object.
//
//          NEGATIVE: a WRONG terminal member on that path resolves to an opaque
//          miss (raised `TypeExpr::Unknown`), NOT a broad object / `any`.
// ---------------------------------------------------------------------------

#[test]
fn vue_chain_expanded_path_reaches_number_terminal() {
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

    // C.$props.child : InstanceType<typeof B>  → B instance
    //   .$props.child : InstanceType<typeof A> → A instance
    //     .$props.a   : number
    let terminal = project_vue_default_path(
        &host,
        C,
        &["$props", "child", "$props", "child", "$props", "a"],
    );
    assert_eq!(
        terminal,
        TypeExpr::Primitive(PrimitiveName::Number),
        "the deep chain terminal C.$props.child.$props.child.$props.a is number"
    );

    // Negative: a member that does not exist on A's props is an opaque miss, not
    // a broad object or `any`/`unknown`-as-success.
    let miss = project_vue_default_path(
        &host,
        C,
        &["$props", "child", "$props", "child", "$props", "nope"],
    );
    assert!(
        matches!(miss, TypeExpr::Unknown { .. }),
        "a wrong terminal member must be an opaque miss, got {miss:?}"
    );
}

// ---------------------------------------------------------------------------
// (conv-2) CYCLE through `InstanceType<typeof Import>`. With `A ↔ B` mutually
//          importing and embedding the other's instance as `peer`:
//          - the CONCRETE first hop `A.$props.peer.$props.b` resolves to the
//            `string` declared on B's props (one real hop into the cycle), and
//          - the bounded back-edge `A.$props.peer.$props.peer` resolves to the
//            recursive sentinel (`TypeExpr::RecursiveRef`) — NOT a miss, NOT a
//            hang. Termination is by query identity (the `Instantiate(.vue
//            default)` memo + `push_instantiate_active` guard the convergence
//            routes `typeof`/`InstanceType` through).
// ---------------------------------------------------------------------------

#[test]
fn vue_cycle_expanded_path_concrete_hop_and_bounded_back_edge() {
    const A: &str = "/w/CycA2.vue";
    const B: &str = "/w/CycB2.vue";
    let a_vue = r#"<script setup lang="ts">
import B from './CycB2.vue';
defineProps<{ peer: InstanceType<typeof B>; a: number }>();
</script>
"#;
    let b_vue = r#"<script setup lang="ts">
import A from './CycA2.vue';
defineProps<{ peer: InstanceType<typeof A>; b: string }>();
</script>
"#;
    let host = make_host_with_files(&[(A, a_vue), (B, b_vue)]);

    // One concrete hop into the cycle: A.$props.peer is B's instance; B.$props.b
    // is string.
    let concrete = project_vue_default_path(&host, A, &["$props", "peer", "$props", "b"]);
    assert_eq!(
        concrete,
        TypeExpr::Primitive(PrimitiveName::String),
        "A.$props.peer.$props.b is the string declared on B's props"
    );

    // The back-edge re-enters the SAME `.vue default` identity — bounded to the
    // recursive sentinel, never an infinite expansion. The mere completion of
    // this call is the no-hang proof.
    let back_edge = project_vue_default_path(&host, A, &["$props", "peer", "$props", "peer"]);
    assert!(
        matches!(back_edge, TypeExpr::RecursiveRef { .. }),
        "the cyclic back-edge must be the recursive sentinel, got {back_edge:?}"
    );
}

// ---------------------------------------------------------------------------
// (conv-3) BARE-`Ref` `.vue`-as-type. `defineProps<{ peer: B }>()` references the
//          imported `.vue` component DIRECTLY as a type (not via
//          `InstanceType<typeof B>`). The bare `Ref("B")` lowers to a `.vue`
//          default carrier and EXPANDED projection drives the `Instantiate(.vue
//          default)` branch, so a chain resolves and a MUTUAL cycle bounds
//          SHALLOW.
//
//          IMPORTANT — this LAZY bare-`Ref` mutual route does NOT reach the
//          `push_instantiate_active` guard: each `Instantiate(.vue default)`
//          frame completes and pops before the next side is demanded, and the
//          inner cyclic `peer` lowers in `Navigate` to a `DeclRef` carrier
//          rather than re-dispatching `Instantiate`. The back-edge is therefore
//          a shallow `Object` whose inner `peer` is the bare `Ref { default }`
//          carrier — NOT `RecursiveRef`. (The `push_instantiate_active`
//          short-circuit to `RecursiveRef` is exercised instead by the EAGER
//          same-key self-cycle in `instance_type_self_cycle_hits_active_guard`,
//          where the outer `Published(Expanded)` frame is still active when
//          `typeof Self` re-enters the SAME `(Self, default)` identity.)
// ---------------------------------------------------------------------------

#[test]
fn vue_bare_ref_chain_and_cycle_resolve() {
    // Chain: D imports A as a bare type.
    const A: &str = "/w/A.vue";
    const D: &str = "/w/D.vue";
    let d_vue = r#"<script setup lang="ts">
import A from './A.vue';
defineProps<{ child: A }>();
</script>
"#;
    let host = make_host_with_files(&[(A, A_VUE), (D, d_vue)]);
    // D.$props.child is A's instance; .$props.a is number.
    let terminal = project_vue_default_path(&host, D, &["$props", "child", "$props", "a"]);
    assert_eq!(
        terminal,
        TypeExpr::Primitive(PrimitiveName::Number),
        "bare-Ref chain D.$props.child.$props.a is number"
    );

    // Cycle via bare Ref: E ↔ F mutually reference each other as bare types.
    const E: &str = "/w/BareE.vue";
    const F: &str = "/w/BareF.vue";
    let e_vue = r#"<script setup lang="ts">
import F from './BareF.vue';
defineProps<{ peer: F; e: number }>();
</script>
"#;
    let f_vue = r#"<script setup lang="ts">
import E from './BareE.vue';
defineProps<{ peer: E; f: string }>();
</script>
"#;
    let host = make_host_with_files(&[(E, e_vue), (F, f_vue)]);
    // One concrete hop: E.$props.peer is F's instance; F.$props.f is string.
    let concrete = project_vue_default_path(&host, E, &["$props", "peer", "$props", "f"]);
    assert_eq!(
        concrete,
        TypeExpr::Primitive(PrimitiveName::String),
        "bare-Ref cycle concrete hop E.$props.peer.$props.f is string"
    );
    // The back-edge is bounded SHALLOW — NOT the recursive sentinel. The lazy
    // bare-`Ref` mutual route never re-enters the SAME `(E, default)` frame while
    // it is active: each `Instantiate(.vue default)` side completes and pops
    // before the next side is demanded (the inner `peer` lowers in `Navigate` to
    // a `DeclRef` carrier, so `push_instantiate_active` is never reached at the
    // terminal). The back-edge is therefore E's shallow instance object whose
    // inner cyclic `peer` is left as the bare `Ref { name: "default" }` carrier.
    let back_edge = project_vue_default_path(&host, E, &["$props", "peer", "$props", "peer"]);

    let TypeExpr::Object(instance) = &back_edge else {
        panic!("bare-Ref cyclic back-edge must be E's shallow instance object, got {back_edge:?}");
    };
    let props = instance
        .properties
        .iter()
        .find_map(|member| match member {
            verter_type_expr::ObjectMember::Property(prop) if prop.name == "$props" => {
                Some(&prop.ty)
            }
            _ => None,
        })
        .expect("E shallow instance must carry $props");

    let TypeExpr::Object(props_obj) = props else {
        panic!("E.$props must be an object, got {props:?}");
    };

    let e = props_obj
        .properties
        .iter()
        .find_map(|member| match member {
            verter_type_expr::ObjectMember::Property(prop) if prop.name == "e" => Some(&prop.ty),
            _ => None,
        })
        .expect("E.$props.e must exist");
    assert_eq!(*e, TypeExpr::Primitive(PrimitiveName::Number));

    let peer = props_obj
        .properties
        .iter()
        .find_map(|member| match member {
            verter_type_expr::ObjectMember::Property(prop) if prop.name == "peer" => Some(&prop.ty),
            _ => None,
        })
        .expect("E.$props.peer must exist");
    assert!(
        matches!(peer, TypeExpr::Ref { name, type_arguments }
            if name.as_ref() == "default" && type_arguments.is_empty()),
        "bare-Ref cycle must stop shallow at inner peer Ref(default), got {peer:?}"
    );
}

// ---------------------------------------------------------------------------
// (conv-4) CONVERGENCE DISCRIMINATOR — provenance, not bare node identity. The
//          construct-signature RETURN node of `TypeOf(A.vue default)` (the node
//          `InstanceType<typeof A>` extracts) MUST be PRODUCED BY the keyed
//          `Instantiate{ A.vue default, [] }` query, i.e. it carries an
//          `OriginEdgeKind::Instantiate` provenance edge.
//
//          Why provenance and not `SemanticNodeId` equality: semantic nodes are
//          structurally interned, so a directly-lowered `{ $props, $emit }`
//          object and the `Instantiate(.vue default)` object can intern to the
//          SAME id even when produced by different paths — node identity does
//          NOT discriminate the convergence. The `Instantiate` ORIGIN EDGE does:
//          `build_vue_default_instance` stamps it, the pre-convergence direct
//          `build_typeof` lowering of `function_signature.return_type` does NOT.
//          This test dispatches ONLY `TypeOf` (never `Instantiate` directly), so
//          the edge can only appear if `build_typeof` itself routed through
//          `Instantiate(.vue default)`. It FAILS against the pre-convergence
//          tree (no Instantiate edge on the typeof construct return) and PASSES
//          post-convergence. A second assertion pins the post-convergence node
//          identity to `Instantiate(.vue default)` as the strongest form.
// ---------------------------------------------------------------------------

#[test]
fn typeof_construct_return_is_produced_by_instantiate_vue_default() {
    use crate::resolver_core::ResolverContext;
    use crate::semantic_query::OriginEdgeKind;

    const A: &str = "/w/A.vue";
    let host = make_host_with_files(&[(A, A_VUE)]);

    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);

    // ONLY TypeOf is dispatched here — so an Instantiate provenance edge on the
    // construct return can ONLY have come from build_typeof routing through it.
    let typeof_return = typeof_default_construct_return_node(&host, &host_ctx, &dispatch, A);
    let graph = host_ctx.project_type_store().semantic_graph();
    let has_instantiate_origin = graph
        .origins(typeof_return)
        .into_iter()
        .any(|(kind, _)| kind == OriginEdgeKind::Instantiate);
    assert!(
        has_instantiate_origin,
        "TypeOf(.vue default)'s construct return must be PRODUCED BY \
         Instantiate(.vue default) (carry an Instantiate origin edge), not \
         directly re-lowered in build_typeof"
    );

    // Strongest form: once converged, the construct return IS the same node the
    // keyed `Instantiate(.vue default)` query resolves to.
    let instantiate_node = instantiate_vue_default_node(&host, &dispatch, A);
    assert_eq!(
        typeof_return, instantiate_node,
        "the construct return of TypeOf(.vue default) is the Instantiate(.vue default) node"
    );
}

// ---------------------------------------------------------------------------
// (guard-1) ACTIVE-INSTANTIATION-GUARD DISCRIMINATOR — `push_instantiate_active`.
//           A SELF-cyclic `.vue` (`Self.vue` references its OWN instance as
//           `InstanceType<typeof Self>`) projected EAGERLY under
//           `Published(Expanded)` is the case that ACTUALLY exercises the
//           `push_instantiate_active` / `is_instantiate_active` guard — the
//           mutual bare-`Ref` fixture (conv-3) does NOT (it bounds shallow via a
//           `Navigate` `DeclRef` carrier).
//
//           Mechanism: dispatching `Instantiate(Self, default, Published(Expanded))`
//           directly pushes `(Self, "default")` and lowers Self's instance shape
//           eagerly. The `self` member `InstanceType<typeof Self>` is lowered in
//           Expanded mode (NOT the Navigate carrier path), so `typeof Self`
//           routes through `build_synthesized_vue_default_construct_object`,
//           which re-issues `Instantiate(Self, default, StructuralTransit(Navigate))`.
//           That is a DIFFERENT context key from the outer `Published(Expanded)`
//           one, so the memo's same-key sentinel does NOT fire — instead the
//           re-entry calls `push_instantiate_active((Self, "default"))`, finds the
//           SAME identity already active (the outer frame has not popped), returns
//           `false`, and short-circuits to `Opaque(RecursiveRef)`. So
//           `$props.self` raises to `TypeExpr::RecursiveRef`.
//
//           DISCRIMINATING: this test FAILS (and is what proves the guard is
//           reached) if `push_instantiate_active` is neutralized to always admit.
//           With the guard disabled the re-entry does NOT short-circuit on the
//           `Instantiate` same-key sentinel (the inner dispatch carries the
//           DIFFERENT `StructuralTransit(Navigate)` context key, so that sentinel
//           never fires). Instead the inner `TypeOf{Self/default}` re-entry hits
//           the `TypeOf` memo's in-flight sentinel, which yields `Opaque(Miss)` —
//           a NON-`Instantiate` sentinel, so the terminal raises to a Miss-derived
//           shape rather than `TypeExpr::RecursiveRef`. The active-instantiation
//           guard is therefore the ONLY path that produces the `RecursiveRef`
//           sentinel here. Verified by temporarily returning `true` unconditionally
//           from `push_instantiate_active` (see the report): the eager self-cycle
//           then does NOT raise to `RecursiveRef`.
// ---------------------------------------------------------------------------

const SELF_VUE: &str = r#"<script setup lang="ts">
import Self from './Self.vue';
defineProps<{ self: InstanceType<typeof Self>; marker: number }>();
</script>
"#;

/// Project an EXPANDED member `path` rooted at a `Published(Expanded)`
/// `Instantiate(.vue default)` of `canonical_id` (NOT the `Navigate` base
/// [`project_vue_default_path`] uses). The EAGER `Published(Expanded)` base is
/// what keeps the `(canonical, "default")` frame ACTIVE while the instance
/// shape's members lower — the precondition for the `push_instantiate_active`
/// guard to fire on a self-cyclic `InstanceType<typeof Self>` member.
fn project_vue_default_path_eager(
    host: &VerterHost,
    canonical_id: &str,
    path: &[&str],
) -> TypeExpr {
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);

    let _whole_hash = host
        .ensure_indexed_ready(canonical_id)
        .expect("indexed ready")
        .whole_hash;
    // EAGER base: `Published(Expanded)` (NOT structural-transit/Navigate), so the
    // body of the instance shape is lowered while `(canonical, "default")` is on
    // the active-instantiation stack.
    let base = match dispatch.execute_type_node(SemanticQueryKey::Instantiate {
        base: crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from(canonical_id),
            Arc::from("default"),
        ),
        args: Arc::from(Vec::new().into_boxed_slice()),
        context: crate::semantic_query::InstantiateContext::new(
            ProjectionReductionContext::published(ProjectionMode::Expanded),
            Default::default(),
        ),
    }) {
        QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
        QueryResult::Recursive(node) => node,
        QueryResult::Error(e) => {
            panic!("eager Instantiate(.vue default) base for {canonical_id} errored: {e:?}")
        }
    };
    let segments: Arc<[PathSegment]> = path
        .iter()
        .map(|s| PathSegment::Member(Arc::from(*s)))
        .collect::<Vec<_>>()
        .into();
    let terminal = match dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base,
        path: segments,
        context: ProjectionReductionContext::published(ProjectionMode::Expanded),
    }) {
        QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
        QueryResult::Recursive(node) => node,
        QueryResult::Error(e) => {
            panic!("ProjectPath {path:?} for {canonical_id} errored: {e:?}")
        }
    };
    dispatch
        .raise_node_to_type_expr(terminal)
        .unwrap_or_else(|| panic!("terminal of {path:?} for {canonical_id} must raise to TypeExpr"))
}

#[test]
fn instance_type_self_cycle_hits_active_guard() {
    const SELF: &str = "/w/Self.vue";
    let host = make_host_with_files(&[(SELF, SELF_VUE)]);

    // Concrete sibling member proves the instance shape lowered (the path
    // machinery and the eager base both work): `$props.marker` is `number`.
    assert_eq!(
        project_vue_default_path_eager(&host, SELF, &["$props", "marker"]),
        TypeExpr::Primitive(PrimitiveName::Number),
        "Self.$props.marker is the number declared alongside the self-cyclic member"
    );

    // The self-cyclic member: `$props.self` is `InstanceType<typeof Self>`. Under
    // the EAGER `Published(Expanded)` base the `(Self, default)` frame is still
    // active when `typeof Self` re-enters the SAME identity, so
    // `push_instantiate_active` short-circuits to the recursive sentinel. The mere
    // completion of this call is the no-hang proof.
    let self_member = project_vue_default_path_eager(&host, SELF, &["$props", "self"]);
    assert!(
        matches!(self_member, TypeExpr::RecursiveRef { .. }),
        "the active-instantiation guard must bound the eager self-cycle to the \
         recursive sentinel, got {self_member:?}"
    );
}
