//! @ai-generated — typeinfo Vue-adapter (U3a) cache / public-type / structural
//! discriminating tests.
//!
//! Companion to `vue_adapter.rs` (the macro-NORMALIZER tests). This file holds
//! the host-cache identity tests (`vue_macro_dtos` content/kind/level keying +
//! stale/poison rejection), the `.vue` PUBLIC component-type tests (resolved
//! through typeinfo WITHOUT component-meta), query-level distinctness, and the
//! spans-not-strings structural guard. Split out so neither file exceeds the
//! `no_oversize_files` architecture-guard limit.

use std::sync::Arc;

use verter_semantic::analysis::types::AnalyzedMacroKind;

use crate::typeinfo::types::{TypeInfoQueryLevel, VueMacroSurfaceRequest};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;

fn make_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert(host: &VerterHost, canonical_id: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical_id.to_string()),
        input_id: canonical_id.to_string(),
        source: Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static(canonical_id)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

fn whole_hash(host: &VerterHost, canonical_id: &str) -> verter_semantic::analysis::types::Hash16 {
    host.ensure_indexed_ready(canonical_id)
        .expect("indexed ready")
        .whole_hash
}

/// Find the index of the first macro of `kind` in the SFC.
fn macro_index_of(host: &VerterHost, canonical_id: &str, kind: AnalyzedMacroKind) -> usize {
    let indexed = host.ensure_indexed_ready(canonical_id).expect("indexed");
    indexed
        .snapshot
        .macros
        .iter()
        .position(|m| m.kind == kind)
        .unwrap_or_else(|| panic!("no {kind:?} macro in {canonical_id}"))
}

fn props_request(
    host: &VerterHost,
    canonical_id: &str,
    kind: AnalyzedMacroKind,
) -> VueMacroSurfaceRequest {
    VueMacroSurfaceRequest {
        owner_canonical: Arc::from(canonical_id),
        macro_index: macro_index_of(host, canonical_id, kind),
        macro_kind: kind,
        root_identity: whole_hash(host, canonical_id),
        level: TypeInfoQueryLevel::FullMetadata,
    }
}

const VUE_PROPS: &str = r#"<script setup lang="ts">
interface Props {
  /** the count */
  count: number;
  label?: string;
  readonly id: string;
}
defineProps<Props>();
</script>
"#;

// ---------------------------------------------------------------------------
// (8) `.vue` PUBLIC component type via public_type.rs — through typeinfo, no
//     component-meta.
//
//     Discriminating: the public surface must carry the synthesized
//     `$props` / `$emit` / `$slots` instance members built from the macros.
//     A `.ts` file (no synthesized default) must return None.
// ---------------------------------------------------------------------------

const VUE_FULL_COMPONENT: &str = r#"<script setup lang="ts">
defineProps<{ count: number }>();
defineEmits<{ (e: 'change', v: number): void }>();
defineSlots<{ default(props: { item: string }): any }>();
</script>
"#;

#[test]
fn vue_public_type_carries_synthesized_instance_members_without_component_meta() {
    const FILE: &str = "/w/FullComp.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_FULL_COMPONENT);

    // Component-meta call hook: the host records every `get_component_meta*`
    // entry in `MetaProvenance::get_component_meta_calls`. Reset it AFTER upsert
    // so we measure exactly the public-type resolution below.
    host.provenance().reset();
    let calls_before = host.provenance().snapshot().get_component_meta_calls;
    assert_eq!(calls_before, 0, "counter reset to zero before the query");

    let public_surface = host
        .resolve_vue_public_type(FILE, TypeInfoQueryLevel::PublicType)
        .expect("a .vue with type-based macros has a public component type");

    let mut members: Vec<&str> = public_surface
        .members
        .iter()
        .map(|m| m.name.as_ref())
        .collect();
    members.sort_unstable();
    assert_eq!(
        members,
        vec!["$emit", "$props", "$slots"],
        "the public component type carries the synthesized instance members"
    );

    // The PROOF: resolving the public type through typeinfo invoked
    // component-meta ZERO times. This is the architectural contract — a `.vue`
    // public type resolves through the shared typeinfo surface path, NOT
    // `get_component_meta`. A regression that routed PublicType through
    // component-meta would bump this counter.
    let calls_after = host.provenance().snapshot().get_component_meta_calls;
    assert_eq!(
        calls_after, 0,
        "resolve_vue_public_type must NOT invoke get_component_meta (typeinfo-only path)"
    );
}

