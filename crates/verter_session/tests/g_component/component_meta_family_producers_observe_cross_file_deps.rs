//! Discriminator: every component-meta family producer records its
//! COMPLETE cross-file dependency set into the published
//! `ComponentMetaResultEntry` signature.
//!
//! Each producer family is exercised by a fixture whose owner SFC
//! depends on a SEPARATE dep file. After a cold `get_component_meta`,
//! the published signature (the tracer-owned `facts` rail UNION the
//! legacy whole-hash rail, surfaced via `ReadSetSignature::canonical_ids`)
//! MUST reference the dep file's canonical id. If a producer dropped a
//! cross-file read from the tracer, the dep's canonical would be
//! absent and the assertion FAILS.
//!
//! Discrimination — each assertion checks the EXACT presence of the
//! dep canonical in the signature's canonical-id set, not a bare
//! `is_some()` on the signature. A fallthrough producer that did not
//! observe recursive child-component facts into the tracer would let a
//! child-root edit slip past the parent's warm hit. The
//! `child_dep_present_for_fallthrough` case is the load-bearing
//! discriminator for fallthrough producer completeness.

#![cfg(test)]

use std::collections::HashSet;
use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

fn build_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert(host: &VerterHost, id: &str, src: &str, kind: FileLanguage) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: kind,
            aliases: Vec::new(),
        })
        .expect("upsert");
}

/// Cold `get_component_meta` on `owner`, then return the set of
/// canonical ids referenced by the published `ComponentMetaResultEntry`
/// signature (tracer-owned `facts` rail ∪ legacy rail).
fn published_signature_canonicals(host: &VerterHost, owner: &str) -> HashSet<String> {
    let meta = host.get_component_meta(owner);
    assert!(
        meta.is_some(),
        "cold get_component_meta on {owner} must resolve a component",
    );
    let sig = verter_session::for_tests::component_meta_result_signature_for_owner(host, owner)
        .unwrap_or_else(|| panic!("a ComponentMetaResultEntry must be published for {owner}"));
    sig.canonical_ids()
        .iter()
        .map(|c| c.as_ref().to_string())
        .collect()
}

#[test]
fn routed_expression_and_registry_dep_present_for_define_props() {
    // `defineProps<Foo>()` — the routed-expression projector +
    // imported registry/declaration lookup resolve `Foo` cross-file.
    let host = build_host();
    upsert(
        &host,
        "/src/props.ts",
        "export interface Foo { a: number; }\n",
        FileLanguage::script_ts(),
    );
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { Foo } from './props';\n\
         defineProps<Foo>();\n\
         </script>\n\
         <template><div /></template>\n",
        FileLanguage::vue(),
    );
    let canons = published_signature_canonicals(&host, "/src/Comp.vue");
    assert!(
        canons.contains("/src/props.ts"),
        "(routed-expression / registry): the \
         published signature MUST reference the cross-file prop type \
         dep `/src/props.ts`. signature canonicals = {canons:?}",
    );
}

#[test]
fn slot_binding_graph_carrier_dep_present_for_define_slots() {
    // `defineSlots<Slots>()` — the slot-binding-graph producer walks
    // the lowered carrier type and the macro payload cross-file.
    let host = build_host();
    upsert(
        &host,
        "/src/slots.ts",
        "export interface Slots { default(props: { row: string }): any }\n",
        FileLanguage::script_ts(),
    );
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { Slots } from './slots';\n\
         defineSlots<Slots>();\n\
         </script>\n\
         <template><div /></template>\n",
        FileLanguage::vue(),
    );
    let canons = published_signature_canonicals(&host, "/src/Comp.vue");
    assert!(
        canons.contains("/src/slots.ts"),
        "(slot-binding graph): the published \
         signature MUST reference the `defineSlots` carrier dep \
         `/src/slots.ts`. signature canonicals = {canons:?}",
    );
}

#[test]
fn materialization_and_transitive_dep_present_for_pick_over_imported_type() {
    // `defineProps<Pick<Cfg, 'a'>>()` — the materialization-structure
    // producer materialises the `Pick` projection over the imported
    // `Cfg`; the transitive dependency-props path observes the dep.
    let host = build_host();
    upsert(
        &host,
        "/src/cfg.ts",
        "export interface Cfg { a: number; b: string; c: boolean; }\n",
        FileLanguage::script_ts(),
    );
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { Cfg } from './cfg';\n\
         defineProps<Pick<Cfg, 'a'>>();\n\
         </script>\n\
         <template><div /></template>\n",
        FileLanguage::vue(),
    );
    let canons = published_signature_canonicals(&host, "/src/Comp.vue");
    assert!(
        canons.contains("/src/cfg.ts"),
        "(materialization / transitive deps): the \
         published signature MUST reference the `Pick` source dep \
         `/src/cfg.ts`. signature canonicals = {canons:?}",
    );
}

