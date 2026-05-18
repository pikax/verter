//! Component-meta cross-file lazy-invalidation canary suite.
//!
//! The owner-upsert path has no eager reverse-dependent invalidation
//! cascade. These canary tests are the coherent named gate that proves
//! the lazy fact-validation substrate backs every `getComponentMeta`
//! cross-file invalidation scenario.
//!
//! Every mutation routes through the skip-own-drain hook (the
//! [`harness::upsert`] helper → `upsert_skipping_own_canonical_drain_for_tests`),
//! which suppresses the post-commit own-canonical query-identity cache
//! drain. The dependency edits exercised here are cross-file, so the
//! drain skip does not change the dependency's effect on the owner —
//! but routing the whole suite through the one hook keeps the wiring
//! uniform with the owner-self-edit canaries, where the drain skip is
//! load-bearing.
//!
//! Each test:
//!  1. Sets up an owner SFC + dependency file(s) and primes a warm
//!     `get_component_meta` result.
//!  2. Mutates the dependency through [`harness::upsert`] — no eager
//!     cascade runs, so the owner's warm `ComponentMetaResultDb` entry
//!     physically survives. The ONLY mechanism that can invalidate it
//!     is `validates_fact_signature` on the warm-hit path.
//!  3. Asserts the lazy semantics: the warm-hit lookup MISSES
//!     (`component_meta_result_cache_misses` advances), the resolver
//!     recomputes, and the recomputed `ComponentMetaAnalysis` props
//!     carry the new dependency content.
//!
//! These tests deliberately do NOT assert physical cache emptiness
//! (`cached_meta_payload.is_none()`): a warm entry can survive the
//! dependency edit and still be lazily rejected on read. The gate is
//! stale-miss + recompute + correct user-visible props.

#![cfg(test)]

use verter_session::FileKind;

#[path = "block_2_canary/harness.rs"]
mod harness;

use harness::{meta_hits, meta_misses, upsert, workspace_host};

/// Sorted slot-binding names for the named slot of a
/// `get_component_meta` result.
fn slot_binding_names(
    meta: &verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    slot: &str,
) -> Vec<String> {
    let mut names: Vec<String> = meta
        .slots
        .iter()
        .find(|s| s.name == slot)
        .unwrap_or_else(|| panic!("missing slot `{slot}`"))
        .bindings
        .iter()
        .map(|b| b.name.clone())
        .collect();
    names.sort_unstable();
    names
}

/// Assert an unedited second `get_component_meta` is a warm hit, then
/// return the pre-edit miss counter — establishes the baseline that
/// makes the post-edit miss-delta a discriminating signal.
fn warm_sanity_then_misses(host: &verter_session::VerterHost, owner: &str) -> u64 {
    let hits_before = meta_hits(host);
    let _ = host.get_component_meta(owner);
    let hits_after = meta_hits(host);
    assert!(
        hits_after > hits_before,
        "warm sanity: an unedited second get_component_meta on {owner} must \
         hit the warm cache (hits {hits_before} -> {hits_after}) — without a \
         round-tripping warm hit the post-edit miss-delta is not discriminating"
    );
    meta_misses(host)
}