#[test]
fn vue_public_type_returns_none_for_plain_ts_file() {
    const FILE: &str = "/w/plain.ts";
    let host = make_host();
    upsert(&host, FILE, "export interface Foo { a: number }\n");

    // A plain `.ts` file has no synthesized `default` instance object — no
    // public component type (negative).
    assert!(
        host.resolve_vue_public_type(FILE, TypeInfoQueryLevel::PublicType)
            .is_none(),
        "a plain .ts file has no .vue public component type"
    );
}

// ---------------------------------------------------------------------------
// (9) Query-level distinctness — PublicType vs FullMetadata produce DISTINCT
//     results for a `.vue`.
//
//     Discriminating: the PublicType surface is the instance object
//     `{ $props, $emit, $slots }`; the FullMetadata defineProps surface is the
//     props object `{ count }`. They MUST differ — a level that collapsed to
//     one result would make the member sets equal.
// ---------------------------------------------------------------------------

#[test]
fn query_level_public_vs_full_metadata_produce_distinct_surfaces() {
    const FILE: &str = "/w/LevelComp.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_FULL_COMPONENT);

    let public_surface = host
        .resolve_vue_public_type(FILE, TypeInfoQueryLevel::PublicType)
        .expect("public type resolves");
    let public_members: std::collections::BTreeSet<&str> = public_surface
        .members
        .iter()
        .map(|m| m.name.as_ref())
        .collect();

    let full_request = props_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    let full = host
        .resolve_vue_macro_surface(&full_request)
        .expect("full-metadata defineProps surface resolves");
    let full_members: std::collections::BTreeSet<&str> = full
        .surface
        .members
        .iter()
        .map(|m| m.name.as_ref())
        .collect();

    assert!(
        public_members.contains("$props"),
        "PublicType carries the instance $props member"
    );
    assert!(
        full_members.contains("count") && !full_members.contains("$props"),
        "FullMetadata defineProps surface carries the prop members, not the instance shape"
    );
    assert_ne!(
        public_members, full_members,
        "PublicType and FullMetadata are DISTINCT query results for the same .vue"
    );
}

// ---------------------------------------------------------------------------
// (10) Cache identity — the store memoizes per (canonical, content, macro,
//      level); a content edit yields a distinct content-addressed key.
//
//      Discriminating: a warm `vue_macro_dtos` call does NOT grow the store
//      (same key hits, pointer-equal Arc); a DIFFERENT macro grows it (distinct
//      slot); a content edit grows it (content-addressed key). A key that
//      omitted content (an env-hash-only key) would serve the stale entry and
//      fail the edited-props assertion.
// ---------------------------------------------------------------------------