#[test]
fn owner_import_surface_barrel_dep_present_for_barrel_reexport() {
    // The owner imports through a barrel that re-exports the prop
    // type from a leaf. The owner-import-surface producer's chain
    // walk must observe BOTH the barrel and the leaf.
    let host = build_host();
    upsert(
        &host,
        "/src/leaf.ts",
        "export interface LeafProps { x: number; }\n",
        FileLanguage::script_ts(),
    );
    upsert(
        &host,
        "/src/barrel.ts",
        "export type { LeafProps } from './leaf';\n",
        FileLanguage::script_ts(),
    );
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { LeafProps } from './barrel';\n\
         defineProps<LeafProps>();\n\
         </script>\n\
         <template><div /></template>\n",
        FileLanguage::vue(),
    );
    let canons = published_signature_canonicals(&host, "/src/Comp.vue");
    assert!(
        canons.contains("/src/barrel.ts"),
        "(owner import surface): the published \
         signature MUST reference the barrel dep `/src/barrel.ts`. \
         signature canonicals = {canons:?}",
    );
    assert!(
        canons.contains("/src/leaf.ts"),
        "(owner import surface): the published \
         signature MUST reference the barrel's re-export leaf dep \
         `/src/leaf.ts` — the chain walk observes the full route. \
         signature canonicals = {canons:?}",
    );
}

#[test]
fn child_dep_present_for_fallthrough() {
    // The owner's single root is a CHILD COMPONENT. The fallthrough
    // producer recurses into the child's surface; the child's facts
    // must reach the published signature so a child-root edit
    // invalidates the parent's warm hit.
    //
    // This is the load-bearing discriminator for fallthrough producer
    // completeness: a fallthrough resolver that built a complete curated
    // `fact_versions` covering the recursive child dep but never
    // observed it into the active tracer would leave the
    // tracer-owned signature missing the child.
    let host = build_host();
    upsert(
        &host,
        "/src/Child.vue",
        "<script setup lang=\"ts\">\n\
         defineProps<{ childProp: string }>();\n\
         </script>\n\
         <template><div /></template>\n",
        FileLanguage::vue(),
    );
    upsert(
        &host,
        "/src/Parent.vue",
        "<script setup lang=\"ts\">\n\
         import Child from './Child.vue';\n\
         </script>\n\
         <template><Child /></template>\n",
        FileLanguage::vue(),
    );
    let canons = published_signature_canonicals(&host, "/src/Parent.vue");
    assert!(
        canons.contains("/src/Child.vue"),
        "(fallthrough producer completeness): the \
         published signature for the parent MUST reference the child \
         component `/src/Child.vue` — the fallthrough resolver \
         recurses into the child's inherited surface and that \
         cross-file dependency must be observed into the fact \
         tracer. signature canonicals = {canons:?}",
    );
}

#[test]
fn prepared_decl_dep_present_for_imported_alias_chain() {
    // The owner's prop type is an imported alias whose body lives in
    // a second file — prepared-declaration cross-file resolution must
    // observe the dep.
    let host = build_host();
    upsert(
        &host,
        "/src/base.ts",
        "export interface Base { id: number; }\n",
        FileLanguage::script_ts(),
    );
    upsert(
        &host,
        "/src/alias.ts",
        "import type { Base } from './base';\n\
         export type AliasProps = Base;\n",
        FileLanguage::script_ts(),
    );
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { AliasProps } from './alias';\n\
         defineProps<AliasProps>();\n\
         </script>\n\
         <template><div /></template>\n",
        FileLanguage::vue(),
    );
    let canons = published_signature_canonicals(&host, "/src/Comp.vue");
    assert!(
        canons.contains("/src/alias.ts"),
        "(prepared declarations): the published \
         signature MUST reference the imported alias dep \
         `/src/alias.ts`. signature canonicals = {canons:?}",
    );
    assert!(
        canons.contains("/src/base.ts"),
        "(prepared declarations): the published \
         signature MUST reference the transitive `Base` dep \
         `/src/base.ts` — the alias body resolves through it. \
         signature canonicals = {canons:?}",
    );
}
