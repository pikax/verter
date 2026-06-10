//! Lifecycle canary suite — negative-result recovery, evicted-owner
//! reload, and the over-invalidation guard.
//!
//! The owner-upsert path has no eager reverse-dependent invalidation
//! cascade. These canary tests are the coherent named gate for the
//! lifecycle slices of the lazy fact-validation substrate:
//!
//!  - **Negative-result recovery** — a previously-missing dependency
//!    appears; the consumer's stale `semanticMiss` is invalidated and
//!    the recomputed type is the concrete object.
//!  - **Evicted-owner reload** — `get_component_meta` on an explicitly
//!    evicted owner reloads to authoritative state and reflects a
//!    dependency edit.
//!  - **Over-invalidation guard** — editing/adding a file the consumer
//!    does NOT import must leave the consumer's warm slot / result
//!    warm and unchanged (negative: the substrate must NOT
//!    over-invalidate).
//!
//! Every mutation routes through the production [`harness::upsert`]
//! helper (plain `VerterHost::upsert`): no eager cascade runs and the
//! upsert performs no own-canonical query-identity cache drain, so only
//! lazy fact-validation drives invalidation (and, for the
//! over-invalidation guard, only fact-validation may decide to leave a
//! warm result alone).

#![cfg(test)]

use verter_session::{CompileProfile, FileKind};
use verter_type_expr::{PrimitiveName, TypeExpr};

use crate::canary_harness::{
    meta_hits, meta_misses, prime_compile, standalone_host, upsert, workspace_host,
};

/// Resolve the evaluated type of a single prop from an
/// `ExpandedComponentTypes` snapshot.
fn evaluated_prop<'a>(
    types: &'a verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    name: &str,
) -> &'a TypeExpr {
    &types
        .props
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("missing evaluated prop `{name}`"))
        .r#type
}

/// Canary — negative-result recovery when a missing dependency
/// appears.
///
/// `Comp.vue` has `defineProps<{ ui: typeof theme }>()` where
/// `import theme from './theme'` initially resolves to nothing. The
/// first evaluation publishes a NEGATIVE result for `ui` — the
/// unresolved `typeof theme` carrier (`TypeExpr::TypeOf`), which the
/// no-poison publication rule keeps in place of a `semanticMiss`
/// sentinel. When `./theme` is later ADDED, the consumer must
/// recover: the stale negative result is invalidated and the
/// recomputed `ui` type is the concrete object.
///
/// Discrimination property: the negative resolution's `ImportRoute`
/// derived fact records `./theme` as unresolved; the fact-validation
/// oracle re-resolves that known-miss specifier against the current
/// workspace generation at validate time
/// (`generation_current_import_route_hash`). Reverting that — letting
/// the warm negative resolution validate against its own stale
/// `ImportRoute` snapshot — keeps the re-evaluation returning the
/// unresolved carrier and the `TypeExpr::Object` assertion fails.
#[test]
fn negative_result_recovers_when_missing_dependency_appears() {
    let host = standalone_host();
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import theme from './theme'\n\
         defineProps<{ ui: typeof theme }>()\n\
         </script>\n\
         <template><div /></template>\n",
        FileKind::VueSfc,
    );

    // First evaluation — `./theme` is missing, so `ui` is a
    // negative (semanticMiss) result.
    let initial = host
        .evaluate_types("/src/Comp.vue")
        .expect("initial evaluate_types must resolve the owner");
    assert!(
        matches!(
            evaluated_prop(&initial, "ui"),
            TypeExpr::Unknown { .. } | TypeExpr::TypeOf(_)
        ),
        "precondition: with `./theme` missing, `ui` must be a NEGATIVE result — the \
         unresolved `typeof theme` carrier (or a miss sentinel), never a concrete \
         object — got {:?}",
        evaluated_prop(&initial, "ui")
    );

    // ADD `./theme` through `harness::upsert` — no eager cascade
    // evicts `/src/Comp.vue`'s artifacts, so the lazy fact-validation
    // substrate is what must detect that `./theme` is now resolvable.
    upsert(
        &host,
        "/src/theme.ts",
        "export default {\n  item: \"item\",\n  body: \"body\",\n}\n",
        FileKind::NonSfc,
    );

    // The consumer must recover: the stale negative result is
    // invalidated and `ui` is now the concrete object.
    let recovered = host
        .evaluate_types("/src/Comp.vue")
        .expect("post-add evaluate_types must resolve the owner");
    match evaluated_prop(&recovered, "ui") {
        TypeExpr::Object(obj) => {
            let members: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|m| match m {
                    verter_type_expr::ObjectMember::Property(p) => Some(p.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                members.contains(&"item") && members.contains(&"body"),
                "the recovered `typeof theme` MUST expose `item` + `body` — \
                 the negative result was invalidated and recomputed against \
                 the now-present dependency. Got members {members:?}"
            );
        }
        other => panic!(
            "`ui` MUST recover to a concrete object once `./theme` exists — \
             a stale negative result would still be the unresolved `typeof \
             theme` carrier / miss sentinel. Got {other:?}"
        ),
    }
}