/// Canary — imported prop type edit.
///
/// `defineProps<Foo>` over a `Foo` interface imported from a
/// workspace `.ts`. Editing a `Foo` member's type must MISS the
/// owner's warm `ComponentMetaResultDb` entry and the recomputed
/// props must carry the new member type.
///
/// Discrimination property: `ComponentMetaResultDb::get_with_view`
/// runs `view.validates_fact_signature(&entry.fact_dep_signature)` on
/// the warm-hit path. The entry's signature records the dep's
/// pre-edit parse facts; the post-edit facts mismatch. Removing that
/// `validates_fact_signature` check serves the stale entry as a hit
/// and the asserted miss-delta never materialises.
#[test]
fn imported_prop_type_edit_misses_warm_component_meta() {
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

    let prime = host.get_component_meta("/workspace/src/Comp.vue");
    assert!(prime.is_some(), "prime get_component_meta must resolve");

    let misses_before = warm_sanity_then_misses(&host, "/workspace/src/Comp.vue");

    // Edit the imported member's type — no eager cascade; the owner's
    // warm ComponentMetaResultDb entry survives the dependency edit.
    let edited = "export interface Foo { a: string; }\n";
    workspace.inject_file(
        "/workspace/src/types.ts".into(),
        std::sync::Arc::from(edited),
    );
    upsert(&host, "/workspace/src/types.ts", edited, FileKind::NonSfc);

    let after = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("post-edit get_component_meta must resolve");
    let misses_after = meta_misses(&host);
    assert!(
        misses_after > misses_before,
        "an imported prop-type edit MUST miss the owner's warm \
         ComponentMetaResultDb entry via validates_fact_signature \
         (misses {misses_before} -> {misses_after})"
    );

    // User-visible output: the recomputed `a` prop is `string`.
    let a_prop = after
        .props
        .iter()
        .find(|p| p.name == "a")
        .expect("recomputed meta must publish prop `a`");
    assert!(
        !matches!(
            a_prop.type_expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        ),
        "the recomputed `a` prop must NOT be the stale `number` type — \
         got {:?}",
        a_prop.type_expr
    );
    assert!(
        matches!(
            a_prop.type_expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "the recomputed `a` prop MUST carry the edited `string` type — \
         a stale warm hit would still report `number`. Got {:?}",
        a_prop.type_expr
    );
}

/// Canary — barrel re-export leaf edit.
///
/// `Button.vue` imports `ButtonProps` through a barrel `index.ts`
/// that `export *`s the leaf `types.ts`. Editing the leaf
/// `types.ts` — reached through the barrel — must surface in the
/// owner's next `get_component_meta`: the resolution MISSES the
/// owner's warm result, the resolver recomputes through
/// owner → barrel → leaf, and the recomputed props carry the new
/// leaf content.
///
/// A barrel-importing owner does not round-trip a `ComponentMetaResultDb`
/// warm hit (the `export *` barrel-hop route facts are not warm-stable
/// on the published signature), so this owner recomputes on every
/// query and `component_meta_result_cache_misses` advances on the
/// post-edit call by construction. The discriminating gate is
/// therefore NOT `validates_fact_signature` but the import-route
/// rebuild: the cold recompute re-resolves the `export *` hop against
/// the leaf's CURRENT content.
///
/// Discrimination property: the import-route / barrel-hop resolution
/// is re-keyed on the leaf's current whole-hash. A stale import-route
/// cache pinned to the pre-edit barrel/leaf content would surface the
/// pre-edit member name (`initial`) in the post-edit prop set — the
/// `!contains("initial")` + `contains("renamed")` pair fails against
/// such a regression.
#[test]
fn barrel_reexport_leaf_edit_recomputes_with_new_content() {
    let (workspace, host) = workspace_host(&[
        (
            "/workspace/src/types.ts",
            "export interface ButtonProps { initial: string }\n",
        ),
        ("/workspace/src/index.ts", "export * from './types'\n"),
        (
            "/workspace/src/Button.vue",
            "<script setup lang=\"ts\">\n\
             import type { ButtonProps } from '/workspace/src/index'\n\
             defineProps<ButtonProps>()\n\
             </script>\n\
             <template><div/></template>\n",
        ),
    ]);

    let prime = host.get_component_meta("/workspace/src/Button.vue");
    assert!(prime.is_some(), "prime get_component_meta must resolve");
    let before_names: Vec<String> = prime
        .unwrap()
        .props
        .iter()
        .map(|p| p.name.clone())
        .collect();
    assert!(
        before_names.iter().any(|n| n == "initial"),
        "pre-edit props must include `initial` from ButtonProps through the \
         barrel — got {before_names:?}"
    );

    let misses_before = meta_misses(&host);

    // Edit the barrel LEAF (rename the member) — no eager cascade; the
    // owner's warm result survives the dependency edit.
    let edited_leaf = "export interface ButtonProps { renamed: string }\n";
    workspace.inject_file(
        "/workspace/src/types.ts".into(),
        std::sync::Arc::from(edited_leaf),
    );
    upsert(
        &host,
        "/workspace/src/types.ts",
        edited_leaf,
        FileKind::NonSfc,
    );

    let after = host
        .get_component_meta("/workspace/src/Button.vue")
        .expect("post-edit get_component_meta must resolve");
    let misses_after = meta_misses(&host);
    // Recomputation occurs: the barrel-importing owner is not
    // warm-served, so the post-edit query takes the cold resolver
    // path (a miss against the result cache).
    assert!(
        misses_after > misses_before,
        "the post-edit query MUST take the cold resolver path — a barrel \
         importer does not warm-serve, so the result-cache miss counter \
         advances (misses {misses_before} -> {misses_after})"
    );

    // User-visible output: the recomputed prop set carries the renamed
    // member and NOT the stale one. A stale import-route cache pinned
    // to the pre-edit barrel content would still report `initial`.
    let after_names: Vec<String> = after.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        after_names.iter().any(|n| n == "renamed"),
        "the recomputed props MUST carry the renamed member `renamed` — \
         the import-route resolution re-resolved the `export *` hop \
         against the leaf's edited content. Got {after_names:?}"
    );
    assert!(
        !after_names.iter().any(|n| n == "initial"),
        "the recomputed props must NOT carry the stale member `initial` — \
         a stale import-route cache pinned to the pre-edit barrel/leaf \
         content would still report it. Got {after_names:?}"
    );
}

