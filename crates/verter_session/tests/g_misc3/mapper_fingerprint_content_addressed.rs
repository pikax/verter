//! Stage 1 discriminator tests for the content-addressed
//! [`MapperFingerprint`].
//!
//! # Why this exists
//!
//! Phase G's `MapperBinderRegistry` was introduced to give every
//! `[K in source]` mapped-type binder a STABLE
//! `(canonical, display_name, fingerprint) -> ordinal` mapping.
//! The original primitive used `Arc::as_ptr(source) as usize` as
//! the fingerprint backbone — which is stable ONLY when the SAME
//! `Arc` allocation is reused.
//!
//! `PreparedTypeDecl.body: TypeExpr` is value-cloned per bundle;
//! two structurally-identical mappers from different load paths
//! end up in DIFFERENT `Arc` allocations and therefore got
//! DIFFERENT pointer fingerprints. Empirical witness on
//! ChatMessages.vue: `mapped_binder_ordinal_collision = 258,505`
//! ≈ `mapped_type_cold = 258,608` — 99.96% of mapped-type cold
//! builds were pointer-aliased duplicates of the same logical
//! mapper.
//!
//! # What this file pins
//!
//! Codex BINDING — Stage 1: the fingerprint must be a
//! **content-addressed** structural hash over the mapper's
//! `source` / `value` / `name_type` `TypeExpr` subtrees plus the
//! `(optional, readonly)` modifiers.
//!
//! - R16 (semantic fingerprint): two structurally-equivalent
//!   mappers MUST share a fingerprint regardless of `Arc`
//!   allocation identity.
//! - R27 (stack-safe): the hash walker MUST tolerate
//!   arbitrarily deep `TypeExpr` trees without exhausting the
//!   Rust call stack.
//! - R7 (cross-owner reusable identity): the fingerprint MUST
//!   distinguish mappers whose STRUCTURE differs — different
//!   modifiers, different source, different value, different
//!   name-type — so distinct binders still get distinct
//!   ordinals.
//!
//! # Pre-fix vs post-fix expectations
//!
//! - `fingerprint_content_addressed_across_value_cloned_arcs` —
//!   PRIMARY: FAILS pre-fix (different `Arc::as_ptr` →
//!   different fingerprints → different ordinals); PASSES
//!   post-fix.
//! - `chatmessages_like_mapped_binder_ordinal_collision_is_zero`
//!   — correctness counter: FAILS pre-fix (collision counter
//!   greater than 10× the post-fix value on a
//!   `MessageBase` / `Pick<PropsBase>` / `[K in keyof X]?:`
//!   ChatMessages-shape fixture); PASSES post-fix.
//! - `fingerprint_distinguishes_structurally_distinct_mappers` —
//!   counterfixture (PASSES pre- AND post-fix): different
//!   modifiers + different source must always produce different
//!   fingerprints.
//! - `fingerprint_handles_deeply_nested_value_without_stack_overflow`
//!   — stack-safety regression: post-fix only; PASSES on the
//!   iterative-worklist walker, would FAIL on a recursive
//!   `Hash::hash(&type_expr)` walker over a 10K-deep
//!   `Array<Array<...>>` value.

#![allow(clippy::too_many_lines)]

use std::sync::Arc;

// The shared harness module re-exports / declares constants that
// other test files in this dir consume. This test only needs
// `build_hermetic_host`; suppress dead-code warnings for the rest
// of the shared surface to keep the file's signal clean.
#[allow(dead_code, unused_imports)]
#[path = "../component_meta_audit/harness.rs"]
mod harness;

use verter_session::audited_request::AuditedRequest;
use verter_session::test_only::mapper_fingerprint::MapperFingerprintProbe;
use verter_type_expr::{
    MappedModifier, ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr,
};

// ---------------------------------------------------------------------------
// Test 1 (PRIMARY) — content-addressed identity across value-cloned bundles
// ---------------------------------------------------------------------------