#[test]
fn vue_macro_dtos_cache_keys_on_content_and_macro() {
    const FILE: &str = "/w/CacheComp.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_PROPS_AND_EMITS);

    assert_eq!(host.vue_surface_store().len(), 0, "store starts empty");

    let request_props = props_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    let first = host.vue_macro_dtos(&request_props);
    assert_eq!(
        first.prop_fields().len(),
        2,
        "cold compute produces the props"
    );
    assert_eq!(
        host.vue_surface_store().len(),
        1,
        "one cold entry published"
    );

    // Warm hit: same key, store does NOT grow, and the returned Arc is the SAME
    // cached value (pointer-equal).
    let second = host.vue_macro_dtos(&request_props);
    assert_eq!(
        host.vue_surface_store().len(),
        1,
        "warm hit reuses the cached entry; store does not grow"
    );
    assert!(
        Arc::ptr_eq(&first, &second),
        "warm hit returns the SAME immutable Arc"
    );

    // A DIFFERENT macro (defineEmits) is a DISTINCT cache slot.
    let request_emits = props_request(&host, FILE, AnalyzedMacroKind::DefineEmits);
    let emits_dtos = host.vue_macro_dtos(&request_emits);
    assert_eq!(
        emits_dtos.emit_fields().len(),
        1,
        "the emits DTO bundle is computed"
    );
    assert_eq!(
        host.vue_surface_store().len(),
        2,
        "a different macro occupies a distinct cache slot"
    );

    // A content edit changes the `.vue`'s whole_hash → a fresh content-addressed
    // key → a new cold entry (the old entry is not served for changed content).
    upsert(&host, FILE, VUE_PROPS_AND_EMITS_EDITED);
    let request_edited = props_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    assert_ne!(
        request_edited.root_identity, request_props.root_identity,
        "the content edit changed the .vue's whole_hash"
    );
    let edited = host.vue_macro_dtos(&request_edited);
    let mut edited_names: Vec<&str> = edited
        .prop_fields()
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    edited_names.sort_unstable();
    assert_eq!(
        edited_names,
        vec!["count", "extra", "label"],
        "the edited content's props reflect the NEW source, not the stale entry"
    );
    assert!(
        host.vue_surface_store().len() >= 3,
        "the content edit produced a distinct content-addressed cache entry"
    );
}

const VUE_PROPS_AND_EMITS: &str = r#"<script setup lang="ts">
defineProps<{ count: number; label?: string }>();
defineEmits<{ (e: 'change', v: number): void }>();
</script>
"#;

const VUE_PROPS_AND_EMITS_EDITED: &str = r#"<script setup lang="ts">
defineProps<{ count: number; label?: string; extra: boolean }>();
defineEmits<{ (e: 'change', v: number): void }>();
</script>
"#;

// ---------------------------------------------------------------------------
// (10a) Cache STALE-identity rejection — `vue_macro_dtos` must derive the
//       `whole_hash` from the LIVE `IndexedReady`, NOT trust the request's
//       `root_identity` hint. A caller holding a `root_identity` captured
//       BEFORE an edit must still get the NEW content's DTOs.
//
//       Discriminating: pre-fix `vue_macro_dtos` keys on `request.root_identity`,
//       so a request carrying the stale (pre-edit) hash hits the OLD slot and
//       returns the v1 props (missing `extra`). Post-fix it keys on the live
//       `whole_hash`, returning the v2 props.
// ---------------------------------------------------------------------------

#[test]
fn vue_macro_dtos_rejects_stale_root_identity_after_edit() {
    const FILE: &str = "/w/StaleId.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_PROPS_AND_EMITS);

    // Capture the v1 request (its `root_identity` is v1's whole_hash) and warm
    // the cache for the props macro.
    let stale_request = props_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    let v1 = host.vue_macro_dtos(&stale_request);
    let mut v1_names: Vec<&str> = v1.prop_fields().iter().map(|p| p.name.as_str()).collect();
    v1_names.sort_unstable();
    assert_eq!(
        v1_names,
        vec!["count", "label"],
        "v1 props are count + label"
    );

    // Edit the file. The live `IndexedReady.whole_hash` now differs from the
    // `stale_request.root_identity` captured above.
    upsert(&host, FILE, VUE_PROPS_AND_EMITS_EDITED);
    let live_hash = whole_hash(&host, FILE);
    assert_ne!(
        stale_request.root_identity, live_hash,
        "the edit changed the live whole_hash; the request still holds the stale one"
    );

    // Re-query with the STALE request (its `root_identity` is the pre-edit
    // hash). `vue_macro_dtos` must derive `whole_hash` from the LIVE
    // `IndexedReady` and return the v2 props — never the stale v1 entry.
    let after_edit = host.vue_macro_dtos(&stale_request);
    let mut after_names: Vec<&str> = after_edit
        .prop_fields()
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    after_names.sort_unstable();
    assert_eq!(
        after_names,
        vec!["count", "extra", "label"],
        "a stale root_identity must NOT serve the pre-edit DTOs; the live whole_hash keys the v2 slot"
    );
}