/// Canary — evicted owner reloads to authoritative state after a
/// dependency edit.
///
/// `Comp.vue` imports `Props` from `types.ts`. After a `types.ts`
/// edit through `harness::upsert` (no eager cascade evicts the
/// owner's artifacts) the owner is EXPLICITLY evicted with
/// `host.evict`. The next `get_component_meta` must reload the evicted
/// owner to authoritative state and reflect the edit.
///
/// Discrimination property: an evicted owner must reload through
/// `ensure_loaded` rather than honour a stale
/// `FileArtifactStore::get_any`-derived whole-hash. Reverting that —
/// letting `current_or_read_whole_hash` accept the stale `get_any`
/// hash for an evicted canonical — runs `get_component_meta` on the
/// stale identity and returns `None`; the `.expect(...)` on the
/// post-edit call fails.
#[test]
fn evicted_owner_reloads_to_authoritative_state_after_dep_edit() {
    let (workspace, host) = workspace_host(&[
        (
            "/workspace/src/types.ts",
            "export interface Props { a: number; }\n",
        ),
        (
            "/workspace/src/Comp.vue",
            "<script setup lang=\"ts\">\n\
             import type { Props } from '/workspace/src/types'\n\
             defineProps<Props>()\n\
             </script>\n\
             <template><div/></template>\n",
        ),
    ]);

    // Cold pass — captures the pre-edit shape into all caches.
    let pre = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("cold get_component_meta must resolve");
    let a_pre = pre
        .props
        .into_iter()
        .find(|p| p.name == "a")
        .expect("cold meta must publish prop `a`");
    assert!(
        matches!(a_pre.type_expr, TypeExpr::Primitive(PrimitiveName::Number)),
        "precondition: cold `a` prop must be `number` — got {:?}",
        a_pre.type_expr
    );

    // Edit `types.ts` through `harness::upsert` (no eager cascade),
    // then explicitly evict the owner.
    let edited = "export interface Props { a: string; }\n";
    workspace.inject_file(
        "/workspace/src/types.ts".into(),
        std::sync::Arc::from(edited),
    );
    upsert(&host, "/workspace/src/types.ts", edited, FileKind::NonSfc);
    host.evict("/workspace/src/Comp.vue");

    // The evicted owner must reload to authoritative state and reflect
    // the edit.
    let post = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("get_component_meta on an evicted owner must reload and resolve");
    let a_post = post
        .props
        .into_iter()
        .find(|p| p.name == "a")
        .expect("reloaded meta must publish prop `a`");
    assert!(
        matches!(a_post.type_expr, TypeExpr::Primitive(PrimitiveName::String)),
        "the reloaded `a` prop MUST carry the edited `string` type — an \
         evicted owner that honoured a stale whole-hash would return None \
         or the stale `number` shape. Got {:?}",
        a_post.type_expr
    );
}

/// Canary — over-invalidation guard, compile tier.
///
/// `Comp.vue` imports a macro type from `types.ts`. Adding AND then
/// editing a wholly UNRELATED file `unrelated.ts` (which `Comp.vue`
/// does NOT import) through `harness::upsert` must leave `Comp.vue`'s
/// warm compile slot warm and unchanged.
///
/// Discrimination property: the compile slot's `fact_dep_signature`
/// records ONLY the facts of files the consumer actually imports;
/// `/src/unrelated.ts` is not in it. The warm-hit oracle
/// `compile_slot_is_warm` therefore still validates after the
/// unrelated upsert. An over-eager "always cold after any upsert"
/// regression — or a signature that bubbled unrelated facts — would
/// flip `warm_after` to `false` and fail the test.
#[test]
fn unrelated_file_upsert_keeps_compile_slot_warm() {
    let host = standalone_host();
    upsert(
        &host,
        "/src/types.ts",
        "export interface MyType { foo: string }\n",
        FileKind::NonSfc,
    );
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { MyType } from './types'\n\
         defineProps<MyType>()\n\
         </script>\n\
         <template><div/></template>\n",
        FileKind::VueSfc,
    );

    let profile = CompileProfile::default();
    prime_compile(&host, "/src/Comp.vue");
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "precondition: Comp.vue must have a warm compile slot after prime"
    );

    // ADD a wholly unrelated file through `harness::upsert`.
    upsert(
        &host,
        "/src/unrelated.ts",
        "export const x = 1;\n",
        FileKind::NonSfc,
    );
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "adding a file the consumer does NOT import MUST leave its warm \
         compile slot warm — fact-validation is path-precise"
    );

    // EDIT that unrelated file through `harness::upsert`.
    upsert(
        &host,
        "/src/unrelated.ts",
        "export const x = 2;\n",
        FileKind::NonSfc,
    );
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "editing a file the consumer does NOT import MUST leave its warm \
         compile slot warm — a flip to not-warm here means the substrate \
         over-invalidates on unrelated edits"
    );

    // The user-visible output is the still-valid warm compilation — a
    // recompile is a no-op and the assembled module is unchanged.
    let response = crate::canary_harness::compile_main(&host, "/src/Comp.vue")
        .expect("the warm compilation remains readable after unrelated edits");
    assert!(
        !response.diagnostics.has_errors,
        "the warm compilation must stay error-free after unrelated upserts: \
         {:?}",
        response.diagnostics
    );
    assert!(
        response.code.contains("MyType"),
        "the warm compiled output must still resolve the imported `MyType` \
         macro type — got: {}",
        response.code
    );
}

