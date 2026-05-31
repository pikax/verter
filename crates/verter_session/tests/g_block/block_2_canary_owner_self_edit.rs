//! Owner-self-edit lazy-invalidation canary suite.
//!
//! The cross-file canary files (`block_2_canary_component_meta`,
//! `_compile_tier`, `_lifecycle`) edit a *dependency* of the owner SFC.
//! This file is the complementary gate for the **owner-self-edit**
//! class: the SFC under test is itself re-upserted with edited content.
//!
//! Why this class needs its own gate. Every mutation here routes
//! through the production [`harness::upsert`] helper (plain
//! `VerterHost::upsert`), which performs no own-canonical
//! query-identity cache drain. The final `ComponentMetaResultDb` slot
//! key is content-free `(owner_canonical, options)`; the owner content
//! version is carried by the per-slot candidate, so an owner edit
//! produces a new candidate discriminant and a post-edit
//! `get_component_meta` finds no candidate matching the new version —
//! it cannot warm-hit the stale *result*. But the cold recompute the
//! candidate-miss triggers walks the query-identity-keyed layer —
//! `semantic_graph`, `declaration_lookup_db`, `materialize_structure_db`,
//! the prepared DBs — whose keys are `(owner_canonical, type_name, ...)`
//! with NO owner whole-hash. Those entries physically survive an
//! owner-self edit. The ONLY mechanism that can reject a stale
//! query-identity entry for the owner canonical is lazy
//! self-version-root validation on the cold-recompute read path.
//!
//! Each test therefore DISCRIMINATES: a substrate lacking the
//! owner-canonical self-version root would serve a stale
//! `declaration_lookup_db` / `semantic_graph` / materialiser entry and
//! the recomputed user-visible output would NOT reflect the owner edit.
//! The asserted observable is always the recomputed component-meta
//! props / slot bindings / compiled output — never physical cache
//! emptiness.

#![cfg(test)]

use verter_session::{CompileProfile, FileKind};
use verter_type_expr::{PrimitiveName, TypeExpr};

#[path = "../block_2_canary/harness.rs"]
mod harness;

use harness::{compile_main, prime_compile, standalone_host, upsert};

/// The named prop's evaluated `TypeExpr` from a `get_component_meta`
/// result.
fn prop_type<'a>(
    meta: &'a verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    name: &str,
) -> &'a TypeExpr {
    &meta
        .props
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("missing prop `{name}`"))
        .type_expr
}

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

/// Canary — owner-self edit to a script-local macro prop type.
///
/// `Comp.vue` declares `interface LocalProps` in its OWN `<script
/// setup>` block and consumes it via `defineProps<LocalProps>()`.
/// Editing `LocalProps`'s member type (`value: number` → `value:
/// string`) — an owner-self edit, re-upserting `Comp.vue` itself —
/// must surface in the next `get_component_meta`.
///
/// Discrimination property: the cold resolve of `LocalProps` publishes
/// a `declaration_lookup_db` / `semantic_graph` entry keyed
/// `(/src/Comp.vue, "LocalProps")` — NO owner whole-hash in that key.
/// The own-canonical drain is skipped, so that entry physically
/// survives the owner edit; only a self-version root on its
/// `fact_dep_signature` (the keyed canonical's `FileWholeHash`) lets
/// lazy validation reject it on the cold-recompute read. A substrate
/// without that self-root serves the stale `value: number` shape and
/// the post-edit `PrimitiveName::String` assertion fails.
#[test]
fn owner_self_edit_to_local_prop_type_recomputes_component_meta() {
    let host = standalone_host();
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         interface LocalProps { value: number }\n\
         defineProps<LocalProps>()\n\
         </script>\n\
         <template><div/></template>\n",
        FileKind::VueSfc,
    );

    let pre = host
        .get_component_meta("/src/Comp.vue")
        .expect("cold get_component_meta must resolve");
    assert!(
        matches!(
            prop_type(&pre, "value"),
            TypeExpr::Primitive(PrimitiveName::Number)
        ),
        "precondition: cold `value` prop must be `number` — got {:?}",
        prop_type(&pre, "value"),
    );

    // Owner-self edit: re-upsert `Comp.vue` with `LocalProps.value`
    // retyped. The own-canonical drain is skipped.
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         interface LocalProps { value: string }\n\
         defineProps<LocalProps>()\n\
         </script>\n\
         <template><div/></template>\n",
        FileKind::VueSfc,
    );

    let post = host
        .get_component_meta("/src/Comp.vue")
        .expect("post-edit get_component_meta must resolve");
    assert!(
        !matches!(
            prop_type(&post, "value"),
            TypeExpr::Primitive(PrimitiveName::Number)
        ),
        "the recomputed `value` prop must NOT be the stale `number` type — \
         a surviving query-identity entry for (/src/Comp.vue, LocalProps) \
         without a self-version root would still report `number`. Got {:?}",
        prop_type(&post, "value"),
    );
    assert!(
        matches!(
            prop_type(&post, "value"),
            TypeExpr::Primitive(PrimitiveName::String)
        ),
        "the recomputed `value` prop MUST carry the owner-self-edited `string` \
         type — got {:?}",
        prop_type(&post, "value"),
    );
}