/// Two FRESH `Arc<TypeExpr>` trees with identical mapper
/// content must produce the SAME fingerprint AND map to the
/// SAME ordinal in the registry. This is the load-bearing
/// invariant: `PreparedTypeDecl.body: TypeExpr` is value-cloned
/// per bundle, so the post-Stage-1 fingerprint MUST recognise
/// the two clones as one logical mapper.
///
/// **Pre-fix behaviour** (pointer-identity primitive): FAILS —
/// `Arc::as_ptr(source_a)` ≠ `Arc::as_ptr(source_b)`, so the
/// fingerprints differ and the registry hands out two different
/// ordinals.
///
/// **Post-fix behaviour** (content-addressed primitive): PASSES.
#[test]
fn fingerprint_content_addressed_across_value_cloned_arcs() {
    // Build two STRUCTURALLY-equivalent mappers that mimic
    // `[K in keyof Foo]?: Foo[K]` — common in the
    // `Partial<Pick<...>>` pattern that drives the
    // ChatMessages.vue cold path.
    //
    // Each side allocates FRESH `Arc<TypeExpr>` instances so
    // pointer identity is GUARANTEED distinct.
    let source_a = make_keyof_foo();
    let value_a = make_indexed_access_foo_k();
    let source_b = make_keyof_foo();
    let value_b = make_indexed_access_foo_k();

    // Sanity check: the two sides genuinely have distinct Arc
    // allocations. If this assertion ever fires, the test's
    // premise is broken and the result is meaningless.
    assert_ne!(
        Arc::as_ptr(&source_a) as usize,
        Arc::as_ptr(&source_b) as usize,
        "test premise: the two source Arcs must NOT share an allocation",
    );
    assert_ne!(
        Arc::as_ptr(&value_a) as usize,
        Arc::as_ptr(&value_b) as usize,
        "test premise: the two value Arcs must NOT share an allocation",
    );

    let fp_a = MapperFingerprintProbe::from_components(
        &source_a,
        &value_a,
        MappedModifier::Add, // `?:`
        MappedModifier::None,
        None,
    );
    let fp_b = MapperFingerprintProbe::from_components(
        &source_b,
        &value_b,
        MappedModifier::Add,
        MappedModifier::None,
        None,
    );

    assert_eq!(
        fp_a, fp_b,
        "two value-cloned `Arc<TypeExpr>` trees with identical \
         structural content must share a fingerprint — the \
         content-addressed primitive is the load-bearing \
         invariant that lets `MapperBinderRegistry` recognise \
         them as ONE logical mapper",
    );

    // Drive the registry — the SAME ordinal must come back for
    // both calls.
    let registry = MapperFingerprintProbe::fresh_registry();
    let canonical: Arc<str> = Arc::from("/file.ts");
    let display: Arc<str> = Arc::from("K");
    let ord_a = MapperFingerprintProbe::ordinal_for(&registry, &canonical, &display, fp_a);
    let ord_b = MapperFingerprintProbe::ordinal_for(&registry, &canonical, &display, fp_b);
    assert_eq!(
        ord_a, ord_b,
        "structurally-equivalent mappers must collide on the \
         same `MapperBinderRegistry` ordinal — distinct ordinals \
         here are the root cause of the 258K-fold mapped_type_cold \
         cross product",
    );
}

// ---------------------------------------------------------------------------
// Test 2 — fingerprint discriminates structurally-distinct mappers
// (counterfixture: PASSES pre-fix AND post-fix)
// ---------------------------------------------------------------------------