// ---------------------------------------------------------------------------
// (10b) Cache macro-KIND-mismatch rejection — `vue_macro_dtos` must derive the
//       macro kind from the snapshot's `macros[macro_index].kind`, NOT trust
//       the request's `macro_kind` hint, and the kind must be part of the
//       cache key.
//
//       Discriminating: the macro at `macro_index` is genuinely a DefineProps.
//       A request that LIES (`macro_kind: DefineEmits`) must STILL be normalized
//       as props (the derived kind wins). Pre-fix the normalizer dispatches on
//       the request's `DefineEmits` hint → runs the emits normalizer over the
//       props surface (props empty, the property-style fallback fabricates
//       events) and — because the pre-fix key omits the kind — poisons the
//       shared slot. Post-fix props are non-empty and emits empty.
// ---------------------------------------------------------------------------

#[test]
fn vue_macro_dtos_rejects_macro_kind_mismatch_without_poisoning_cache() {
    const FILE: &str = "/w/KindMismatch.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_PROPS_AND_EMITS);

    // The DefineProps macro's index — but we will LIE about its kind in the
    // request, claiming it is a DefineEmits.
    let props_index = macro_index_of(&host, FILE, AnalyzedMacroKind::DefineProps);
    let lying_request = VueMacroSurfaceRequest {
        owner_canonical: Arc::from(FILE),
        macro_index: props_index,
        macro_kind: AnalyzedMacroKind::DefineEmits, // WRONG — the macro is DefineProps.
        root_identity: whole_hash(&host, FILE),
        level: TypeInfoQueryLevel::FullMetadata,
    };

    // COLD call with the lying kind. The derived kind (DefineProps) must win:
    // the bundle carries PROPS, not the emits the property-style fallback would
    // fabricate from the props surface.
    let cold = host.vue_macro_dtos(&lying_request);
    let mut cold_props: Vec<&str> = cold.prop_fields().iter().map(|p| p.name.as_str()).collect();
    cold_props.sort_unstable();
    assert_eq!(
        cold_props,
        vec!["count", "label"],
        "the derived DefineProps kind wins; the bundle is props, not the lying-kind emits"
    );
    assert!(
        cold.emit_fields().is_empty(),
        "a props macro must not produce emits even when the request lies about the kind (negative)"
    );

    // A truthful DefineProps request at the same index keys the SAME derived
    // slot (the kind was derived identically) and returns the SAME Arc — the
    // lying call did not poison or fork the slot.
    let truthful_request = props_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    let truthful = host.vue_macro_dtos(&truthful_request);
    assert!(
        Arc::ptr_eq(&cold, &truthful),
        "the derived-kind slot is shared; the lying request did not poison a separate slot"
    );
}