/// Canary — owner-self edit ADDING a script-local macro prop.
///
/// `Comp.vue` declares `interface LocalProps` locally. Editing the
/// owner SFC to ADD a sibling member (`extra: boolean`) must surface
/// the new prop in the next `get_component_meta`.
///
/// Discrimination property: same as above — the surviving query-identity
/// entry for the owner-local `LocalProps` is rejected only by its
/// self-version root. A substrate without the self-root publishes the
/// stale single-member prop set and the `extra` membership assertion
/// fails.
#[test]
fn owner_self_edit_adding_local_prop_member_recomputes_component_meta() {
    let host = standalone_host();
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         interface LocalProps { base: string }\n\
         defineProps<LocalProps>()\n\
         </script>\n\
         <template><div/></template>\n",
        FileKind::VueSfc,
    );

    let pre = host
        .get_component_meta("/src/Comp.vue")
        .expect("cold get_component_meta must resolve");
    let pre_names: Vec<String> = pre.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        pre_names.iter().any(|n| n == "base") && !pre_names.iter().any(|n| n == "extra"),
        "precondition: cold props must be exactly `base` (no `extra`) — got {pre_names:?}",
    );

    // Owner-self edit: add a sibling member to the local `LocalProps`.
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         interface LocalProps { base: string; extra: boolean }\n\
         defineProps<LocalProps>()\n\
         </script>\n\
         <template><div/></template>\n",
        FileKind::VueSfc,
    );

    let post = host
        .get_component_meta("/src/Comp.vue")
        .expect("post-edit get_component_meta must resolve");
    let post_names: Vec<String> = post.props.iter().map(|p| p.name.clone()).collect();
    assert!(
        post_names.iter().any(|n| n == "extra"),
        "the recomputed prop set MUST carry the owner-self-added member `extra` — \
         a surviving query-identity entry without a self-version root would \
         report only the stale `base`. Got {post_names:?}",
    );
    assert!(
        post_names.iter().any(|n| n == "base"),
        "the recomputed prop set must still carry `base` — got {post_names:?}",
    );
}

/// Canary — owner-self edit to a script-local `defineSlots` type.
///
/// `Comp.vue` declares `interface LocalSlots` in its own script block
/// and consumes it via `defineSlots<LocalSlots>()`. Editing the slot
/// payload shape — an owner-self edit — must surface in the recomputed
/// slot bindings.
///
/// Discrimination property: slot-binding synthesis routes through the
/// `materialize_structure_db` / `semantic_graph` query-identity layer
/// keyed by `(owner_canonical, ...)` without an owner whole-hash. The
/// own-canonical drain is skipped, so a stale materialiser / graph
/// entry survives the owner edit; only its self-version root lets lazy
/// validation reject it. A substrate without the self-root publishes
/// the stale `row` binding and the post-edit `[column, index]`
/// assertion fails.
#[test]
fn owner_self_edit_to_local_slot_type_recomputes_slot_bindings() {
    let host = standalone_host();
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         interface LocalSlots { default(props: { row: string }): any }\n\
         defineSlots<LocalSlots>()\n\
         </script>\n\
         <template><div/></template>\n",
        FileKind::VueSfc,
    );

    let pre = host
        .get_component_meta("/src/Comp.vue")
        .expect("cold get_component_meta must resolve");
    assert_eq!(
        slot_binding_names(&pre, "default"),
        vec!["row"],
        "precondition: cold `default` slot must publish exactly `row`",
    );

    // Owner-self edit: replace the slot payload shape.
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         interface LocalSlots { default(props: { column: number; index: number }): any }\n\
         defineSlots<LocalSlots>()\n\
         </script>\n\
         <template><div/></template>\n",
        FileKind::VueSfc,
    );

    let post = host
        .get_component_meta("/src/Comp.vue")
        .expect("post-edit get_component_meta must resolve");
    assert_eq!(
        slot_binding_names(&post, "default"),
        vec!["column", "index"],
        "the recomputed `default` slot MUST publish the owner-self-edited \
         bindings [column, index] — a surviving slot-binding / materialiser \
         entry without a self-version root would still publish the stale \
         [row]. Got {:?}",
        slot_binding_names(&post, "default"),
    );
}