/// Canary — transitive type dependency edit.
///
/// `App.vue` → `types.ts` → `nested.ts`. `App.vue` imports `Props`
/// from `types.ts`; `Props` references `Nested` from `nested.ts`.
/// Editing the transitive grandparent `nested.ts` must miss the
/// owner's warm `ComponentMetaResultDb` entry and the recomputed
/// props must carry the new transitive type.
///
/// Discrimination property: the cold compute walks the full
/// `App.vue → types.ts → nested.ts` declaration graph and records
/// `nested.ts`'s parse facts into the published signature.
/// `validates_fact_signature` catches the transitive-fact mismatch on
/// the warm-hit path. Removing that check serves the stale entry and
/// the miss-delta never materialises.
#[test]
fn transitive_type_dep_edit_misses_warm_component_meta() {
    let (workspace, host) = workspace_host(&[
        (
            "/workspace/src/types.ts",
            "import type { Nested } from './nested'\n\
             export interface Props { msg: Nested }\n",
        ),
        ("/workspace/src/nested.ts", "export type Nested = string\n"),
        (
            "/workspace/src/App.vue",
            "<script setup lang=\"ts\">\n\
             import type { Props } from '/workspace/src/types'\n\
             defineProps<Props>()\n\
             </script>\n\
             <template><div/></template>\n",
        ),
    ]);

    let prime = host.get_component_meta("/workspace/src/App.vue");
    assert!(prime.is_some(), "prime get_component_meta must resolve");
    let msg_pre = prime
        .unwrap()
        .props
        .into_iter()
        .find(|p| p.name == "msg")
        .expect("prime meta must publish prop `msg`");
    assert!(
        matches!(
            msg_pre.type_expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
        ),
        "pre-edit `msg` prop must be `string` (Nested = string) — got {:?}",
        msg_pre.type_expr
    );

    let misses_before = warm_sanity_then_misses(&host, "/workspace/src/App.vue");

    // Edit the TRANSITIVE grandparent `nested.ts` — no eager cascade;
    // the owner's warm result survives the dependency edit.
    let edited_nested = "export type Nested = number\n";
    workspace.inject_file(
        "/workspace/src/nested.ts".into(),
        std::sync::Arc::from(edited_nested),
    );
    upsert(
        &host,
        "/workspace/src/nested.ts",
        edited_nested,
        FileKind::NonSfc,
    );

    let after = host
        .get_component_meta("/workspace/src/App.vue")
        .expect("post-edit get_component_meta must resolve");
    let misses_after = meta_misses(&host);
    assert!(
        misses_after > misses_before,
        "a transitive type-dependency edit MUST miss the owner's warm \
         ComponentMetaResultDb entry via validates_fact_signature \
         (misses {misses_before} -> {misses_after})"
    );

    // User-visible output: the recomputed `msg` prop reflects the new
    // transitive type.
    let msg_post = after
        .props
        .into_iter()
        .find(|p| p.name == "msg")
        .expect("recomputed meta must publish prop `msg`");
    assert!(
        matches!(
            msg_post.type_expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        ),
        "the recomputed `msg` prop MUST carry the edited transitive type \
         `number` (Nested = number) — a stale warm hit would still report \
         `string`. Got {:?}",
        msg_post.type_expr
    );
}