/// Different modifiers, different source, different value, or
/// different name-type each independently force the fingerprint
/// to differ. This prevents the content-addressed primitive
/// from over-collapsing (treating "all mapped types" as one
/// fingerprint) and preserves the original "distinct binders →
/// distinct ordinals" property.
#[test]
fn fingerprint_distinguishes_structurally_distinct_mappers() {
    let source = Arc::new(TypeExpr::Primitive(PrimitiveName::String));
    let value = Arc::new(TypeExpr::Primitive(PrimitiveName::Number));

    let base = MapperFingerprintProbe::from_components(
        &source,
        &value,
        MappedModifier::None,
        MappedModifier::None,
        None,
    );

    // Different `optional` modifier (`[K in X]: V` vs `[K in X]?: V`).
    let optional_add = MapperFingerprintProbe::from_components(
        &source,
        &value,
        MappedModifier::Add,
        MappedModifier::None,
        None,
    );
    assert_ne!(
        base, optional_add,
        "different `optional` modifiers must yield distinct fingerprints",
    );

    // Different `optional` modifier (Remove: `-?`).
    let optional_remove = MapperFingerprintProbe::from_components(
        &source,
        &value,
        MappedModifier::Remove,
        MappedModifier::None,
        None,
    );
    assert_ne!(
        base, optional_remove,
        "`-?` must distinguish from no-modifier",
    );
    assert_ne!(
        optional_add, optional_remove,
        "`?` and `-?` are different operations — fingerprints must differ",
    );

    // Different `readonly` modifier.
    let readonly_add = MapperFingerprintProbe::from_components(
        &source,
        &value,
        MappedModifier::None,
        MappedModifier::Add,
        None,
    );
    assert_ne!(
        base, readonly_add,
        "different `readonly` modifiers must yield distinct fingerprints",
    );

    // Different source shape (`keyof X` vs `keyof Y`).
    let source_x = Arc::new(TypeExpr::Ref {
        name: Arc::from("X"),
        type_arguments: Arc::from([] as [TypeExpr; 0]),
    });
    let source_y = Arc::new(TypeExpr::Ref {
        name: Arc::from("Y"),
        type_arguments: Arc::from([] as [TypeExpr; 0]),
    });
    let fp_x = MapperFingerprintProbe::from_components(
        &source_x,
        &value,
        MappedModifier::None,
        MappedModifier::None,
        None,
    );
    let fp_y = MapperFingerprintProbe::from_components(
        &source_y,
        &value,
        MappedModifier::None,
        MappedModifier::None,
        None,
    );
    assert_ne!(
        fp_x, fp_y,
        "different source `Ref` names must yield distinct fingerprints",
    );

    // Different value shape (`V1` vs `V2`).
    let value_v1 = Arc::new(TypeExpr::Ref {
        name: Arc::from("V1"),
        type_arguments: Arc::from([] as [TypeExpr; 0]),
    });
    let value_v2 = Arc::new(TypeExpr::Ref {
        name: Arc::from("V2"),
        type_arguments: Arc::from([] as [TypeExpr; 0]),
    });
    let fp_v1 = MapperFingerprintProbe::from_components(
        &source,
        &value_v1,
        MappedModifier::None,
        MappedModifier::None,
        None,
    );
    let fp_v2 = MapperFingerprintProbe::from_components(
        &source,
        &value_v2,
        MappedModifier::None,
        MappedModifier::None,
        None,
    );
    assert_ne!(
        fp_v1, fp_v2,
        "different value `Ref` names must yield distinct fingerprints",
    );

    // Different name-type (`as N` rename).
    let name_type = Arc::new(TypeExpr::Primitive(PrimitiveName::String));
    let with_name = MapperFingerprintProbe::from_components(
        &source,
        &value,
        MappedModifier::None,
        MappedModifier::None,
        Some(&name_type),
    );
    assert_ne!(
        base, with_name,
        "presence of a `name_type` rename must change the fingerprint",
    );
}

// ---------------------------------------------------------------------------
// Test 3 — stack-safety: deeply-nested `value` does NOT overflow
// ---------------------------------------------------------------------------

/// Build a mapper whose `value` is 10,000 levels of
/// `Array<Array<Array<...>>>` and confirm that
/// `MapperFingerprint::from_components` returns without
/// stack-overflowing. The iterative-worklist walker tolerates
/// arbitrary depths; a naïve recursive `Hash::hash(&type_expr)`
/// would blow the Rust call stack on a depth this large.
///
/// This pins R27 (stack-safe). The test runs under
/// `RUST_MIN_STACK=134217728` like the rest of the suite, but
/// the iterative walker should pass even on the default 2 MiB
/// thread stack.
#[test]
fn fingerprint_handles_deeply_nested_value_without_stack_overflow() {
    const DEPTH: usize = 10_000;

    // Build `Array<Array<Array<...<string>>>` to DEPTH layers
    // bottom-up so we never recurse on construction either.
    let mut current = Arc::new(TypeExpr::Primitive(PrimitiveName::String));
    for _ in 0..DEPTH {
        current = Arc::new(TypeExpr::Array {
            element: current,
            readonly: false,
        });
    }

    let source = Arc::new(TypeExpr::Primitive(PrimitiveName::String));
    let value_deep = current;

    // The call MUST NOT panic / stack-overflow / hang. If the
    // walker were recursive (as the derived `Hash` on `TypeExpr`
    // is), this would blow the default thread stack long before
    // returning.
    let fp = MapperFingerprintProbe::from_components(
        &source,
        &value_deep,
        MappedModifier::None,
        MappedModifier::None,
        None,
    );

    // A trivial validity gate: the result is a usable 64-bit
    // fingerprint. (We can't predict the exact hash value
    // without re-implementing the walker.)
    let raw = MapperFingerprintProbe::raw(fp);
    // Smoke: a deep tree must produce a non-default hash —
    // FxHasher with this much input is virtually never zero.
    assert_ne!(raw, 0, "deep tree must produce a non-zero hash");
}

// ---------------------------------------------------------------------------
// Test 4 — correctness-verifying counter: ChatMessages-like fixture
// ---------------------------------------------------------------------------