/// Canary — owner-self edit to the SFC template recompiles the slot.
///
/// `Comp.vue`'s template text is spliced into the compiled render
/// function. Editing the owner SFC's template — an owner-self edit —
/// must invalidate the owner's own warm compile slot and the recompiled
/// assembled `Main` output must carry the new template content.
///
/// Discrimination property: the compile slot is keyed by the owner's
/// content profile; an owner edit shifts the slot's identity, so the
/// post-edit `compile_slot_is_warm` is `false` and `compile_main`
/// recompiles. The cold recompile re-runs template lowering and IDE
/// codegen, which walk the `semantic_graph` / query-identity layer
/// keyed `(owner_canonical, ...)`. With the own-canonical drain skipped
/// those entries survive; only their self-version roots let the cold
/// recompile observe the new owner content. A substrate without the
/// self-root would resurrect stale template-derived nodes and the
/// recompiled output would still render the OLD text.
#[test]
fn owner_self_edit_to_template_recompiles_compile_slot() {
    let host = standalone_host();
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nconst n = 1;\n</script>\n\
         <template><div>ALPHA</div></template>\n",
        FileKind::VueSfc,
    );

    let profile = CompileProfile::default();
    prime_compile(&host, "/src/Comp.vue");
    let before = compile_main(&host, "/src/Comp.vue").expect("pre-edit assembled module compiles");
    assert!(
        before.code.contains("ALPHA"),
        "precondition: pre-edit compiled output must carry the original \
         template text `ALPHA` — got: {}",
        before.code,
    );

    // Owner-self edit: change the SFC's own template text.
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\nconst n = 1;\n</script>\n\
         <template><section>BETA</section></template>\n",
        FileKind::VueSfc,
    );

    host.ensure_compiled("/src/Comp.vue", &profile)
        .expect("recompile after owner-self template edit");
    let after = compile_main(&host, "/src/Comp.vue")
        .expect("assembled module recompiles after the owner-self template edit");
    assert!(
        after.code.contains("BETA"),
        "the recompiled output MUST carry the NEW owner template text `BETA` — \
         a stale query-identity entry without a self-version root would still \
         render `ALPHA`. Got: {}",
        after.code,
    );
    assert!(
        !after.code.contains("ALPHA"),
        "the recompiled output must NOT carry the OLD owner template text \
         `ALPHA` — got: {}",
        after.code,
    );
}

/// Canary — owner-self edit to a script-local prop type, observed
/// through `evaluate_types`.
///
/// Identical owner-self edit to the component-meta canary above, but
/// the observable is the `evaluate_types` evaluated-type snapshot — the
/// type-evaluation surface, a distinct read path into the same
/// query-identity layer. `Comp.vue` consumes a script-local
/// `interface Shape` via `defineProps<Shape>()`; editing `Shape`'s
/// member type must surface in the recomputed evaluated props.
///
/// Discrimination property: `evaluate_types` resolves the owner-local
/// `Shape` through the same `declaration_lookup_db` / `semantic_graph`
/// query-identity entries keyed `(/src/Comp.vue, "Shape")`. The
/// own-canonical drain is skipped; a stale entry survives the owner
/// edit and is rejected only by its self-version root. A substrate
/// without the self-root yields the stale `boolean` evaluated type.
#[test]
fn owner_self_edit_to_local_prop_type_recomputes_evaluate_types() {
    let host = standalone_host();
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         interface Shape { flag: boolean }\n\
         defineProps<Shape>()\n\
         </script>\n\
         <template><div/></template>\n",
        FileKind::VueSfc,
    );

    let pre = host
        .evaluate_types("/src/Comp.vue")
        .expect("cold evaluate_types must resolve the owner");
    let pre_flag = &pre
        .props
        .iter()
        .find(|f| f.name == "flag")
        .expect("cold evaluated props must include `flag`")
        .r#type;
    assert!(
        matches!(pre_flag, TypeExpr::Primitive(PrimitiveName::Boolean)),
        "precondition: cold evaluated `flag` must be `boolean` — got {pre_flag:?}",
    );

    // Owner-self edit: retype the local `Shape.flag`.
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         interface Shape { flag: number }\n\
         defineProps<Shape>()\n\
         </script>\n\
         <template><div/></template>\n",
        FileKind::VueSfc,
    );

    let post = host
        .evaluate_types("/src/Comp.vue")
        .expect("post-edit evaluate_types must resolve the owner");
    let post_flag = &post
        .props
        .iter()
        .find(|f| f.name == "flag")
        .expect("post-edit evaluated props must include `flag`")
        .r#type;
    assert!(
        !matches!(post_flag, TypeExpr::Primitive(PrimitiveName::Boolean)),
        "the recomputed evaluated `flag` must NOT be the stale `boolean` type — \
         a surviving query-identity entry without a self-version root would \
         still report `boolean`. Got {post_flag:?}",
    );
    assert!(
        matches!(post_flag, TypeExpr::Primitive(PrimitiveName::Number)),
        "the recomputed evaluated `flag` MUST carry the owner-self-edited \
         `number` type — got {post_flag:?}",
    );
}