/// Canary — over-invalidation guard, component-meta tier.
///
/// `Comp.vue` imports `Foo` from `types.ts` and warm-caches a
/// `get_component_meta` result. Adding a wholly UNRELATED file
/// `other.ts` through `harness::upsert` must leave the owner's warm
/// `ComponentMetaResultDb` entry warm — the next `get_component_meta`
/// HITS the warm cache and returns the unchanged props.
///
/// Discrimination property: the published `ComponentMetaResultEntry`
/// signature records only the facts of files the owner's resolution
/// actually walked; `/workspace/src/other.ts` is not among them.
/// `validates_fact_signature` therefore still passes after the
/// unrelated upsert and the warm hit is served. An over-eager
/// regression that invalidated on any upsert — or a signature that
/// bubbled unrelated facts — would force a miss and the
/// `meta_hits` advance assertion fails.
#[test]
fn unrelated_file_upsert_keeps_component_meta_warm() {
    let (workspace, host) = workspace_host(&[
        (
            "/workspace/src/types.ts",
            "export interface Foo { a: number; }\n",
        ),
        (
            "/workspace/src/Comp.vue",
            "<script setup lang=\"ts\">\n\
             import type { Foo } from '/workspace/src/types'\n\
             defineProps<Foo>()\n\
             </script>\n\
             <template><div/></template>\n",
        ),
    ]);

    let prime = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("prime get_component_meta must resolve");
    let prime_names: Vec<String> = prime.props.iter().map(|p| p.name.clone()).collect();

    // Warm sanity — an unedited second call must hit the warm cache.
    let hits_before = meta_hits(&host);
    let _ = host.get_component_meta("/workspace/src/Comp.vue");
    let hits_after_sanity = meta_hits(&host);
    assert!(
        hits_after_sanity > hits_before,
        "warm sanity: an unedited second get_component_meta must hit the \
         warm cache (hits {hits_before} -> {hits_after_sanity})"
    );
    let misses_before = meta_misses(&host);

    // ADD a wholly unrelated file through `harness::upsert`.
    let unrelated = "export interface Other { z: boolean }\n";
    workspace.inject_file(
        "/workspace/src/other.ts".into(),
        std::sync::Arc::from(unrelated),
    );
    upsert(
        &host,
        "/workspace/src/other.ts",
        unrelated,
        FileKind::NonSfc,
    );

    // The owner's warm result MUST survive: the next query HITS the
    // warm cache and does NOT miss.
    let hits_before_query = meta_hits(&host);
    let after = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("post-unrelated-upsert get_component_meta must resolve");
    let hits_after_query = meta_hits(&host);
    let misses_after = meta_misses(&host);
    assert!(
        hits_after_query > hits_before_query,
        "adding a file the owner does NOT import MUST leave its warm \
         ComponentMetaResultDb entry warm — the next query must HIT \
         (hits {hits_before_query} -> {hits_after_query})"
    );
    assert_eq!(
        misses_after, misses_before,
        "the unrelated upsert must NOT force a result-cache miss on the \
         owner — fact-validation is path-precise (misses stayed at \
         {misses_before}, observed {misses_after})"
    );

    // User-visible output: the warm-served props are unchanged.
    let after_names: Vec<String> = after.props.iter().map(|p| p.name.clone()).collect();
    assert_eq!(
        after_names, prime_names,
        "the warm-served props must be IDENTICAL to the pre-upsert props — \
         an unrelated edit must not perturb the owner's metadata"
    );
}