/// Drive a `getComponentMeta` resolve on a hermetic fixture
/// that mimics the ChatMessages.vue pattern (a defineSlots
/// whose slot type is `Mapped { source: keyof Tool<I, O>,
/// value: ... }` over an imported generic interface). Read the
/// audit's `mapped_binder_ordinal_collision` counter — under
/// the content-addressed fingerprint this should be ZERO
/// (every lowering of the SAME mapper gets the SAME ordinal,
/// so there's no "different ordinal for same `(canonical,
/// display_name)`" collision to count).
///
/// **Pre-fix behaviour** (pointer-identity primitive): the
/// counter is non-zero — each value-cloned bundle minted a
/// distinct ordinal and tripped the collision detector.
///
/// **Post-fix behaviour** (content-addressed primitive): the
/// counter is zero on this fixture.
///
/// The fixture is hermetic — built from `MemoryWorkspace` — so
/// it does NOT depend on the nuxt-ui corpus and runs in the
/// default `cargo test --workspace --tests` invocation.
const TOOL_TS: &str = r#"
export interface UIMessage {
  role: 'user' | 'assistant';
  content: string;
}

export interface Tool<INPUT = unknown, OUTPUT = unknown> {
  outputSchema: OUTPUT;
  execute: (input: INPUT) => OUTPUT;
}

export interface PropsBase<T extends UIMessage[]> {
  messages: T;
  tools: Tool[];
  assistant: string;
}

export interface MessageBase<T extends UIMessage[]> {
  message: T[number];
  tool: Tool;
}
"#;

const CHAT_MESSAGES_VUE: &str = r#"<script setup lang="ts" generic="T extends UIMessage[]">
import type { Tool, UIMessage, PropsBase, MessageBase } from './tool';

const props = defineProps<Partial<Pick<PropsBase<T>, 'messages' | 'tools' | 'assistant'>>>();

defineSlots<{
  [K in keyof MessageBase<T>]?: (props: { slot: MessageBase<T>[K] }) => unknown;
}>();
</script>
<template><div></div></template>
"#;

#[test]
fn chatmessages_like_mapped_binder_ordinal_collision_is_zero() {
    let host = harness::build_hermetic_host(&[
        ("/tool.ts", TOOL_TS),
        ("/ChatMessages.vue", CHAT_MESSAGES_VUE),
    ]);

    let (_analysis, _resolved, audit) = AuditedRequest::builder()
        .attach_to(host)
        .resolve_component_meta("/ChatMessages.vue")
        .expect("hermetic resolve must succeed");

    let footprint = audit
        .footprint
        .as_ref()
        .expect("footprint_capture is enabled in the harness");

    // Codex BINDING — Stage 1 prediction: with the
    // content-addressed fingerprint, no two lowerings of the
    // SAME mapper land on different ordinals, so the collision
    // counter is zero on this fixture.
    //
    // Pre-fix: the value-cloned `PreparedTypeDecl.body` bundles
    // for `MessageBase<T>` / `Pick<PropsBase<T>, '...'>` /
    // `[K in keyof MessageBase<T>]?: ...` each minted a
    // distinct pointer fingerprint → distinct ordinals on the
    // SAME `(canonical, "K")` slot → collision counter > 0.
    assert_eq!(
        footprint.resolver_hot_path.mapped_binder_ordinal_collision, 0,
        "content-addressed `MapperFingerprint` must yield zero \
         mapped-binder ordinal collisions on a ChatMessages-shape \
         fixture; observed = {}",
        footprint.resolver_hot_path.mapped_binder_ordinal_collision,
    );
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// `keyof Foo` — the most common mapped-type source shape.
fn make_keyof_foo() -> Arc<TypeExpr> {
    Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::Ref {
        name: Arc::from("Foo"),
        type_arguments: Arc::from([] as [TypeExpr; 0]),
    })))
}

/// `Foo[K]` — the most common mapped-type value shape (paired
/// with `keyof Foo` as source).
fn make_indexed_access_foo_k() -> Arc<TypeExpr> {
    Arc::new(TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::Ref {
            name: Arc::from("Foo"),
            type_arguments: Arc::from([] as [TypeExpr; 0]),
        }),
        index: Arc::new(TypeExpr::Ref {
            name: Arc::from("K"),
            type_arguments: Arc::from([] as [TypeExpr; 0]),
        }),
    })
}

/// (unused by current tests, kept for future Object-shape mapper
/// fixtures so we don't have to re-derive the helper)
#[allow(dead_code)]
fn make_object_with_two_props() -> Arc<TypeExpr> {
    Arc::new(TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![
            ObjectMember::Property(ObjectProperty::synthetic(
                "a".into(),
                TypeExpr::Primitive(PrimitiveName::String),
                false,
                false,
            )),
            ObjectMember::Property(ObjectProperty::synthetic(
                "b".into(),
                TypeExpr::Primitive(PrimitiveName::Number),
                false,
                false,
            )),
        ],
    })))
}