/// Canary — route-surface dependency edit.
///
/// `defineProps<RProps>()` over an imported type. Resolving the macro
/// root walks the named-type export route — the route walk observes
/// the route DEP's `DerivedFactHash{Route}` participant facts into
/// the published signature. Editing the route source type must miss
/// the owner's warm `ComponentMetaResultDb` entry and the recomputed
/// props must carry the new route-surface shape.
///
/// Discrimination property: cross-file route facts flow into the
/// published signature; `validates_fact_signature` catches the route
/// DEP's `DerivedFactHash{Route}` mismatch on the warm-hit path.
/// Removing that check serves the stale entry and the miss-delta
/// never materialises.
#[test]
fn route_surface_dep_edit_misses_warm_component_meta() {
    let (workspace, host) = workspace_host(&[
        (
            "/workspace/src/types.ts",
            "export interface RProps { a: number; }\n",
        ),
        (
            "/workspace/src/Comp.vue",
            "<script setup lang=\"ts\">\n\
             import type { RProps } from '/workspace/src/types'\n\
             defineProps<RProps>()\n\
             </script>\n\
             <template><div/></template>\n",
        ),
    ]);

    let prime = host.get_component_meta("/workspace/src/Comp.vue");
    assert!(prime.is_some(), "prime get_component_meta must resolve");

    let misses_before = warm_sanity_then_misses(&host, "/workspace/src/Comp.vue");

    // Edit the route source type — `RProps` gains `b`. No eager
    // cascade; the owner's warm result survives the dependency edit.
    let edited = "export interface RProps { a: number; b: string; }\n";
    workspace.inject_file(
        "/workspace/src/types.ts".into(),
        std::sync::Arc::from(edited),
    );
    upsert(&host, "/workspace/src/types.ts", edited, FileKind::NonSfc);

    let after = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("post-edit get_component_meta must resolve");
    let misses_after = meta_misses(&host);
    assert!(
        misses_after > misses_before,
        "a route-surface dependency edit MUST miss the owner's warm \
         ComponentMetaResultDb entry — the cross-file route facts flow \
         into the published signature and validates_fact_signature catches \
         the change (misses {misses_before} -> {misses_after})"
    );

    // User-visible output: the recomputed prop set carries the new
    // route-surface member.
    let after_names: Vec<String> = after.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        after_names.iter().any(|n| n == "a") && after_names.iter().any(|n| n == "b"),
        "the recomputed props MUST reflect the new `RProps` route surface \
         (`a` + `b`) — a stale warm hit would report only `a`. Got \
         {after_names:?}"
    );
}

/// Canary — cross-file `defineSlots` carrier edit.
///
/// `Comp.vue` declares `defineSlots<Slots>()` over a `Slots` interface
/// imported from a workspace `./types`. Editing the carrier `types.ts`
/// — replacing the `default` slot payload shape — must surface in the
/// owner's next `get_component_meta` slot bindings.
///
/// This is the cross-file complement of the owner-self-edit
/// `defineSlots` canary (`block_2_canary_owner_self_edit`): there the
/// slot type is script-local; here it is imported and the *carrier* is
/// edited.
///
/// Discrimination property: slot-binding synthesis walks the
/// `materialize_structure_db` / `semantic_graph` query-identity layer
/// and folds the carrier dep's parse facts into the published
/// `ComponentMetaResultEntry` signature. The owner SFC itself is
/// unchanged, so its `owner_whole_hash` result-cache key is stable and
/// the warm result is eligible for a hit — only
/// `validates_fact_signature` against the edited carrier's facts can
/// reject it. The carrier edit routes through the skip-own-drain hook,
/// so the carrier's own query-identity entries are NOT eagerly drained;
/// a substrate that keyed slot bindings under the owner content hash
/// alone — ignoring the carrier dep-signature — would serve the stale
/// `[row]` binding and the `[column, index]` assertion fails.
#[test]
fn cross_file_define_slots_carrier_edit_recomputes_slot_bindings() {
    let (workspace, host) = workspace_host(&[
        (
            "/workspace/src/types.ts",
            "export interface Slots { default(props: { row: string }): any }\n",
        ),
        (
            "/workspace/src/Comp.vue",
            "<script setup lang=\"ts\">\n\
             import type { Slots } from '/workspace/src/types'\n\
             defineSlots<Slots>()\n\
             </script>\n\
             <template><div/></template>\n",
        ),
    ]);

    let pre = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("cold get_component_meta must resolve");
    assert_eq!(
        slot_binding_names(&pre, "default"),
        vec!["row"],
        "precondition: cold `default` slot must publish exactly `row` from \
         the imported `Slots` carrier",
    );

    // Edit the carrier — replace the `default` slot payload shape. No
    // eager cascade; the owner's warm result survives the carrier edit.
    let edited = "export interface Slots \
         { default(props: { column: number; index: number }): any }\n";
    workspace.inject_file(
        "/workspace/src/types.ts".into(),
        std::sync::Arc::from(edited),
    );
    upsert(&host, "/workspace/src/types.ts", edited, FileKind::NonSfc);

    let post = host
        .get_component_meta("/workspace/src/Comp.vue")
        .expect("post-edit get_component_meta must resolve");
    assert_eq!(
        slot_binding_names(&post, "default"),
        vec!["column", "index"],
        "the recomputed `default` slot MUST publish the carrier-edited \
         bindings [column, index] — a slot-binding cache keyed under the \
         owner content hash alone (ignoring the carrier dep-signature) \
         would still publish the stale [row]. Got {:?}",
        slot_binding_names(&post, "default"),
    );
}