/// The query level is QUERY IDENTITY, not an env-hash dimension (R21). Guard:
/// the DTO cache key carries the level tag + content hash + macro kind, NOT any
/// of the five env hashes. A structural check on the key type — if a future
/// edit folded an env hash into the key (or dropped the level), this fails to
/// compile / the field set changes.
///
/// Discriminating: destructures the EXACT key field set without `..`, so
/// adding an owned field (e.g. a `resolve_env_hash`) or removing `level_tag`
/// fails to compile. Also asserts the level tag is a 1-byte discriminant, not a
/// 16-byte env hash.
#[test]
fn vue_macro_dto_key_carries_level_and_content_not_env_hash() {
    use crate::framework::surface_store::FullKey;
    use crate::typeinfo::framework_surface::VueSurfaceKey;
    use verter_protocol::typeinfo::graph::FrameworkSurfaceKind;

    let make = |kind: FrameworkSurfaceKind,
                whole_hash: [u8; 16],
                level: TypeInfoQueryLevel,
                macro_index: usize,
                macro_kind: AnalyzedMacroKind|
     -> FullKey<VueSurfaceKey> {
        FullKey {
            kind,
            query_level: level,
            canonical: Arc::from("/w/x.vue"),
            owner_whole_hash: whole_hash,
            adapter_key: VueSurfaceKey {
                macro_index,
                macro_kind,
            },
        }
    };

    let a = make(
        FrameworkSurfaceKind::Props,
        [1u8; 16],
        TypeInfoQueryLevel::PublicType,
        0,
        AnalyzedMacroKind::DefineProps,
    );
    let b = make(
        FrameworkSurfaceKind::Props,
        [1u8; 16],
        TypeInfoQueryLevel::FullMetadata,
        0,
        AnalyzedMacroKind::DefineProps,
    );
    // Distinct level ⇒ distinct key (level is part of identity).
    assert_ne!(a, b, "the level discriminates the key");

    // Distinct content ⇒ distinct key (content-addressed).
    let c = make(
        FrameworkSurfaceKind::Props,
        [2u8; 16],
        TypeInfoQueryLevel::PublicType,
        0,
        AnalyzedMacroKind::DefineProps,
    );
    assert_ne!(a, c, "the content hash discriminates the key");

    // Distinct macro kind (and its derived surface kind) ⇒ distinct key — a
    // kind mismatch must not read / poison the sibling kind's slot.
    let d = make(
        FrameworkSurfaceKind::Emits,
        [1u8; 16],
        TypeInfoQueryLevel::PublicType,
        0,
        AnalyzedMacroKind::DefineEmits,
    );
    assert_ne!(a, d, "the macro kind discriminates the key");

    // Structural field-set guard: destructure the WHOLE neutral key AND the Vue
    // adapter remainder without `..`. Any added owned field breaks this
    // destructure (compile error), forcing a conscious decision about whether
    // the new field belongs in cache identity. An env-hash dimension folded into
    // either struct would surface here (and would not be `Copy`/discriminant).
    let FullKey {
        kind,
        query_level,
        canonical,
        owner_whole_hash,
        adapter_key: VueSurfaceKey {
            macro_index,
            macro_kind,
        },
    } = &a;
    assert_eq!(*kind, FrameworkSurfaceKind::Props);
    assert_eq!(*query_level, TypeInfoQueryLevel::PublicType);
    assert_eq!(canonical.as_ref(), "/w/x.vue");
    assert_eq!(
        *owner_whole_hash, [1u8; 16],
        "the content hash is part of the key"
    );
    assert_eq!(*macro_index, 0);
    assert_eq!(*macro_kind, AnalyzedMacroKind::DefineProps);

    // The query level is a small query-identity discriminant, NOT a 16-byte
    // env-hash; PublicType and FullMetadata differ by exactly the tag byte.
    assert_eq!(TypeInfoQueryLevel::PublicType.cache_tag(), 0);
    assert_eq!(TypeInfoQueryLevel::FullMetadata.cache_tag(), 1);
    assert_ne!(
        TypeInfoQueryLevel::PublicType.cache_tag(),
        TypeInfoQueryLevel::FullMetadata.cache_tag()
    );
}

// ---------------------------------------------------------------------------
// (11) No owned `String` type / JSDoc text on VueMacroSurface / the surface.
//
//      The surface carries SPANS only (ids + flags + interned names). This is a
//      compile-time structural guard plus a runtime assertion that the surface
//      members expose span fields, never an owned type-text String.
// ---------------------------------------------------------------------------

