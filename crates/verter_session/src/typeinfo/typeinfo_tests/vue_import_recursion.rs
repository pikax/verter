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

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    DeclIdentity, ProjectionMode, ProjectionReductionContext, QueryResult, SemanticNodeData,
    SemanticQueryApi, SemanticQueryKey,
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