#[test]
fn vue_macro_surface_carries_spans_not_owned_type_strings() {
    use crate::typeinfo::surface::{
        CanonicalSpan, JsdocTagSpan, SurfaceMemberOrigin, TypeInfoIndexSignature, TypeInfoSurface,
        TypeInfoSurfaceMember, TypeInfoSurfaceSignature,
    };

    const FILE: &str = "/w/SpanComp.vue";
    let host = make_host();
    upsert(&host, FILE, VUE_PROPS);

    let request = props_request(&host, FILE, AnalyzedMacroKind::DefineProps);
    let surface = host.resolve_vue_macro_surface(&request).expect("surface");

    // The surface MUST carry members for this fixture (a stub returning an empty
    // surface would defeat the structural guard below).
    assert!(
        !surface.surface.members.is_empty(),
        "the props surface carries members (non-empty guard)"
    );

    // Whole-struct destructure WITHOUT `..` — the field set is a compile-time
    // guard. Adding ANY owned `String` type-text / JSDoc-text field to the
    // surface (the exact regression the spans-not-strings rule forbids) breaks
    // THIS destructure. Each binding's type is annotated to pin "span / id /
    // flag / interned name", never an owned `String` type body.
    let TypeInfoSurface {
        members,
        call_signatures,
        construct_signatures,
        index_signatures,
        keyspace,
        has_index_signature,
    } = &surface.surface;
    let _members: &Arc<[TypeInfoSurfaceMember]> = members;
    let _call_signatures: &Arc<[TypeInfoSurfaceSignature]> = call_signatures;
    let _construct_signatures: &Arc<[TypeInfoSurfaceSignature]> = construct_signatures;
    let _index_signatures: &Arc<[TypeInfoIndexSignature]> = index_signatures;
    let _keyspace: &Option<crate::semantic_query::SemanticNodeId> = keyspace;
    let _has_index_signature: &bool = has_index_signature;

    for member in members.iter() {
        let TypeInfoSurfaceMember {
            name,
            name_span,
            value,
            type_annotation_span,
            optional,
            readonly,
            is_method,
            visibility,
            declared_in_macro_type_arg,
            jsdoc_description_span,
            jsdoc_tag_spans,
            origin,
        } = member;
        // Interned name (not an owned type body) + node-id value (not an
        // expanded body).
        let _name: &Arc<str> = name;
        let _value: &crate::semantic_query::SemanticNodeId = value;
        // Every text-bearing field is a SPAN (`CanonicalSpan`), never a
        // `String`. The type annotations below would fail to compile if any
        // field were widened to an owned `String`.
        let _name_span: &Option<CanonicalSpan> = name_span;
        let _type_annotation_span: &Option<CanonicalSpan> = type_annotation_span;
        let _jsdoc_description_span: &Option<CanonicalSpan> = jsdoc_description_span;
        let _jsdoc_tag_spans: &Arc<[JsdocTagSpan]> = jsdoc_tag_spans;
        // Flags are `bool`.
        let _optional: &bool = optional;
        let _readonly: &bool = readonly;
        let _is_method: &bool = is_method;
        // Visibility is a small `Copy` enum (a declared-accessibility fact),
        // never an owned `String`.
        let _visibility: &verter_type_expr::MemberVisibility = visibility;
        let _declared_in_macro_type_arg: &bool = declared_in_macro_type_arg;
        // Origin carries the declaration file id + spans + merge role — no owned
        // type text.
        let SurfaceMemberOrigin {
            canonical_file,
            declaration_span,
            merge_role,
        } = origin;
        let _canonical_file: &Option<Arc<str>> = canonical_file;
        let _declaration_span: &Option<CanonicalSpan> = declaration_span;
        let _merge_role: &crate::semantic_query::MemberMergeRole = merge_role;
    }

    // The whole surface is `Eq + Hash` (a structural value of spans/ids/flags),
    // which would not hold if it carried interior-mutable / non-hashable owned
    // payloads. Exercise it.
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    surface.surface.hash(&mut h);
    let _ = h.finish();
}
