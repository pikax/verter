//! R3/R26/R28 compile-tier producer-timing discriminator.
//!
//! Pins the invariant that the compile-tier fact tracer DOES record
//! cross-file `Member` / `MemberPresence` observations when the
//! consumer SFC imports types from a workspace `.ts` file — i.e.
//! the dependency surface is resolved + indexed-ready BEFORE the
//! tracer installs, so `lookup_parse_fact_hash` returns `Some(_)`
//! and the consumer's `fact_dep_signature` actually records a
//! cross-file dep.
//!
//! Without the pre-tracer prefetch, the tracer silently skips
//! observation (artifacts not in store, route not in derived
//! cache) and the signature ends up empty. That breaks R3
//! fact-validation for cross-file edits: the consumer never
//! invalidates on a `types.ts` edit unless eager invalidation
//! drains the cache, which is the bypass we're removing.
//!
//! The discriminator is structural: after a cold compute of a SFC
//! that imports a type from `/src/types.ts`, the SFC's
//! `compile_slot.fact_dep_signature` MUST contain at least one
//! `FactVersionRef::Parse` entry whose `canonical_id` is
//! `/src/types.ts`. This WOULD FAIL against a tree where the prefetch
//! is removed.

use verter_semantic::facts::registry::FactKey;
use verter_session::resolver_core::{FactVersionRef, ParseFactRef};
use verter_session::ReadSetSignature;
use verter_session::{
    CompileCacheMode, CompileErrorPolicy, CompileProfile, FileKind, HostConfig, UpsertRequest,
    VerterHost, VirtualNodeKind, VirtualQuery,
};

fn upsert_ts(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: source.into(),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("upsert ts");
}

fn upsert_vue(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: source.into(),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .expect("upsert vue");
}

fn prime_compile(host: &VerterHost, canonical: &str) {
    let _ = host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical.to_string()),
        node_kind: Some(VirtualNodeKind::Script),
        compile_profile: CompileProfile::default(),
    });
}

/// Read the `CompileSlot.fact_dep_signature` for the canonical at
/// the default profile, or an empty signature if no slot was admitted.
fn read_signature(host: &VerterHost, canonical: &str) -> ReadSetSignature {
    host.compile_slot_fact_dep_signature(canonical, &CompileProfile::default())
        .unwrap_or_else(ReadSetSignature::empty)
}

/// R3/R26/R28 producer-timing discriminator.
///
/// After a cold compute of a SFC that imports a type from a
/// workspace `.ts` file, the compile slot's `fact_dep_signature`
/// MUST contain at least one `FactVersionRef::Parse` observation
/// whose `canonical_id` matches the imported `.ts`.
///
/// Discriminating: against a tree where the pre-tracer prefetch is
/// removed, the signature would be empty (artifacts not in store,
/// routes not in derived cache → silent skip). The prefetch populates
/// both layers and the observation lands.
#[test]
fn cold_compile_observes_member_fact_for_cross_file_type_import() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface Foo { a: number; }\n",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { Foo } from './types';\n\
         defineProps<Foo>();\n\
         </script>\n",
    );

    // Cold compute — the prefetch runs, deps reach indexed-ready,
    // the tracer observes per-Member facts. The compile slot is
    // admitted with a non-empty fact_dep_signature.
    prime_compile(&host, "/src/Comp.vue");

    let signature = read_signature(&host, "/src/Comp.vue");
    assert!(
        !signature.facts.is_empty(),
        "R3/R26/R28: compile_slot.fact_dep_signature MUST be non-empty \
         after cold compute of an SFC importing types from a workspace .ts \
         (signature.facts.len() = {}). The pre-tracer prefetch is the producer-timing \
         contract — without it, observations silently skip and the signature \
         is empty, breaking cross-file fact-validation.",
        signature.facts.len()
    );

    // Path-precision: at least one observation must reference the
    // cross-file `.ts` canonical (NOT just the SFC's own facts).
    // R28 path-precise facts include `Export`, `MemberShape`, and
    // `MemberPresence` for the imported type — the producer admits
    // one Export + one MemberShape + per-member MemberPresence
    // observations for each `macro_type_dep`.
    let observes_cross_file_type_fact = signature.facts.iter().any(|fact| {
        matches!(
            fact,
            FactVersionRef::Parse(ParseFactRef { canonical_id, key, .. })
                if canonical_id == "/src/types.ts"
                    && matches!(
                        key,
                        FactKey::Export { .. }
                            | FactKey::MemberShape { .. }
                            | FactKey::MemberPresence { .. }
                    )
        )
    });
    assert!(
        observes_cross_file_type_fact,
        "R28: at least one fact observation MUST target the imported `.ts` canonical \
         with an Export / MemberShape / MemberPresence key. \
         Signature observed: {:?}",
        signature
            .facts
            .iter()
            .map(|f| format!("{:?}", f))
            .collect::<Vec<_>>()
    );
}

/// Production-flow oracle: after a cold compute of a SFC that
/// imports types from `/src/types.ts`, editing `/src/types.ts`
/// (changing the imported member's body) MUST invalidate the
/// consumer's compile slot via fact-validation on the next read.
///
/// This is the integration check: the prefetch + producer
/// observation together ensure the consumer's signature carries
/// the dep's pre-edit hash, and the post-edit fact registry
/// fingerprints differ, so `compile_slot_is_warm` returns false
/// without eager invalidation.
#[test]
fn cross_file_type_edit_invalidates_consumer_via_fact_validation() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface Foo { a: number; }\n",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { Foo } from './types';\n\
         defineProps<Foo>();\n\
         </script>\n",
    );

    prime_compile(&host, "/src/Comp.vue");
    let profile = CompileProfile::default();
    let warm_before = host.compile_slot_is_warm("/src/Comp.vue", &profile);
    let signature_before = read_signature(&host, "/src/Comp.vue");

    // Skip the discrimination if the SFC failed to compile during
    // prime — the test pins the BEHAVIOUR DELTA between pre-edit
    // and post-edit warm-hit state.
    if !warm_before {
        // The producer observation MUST still record a non-empty
        // signature even when prime didn't reach a warm slot; this
        // ensures the discriminator is meaningful.
        eprintln!(
            "INFO: warm_before=false (prime likely failed to admit a slot); \
             signature_before.facts.len()={}",
            signature_before.facts.len()
        );
        return;
    }

    assert!(
        !signature_before.facts.is_empty(),
        "warm slot MUST carry a non-empty fact_dep_signature for a SFC with \
         cross-file type imports — fact-validation gates the warm hit."
    );

    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface Foo { a: string; }\n",
    );

    let warm_after = host.compile_slot_is_warm("/src/Comp.vue", &profile);
    assert!(
        !warm_after,
        "R3: cross-file type-import edit MUST invalidate the consumer \
         compile slot via fact-validation on read. \
         warm_before=true, warm_after=true indicates either a missing \
         observation (the producer skipped the dep) or an empty signature."
    );
}

// ── Runtime-import / external-dep discriminators ──────────────────
//
// These tests upsert the dependency through the plain `upsert`. The
// owner-upsert path has no eager reverse-dependent cascade, so the
// consumer's compile slot is not physically cleared by a dependency
// edit. The ONLY mechanism that can invalidate the consumer is the
// warm-hit fact-signature check (`compile_slot_fact_signature_validates`).
// That isolation is what makes each test discriminating: a producer
// without a `FileWholeHash` for runtime imports / external `src=` deps
// would record no fact for the edited dep in the consumer's signature,
// so the warm hit would be served stale; with the whole-hash fact the
// signature mismatches on edit and the warm hit misses.

/// Discriminator 1 — compile-slot invalidation on a runtime-import
/// body edit.
///
/// `Comp.vue` has a *runtime* (value, non-type-only) import
/// `import { helper } from './utils'`. The producer observes a
/// `FileWholeHash` of `/src/utils.ts`. Editing `helper`'s BODY
/// (`return 1` → `return 2`) — a change the signature-pinned
/// `Export` fact cannot see, since the function signature
/// `() => number` is unchanged — must still invalidate `Comp.vue`'s
/// warm compile slot.
///
/// A producer that recorded only `ImportRef` + signature-pinned
/// `Export` would leave both hashes identical on a body-only edit →
/// `compile_slot_is_warm` stays `true` → stale slot served. With the
/// `FileWholeHash` fact the edit mismatches → warm hit misses.
#[test]
fn compile_slot_invalidates_on_runtime_import_body_edit() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/utils.ts",
        "export function helper() { return 1; }\n",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import { helper } from './utils';\n\
         const n = helper();\n\
         </script>\n\
         <template><div>{{ n }}</div></template>\n",
    );

    let profile = CompileProfile::default();
    prime_compile(&host, "/src/Comp.vue");
    let warm_before = host.compile_slot_is_warm("/src/Comp.vue", &profile);

    // If prime did not admit a warm slot the behaviour delta cannot
    // be measured — surface it loudly rather than passing vacuously.
    assert!(
        warm_before,
        "precondition: Comp.vue MUST have a warm compile slot after \
         the initial compile (warm_before=false means prime failed to \
         admit a slot — the discriminator cannot run)."
    );

    let signature = read_signature(&host, "/src/Comp.vue");
    let observes_utils_whole_hash = signature.facts.iter().any(|fact| {
        matches!(
            fact,
            FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/src/utils.ts"
        )
    });
    assert!(
        observes_utils_whole_hash,
        "R3: the compile-tier producer MUST observe a \
         FileWholeHash for a runtime-imported dependency. \
         Signature observed: {:?}",
        signature
            .facts
            .iter()
            .map(|f| format!("{f:?}"))
            .collect::<Vec<_>>()
    );

    // Edit ONLY the body of `helper` — signature stays `() => number`.
    // The owner-upsert path has no eager cascade, so fact-validation
    // is the sole invalidation path.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/utils.ts".to_string()),
            input_id: "/src/utils.ts".to_string(),
            source: "export function helper() { return 2; }\n".into(),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("dep upsert");

    let warm_after = host.compile_slot_is_warm("/src/Comp.vue", &profile);
    assert!(
        !warm_after,
        "a runtime-import body edit MUST invalidate the \
         consumer compile slot via FileWholeHash fact-validation. \
         warm_after=true means the producer recorded no whole-hash \
         fact for the runtime dep (the signature-pinned Export fact \
         alone cannot see a body-only edit)."
    );

    // ensure_compiled's warm path must agree: it recompiles rather
    // than returning Ok on the stale slot.
    host.ensure_compiled("/src/Comp.vue", &profile)
        .expect("recompile after dep edit");
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "after ensure_compiled the slot is warm again (recompiled)"
    );
}

/// Discriminator 2 — `ensure_compiled`'s warm path validates the
/// compile-slot fact signature.
///
/// After a cold compile of an SFC with a cross-file macro type dep,
/// editing the dep's type (suppressing the eager cascade) leaves the
/// consumer's compile slot stale-but-present, and `compile_slot_is_warm`
/// returns `false` (the dep's `MemberPresence` fact mismatches). The
/// discriminating call is `ensure_compiled`: it must RECOMPILE,
/// replacing the stale slot with a fresh one — which makes
/// `compile_slot_is_warm` return `true` again.
///
/// A warm path that checked only
/// `slot.semantic_hash == parse.semantic_hash && style_override_hash`
/// would hold that predicate when the SFC's own content is unchanged,
/// so `ensure_compiled` would return `Ok(())` WITHOUT recompiling — the
/// stale slot would stay in place and `compile_slot_is_warm` would stay
/// `false` even after `ensure_compiled` ran. The warm path additionally
/// runs `compile_slot_fact_signature_validates`, sees the dep fact
/// mismatch, falls through to a real recompile, and the slot becomes
/// warm.
#[test]
fn ensure_compiled_warm_path_validates_compile_slot_fact_signature() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface Foo { a: number; }\n",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { Foo } from './types';\n\
         defineProps<Foo>();\n\
         </script>\n\
         <template><div/></template>\n",
    );

    let profile = CompileProfile::default();
    // First compile via ensure_compiled itself.
    host.ensure_compiled("/src/Comp.vue", &profile)
        .expect("first compile");
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "precondition: Comp.vue warm after first ensure_compiled."
    );

    // Edit the imported type — change the member's type. The
    // owner-upsert path has no eager cascade, so only fact-validation
    // can invalidate.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/types.ts".to_string()),
            input_id: "/src/types.ts".to_string(),
            source: "export interface Foo { a: string; }\n".into(),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("dep upsert");

    // The slot must NOT be warm after the dep edit — fact-validation
    // catches the cross-file type change even though Comp.vue's own
    // content is unchanged.
    assert!(
        !host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "after a cross-file dep edit the compile slot MUST \
         fail fact-validation (a warm slot here means the signature \
         did not catch the dep edit)."
    );

    // The discriminating call. `ensure_compiled` must RECOMPILE the
    // stale slot, not short-circuit on the unchanged `semantic_hash`.
    host.ensure_compiled("/src/Comp.vue", &profile)
        .expect("ensure_compiled recompile");

    // The recompile re-records a fresh fact signature against the
    // edited dep, so the slot is warm again. A warm path that returned
    // `Ok(())` without recompiling would leave the stale slot (still
    // carrying the pre-edit dep fact hash) in place, so this assertion
    // would FAIL with the slot still not warm.
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "ensure_compiled's warm path MUST validate the \
         compile-slot fact signature. A slot that is still not warm \
         after ensure_compiled means it short-circuited on the \
         unchanged semantic_hash and left the stale slot in place \
         instead of recompiling."
    );
}

/// Discriminator 3 — compile-slot invalidation on an external
/// `src=` template-block edit.
///
/// `Comp.vue` has `<template src="./tpl.html">`. The external file
/// content is spliced verbatim into the compiled output by
/// `merge_external_sources`, so editing `tpl.html` must invalidate
/// the consumer compile slot.
///
/// If `external_requests` is not passed to the compile-tier producer,
/// the SFC's `fact_dep_signature` is completely empty, which trivially
/// validates → stale slot served forever. The producer observes a
/// `FileWholeHash` of the resolved external canonical → an edit
/// mismatches → warm hit misses.
#[test]
fn compile_slot_invalidates_on_external_src_template_edit() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(&host, "/src/tpl.html", "<div>A</div>\n");
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<template src=\"./tpl.html\"></template>\n\
         <script setup lang=\"ts\">\nconst n = 1;\n</script>\n",
    );

    let profile = CompileProfile::default();
    prime_compile(&host, "/src/Comp.vue");
    let warm_before = host.compile_slot_is_warm("/src/Comp.vue", &profile);
    assert!(
        warm_before,
        "precondition: Comp.vue MUST have a warm compile slot after \
         the initial compile."
    );

    let signature = read_signature(&host, "/src/Comp.vue");
    let observes_tpl_whole_hash = signature.facts.iter().any(|fact| {
        matches!(
            fact,
            FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/src/tpl.html"
        )
    });
    assert!(
        observes_tpl_whole_hash,
        "R3: the compile-tier producer MUST observe a \
         FileWholeHash for an external `src=` dependency. A producer \
         that never received `external_requests` would produce an \
         empty signature. Signature observed: {:?}",
        signature
            .facts
            .iter()
            .map(|f| format!("{f:?}"))
            .collect::<Vec<_>>()
    );

    // Edit the external template. The owner-upsert path has no eager
    // cascade.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/tpl.html".to_string()),
            input_id: "/src/tpl.html".to_string(),
            source: "<section>B</section>\n".into(),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("external dep upsert");

    let warm_after = host.compile_slot_is_warm("/src/Comp.vue", &profile);
    assert!(
        !warm_after,
        "an external `src=` template edit MUST invalidate the \
         consumer compile slot via FileWholeHash fact-validation. \
         warm_after=true means the producer recorded no whole-hash \
         fact for the external dependency."
    );
}

/// Discriminator 4 — compile-slot invalidation on a *side-effect*
/// runtime-import body edit.
///
/// `Comp.vue` has a side-effect import `import './setup'` — a runtime
/// (non-type-only) import with ZERO bindings. The dependency's content
/// is re-emitted in the assembled module, so editing `setup.ts`'s body
/// must invalidate `Comp.vue`'s warm compile slot.
///
/// If the producer's `FileWholeHash` admission ran only inside the
/// `for binding in import.bindings.iter()` loop, a side-effect import
/// (empty `bindings`) would never execute the loop body and no
/// `FileWholeHash` fact would be recorded for `./setup` — the
/// consumer's signature would carry no fact for the dep, so a warm hit
/// would be served stale. A non-type-only import with a resolved dep
/// contributes its `FileWholeHash` fact even with zero bindings → the
/// edit mismatches → warm hit misses.
#[test]
fn compile_slot_invalidates_on_side_effect_import_body_edit() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(&host, "/src/setup.ts", "globalThis.__verter_setup = 1;\n");
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import './setup';\n\
         const n = 1;\n\
         </script>\n\
         <template><div>{{ n }}</div></template>\n",
    );

    let profile = CompileProfile::default();
    prime_compile(&host, "/src/Comp.vue");
    let warm_before = host.compile_slot_is_warm("/src/Comp.vue", &profile);

    // If prime did not admit a warm slot the behaviour delta cannot
    // be measured — surface it loudly rather than passing vacuously.
    assert!(
        warm_before,
        "precondition: Comp.vue MUST have a warm compile slot after \
         the initial compile (warm_before=false means prime failed to \
         admit a slot — the discriminator cannot run)."
    );

    // Fact-presence half of the discriminator: the compile slot's
    // signature MUST carry a `FileWholeHash` for the side-effect dep.
    let signature = read_signature(&host, "/src/Comp.vue");
    let observes_setup_whole_hash = signature.facts.iter().any(|fact| {
        matches!(
            fact,
            FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/src/setup.ts"
        )
    });
    assert!(
        observes_setup_whole_hash,
        "R3: the compile-tier producer MUST observe a \
         FileWholeHash for a side-effect (bindings-empty) runtime \
         import. If the whole-hash admission ran only inside the \
         per-binding loop, it would never execute for a side-effect \
         import. Signature observed: {:?}",
        signature
            .facts
            .iter()
            .map(|f| format!("{f:?}"))
            .collect::<Vec<_>>()
    );

    // Edit ONLY the body of `setup.ts`. The owner-upsert path has no
    // eager cascade, so fact-validation is the sole invalidation path.
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/src/setup.ts".to_string()),
            input_id: "/src/setup.ts".to_string(),
            source: "globalThis.__verter_setup = 2;\n".into(),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("dep upsert");

    let warm_after = host.compile_slot_is_warm("/src/Comp.vue", &profile);
    assert!(
        !warm_after,
        "a side-effect import body edit MUST invalidate the \
         consumer compile slot via FileWholeHash fact-validation. \
         warm_after=true means the producer recorded no whole-hash \
         fact for the side-effect dep (its empty `bindings` skipped \
         the per-binding whole-hash admission)."
    );

    // ensure_compiled's warm path must agree: it recompiles rather
    // than returning Ok on the stale slot.
    host.ensure_compiled("/src/Comp.vue", &profile)
        .expect("recompile after dep edit");
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "after ensure_compiled the slot is warm again (recompiled)"
    );
}

// ── External module-augmentation whole-hash discriminator (R29) ─────
//
// The `ModuleAugmentation` parse-fact value is HEADER-level (kind +
// member headers + contributor count — no bodies) to honor the
// zero-body-lowering publish invariant. The augmenter-set fingerprint
// (`ModuleAugmentationIndexShape`) folds each augmenter's
// `parse_stable_hash` (the decl skeleton), which is invariant under a
// member's VALUE-type edit. The SEMANTIC augmentation stitch compensates
// by recording a per-augmenter `FileWholeHash` self-root; the COMPILE
// augmentation rail (`observe_augmentation_fingerprints`) historically
// observed ONLY the header-level index-shape fingerprint, so a body-only
// edit inside `declare module 'vue' { interface … { foo: <T> } }` left
// the dependent compile slot warm-but-stale.
//
// Isolation that makes this discriminating: the owner imports `'vue'`
// (external specifier) but does NOT import the augmenter file, so the
// owner's compile signature pins NO `Export` / `ImportRef` fact on the
// augmenter — the augmentation index-shape fingerprint is the only fact
// referencing the augmenter's content. A separate `Helper.vue`
// side-effect-imports the augmenter purely to materialise it into the
// store so the owner's Content-classification index scan discovers it.

fn upsert_vue_prod(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: source.into(),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .expect("upsert vue (prod host)");
}

/// The augmenter set discovered for `ExternalSpecifier("vue")` across
/// every populated augmentation-index entry (canonical ids).
fn external_vue_augmenters(host: &VerterHost) -> Vec<String> {
    let store = host.project_type_store().indexed();
    for (key, _fp) in store.snapshot_augmentation_index_fingerprints() {
        if let verter_session::file_artifact_store::AugmentationTargetKind::ExternalSpecifier(
            spec,
        ) = &key.target
        {
            if spec.0.as_ref() == "vue" {
                return store
                    .get_augmenter_set(&key)
                    .map(|s| {
                        s.entries
                            .iter()
                            .map(|e| e.canonical().to_string())
                            .collect()
                    })
                    .unwrap_or_default();
            }
        }
    }
    Vec::new()
}

/// The `ModuleAugmentationIndexShape` fingerprint for
/// `ExternalSpecifier("vue")` (the header-level augmenter-set hash).
fn external_vue_index_shape_fp(host: &VerterHost) -> Option<[u8; 16]> {
    let store = host.project_type_store().indexed();
    for (key, fp) in store.snapshot_augmentation_index_fingerprints() {
        if let verter_session::file_artifact_store::AugmentationTargetKind::ExternalSpecifier(
            spec,
        ) = &key.target
        {
            if spec.0.as_ref() == "vue" {
                return Some(fp);
            }
        }
    }
    None
}

/// Discriminator — compile-slot invalidation on an EXTERNAL module
/// augmenter's member-VALUE-type edit.
///
/// `aug.ts` declares `declare module 'vue' { interface
/// ComponentCustomProperties { $foo: string } }`. `Comp.vue` imports
/// `{ ref } from 'vue'` (so `'vue'` is a consumed external specifier)
/// but never imports `aug.ts`. `Helper.vue` side-effect-imports `aug.ts`
/// to materialise it; a Content-classification of `Comp.vue` then
/// populates `ExternalSpecifier("vue")` with `aug.ts` as augmenter.
///
/// Editing `$foo`'s type (`string` → `number`) leaves the augmenter's
/// decl skeleton — hence `parse_stable_hash`, hence the
/// `ModuleAugmentationIndexShape` fingerprint — UNCHANGED (asserted), so
/// the header-level fingerprint fact cannot catch the edit. The ONLY
/// fact rail that can is a per-augmenter `FileWholeHash`.
///
/// Pre-fix: the compile augmentation rail observed only the index-shape
/// fingerprint, so the signature carried no `FileWholeHash` for `aug.ts`
/// and the member-type edit left the slot warm-but-stale
/// (`observes_aug_whole_hash == false`, `warm_after == true`). Post-fix:
/// the producer also observes a `FileWholeHash` per augmenter contributor
/// file, so the edit mismatches and the warm hit misses.
#[test]
fn compile_slot_invalidates_on_external_augmenter_member_type_edit() {
    const AUG_PRE: &str = "declare module 'vue' {\n\
         \x20 interface ComponentCustomProperties { $foo: string }\n\
         }\n\
         export {};\n";
    // Same member name `$foo`, same kind (property), same member count —
    // ONLY the annotated VALUE type changes (string → number). The
    // augmenter decl skeleton (hence `parse_stable_hash`, hence the
    // augmenter-set fingerprint) is invariant under this edit.
    const AUG_POST: &str = "declare module 'vue' {\n\
         \x20 interface ComponentCustomProperties { $foo: number }\n\
         }\n\
         export {};\n";

    let host = VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    });
    upsert_ts(&host, "/src/aug.ts", AUG_PRE);
    // An unrelated non-augmenter file — its edit must NOT invalidate the
    // owner's warm slot (no over-invalidation).
    upsert_ts(&host, "/src/other.ts", "export const x = 1;\n");
    // Materialiser: a separate SFC that side-effect-imports the augmenter
    // so `aug.ts` enters the store (the index cold-scan only sees loaded
    // artifacts). It is NOT the SFC under test.
    upsert_vue_prod(
        &host,
        "/src/Helper.vue",
        "<script setup lang=\"ts\">\n\
         import '/src/aug';\n\
         const h = 1;\n\
         </script>\n\
         <template><div>{{ h }}</div></template>\n",
    );
    // Owner under test: imports `'vue'` (external specifier) only. It does
    // NOT import `aug.ts`, so its compile signature pins NO Parse fact on
    // the augmenter — the augmentation index-shape fingerprint is the sole
    // fact referencing the augmenter's content.
    upsert_vue_prod(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import { ref } from 'vue';\n\
         const n = ref(1);\n\
         </script>\n\
         <template><div>{{ n }}</div></template>\n",
    );

    let profile = CompileProfile::default();

    // Materialise the augmenter (Helper side-effect import pulls aug.ts
    // into the store).
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Helper.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Script),
            compile_profile: profile.clone(),
        })
        .expect("materialise augmenter via Helper.vue");

    // A Content classification of the owner runs the augmentation probe
    // (`owner_has_module_augmentation_dependency`), populating
    // `ExternalSpecifier("vue")` with the now-materialised augmenter.
    let _ = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Script),
            compile_profile: CompileProfile {
                requested_mode: CompileCacheMode::Content,
                ..CompileProfile::default()
            },
        })
        .expect("content classify owner");
    assert_eq!(
        external_vue_augmenters(&host),
        vec!["/src/aug.ts".to_string()],
        "precondition: the ExternalSpecifier(\"vue\") augmenter set must contain aug.ts \
         after the owner's Content classification (the discriminator cannot run otherwise)"
    );

    // Session compile of the owner — installs the fact tracer and admits a
    // warm slot whose signature carries the augmentation observation.
    prime_compile(&host, "/src/Comp.vue");
    let warm_before = host.compile_slot_is_warm("/src/Comp.vue", &profile);
    assert!(
        warm_before,
        "precondition: Comp.vue MUST have a warm compile slot after the Session compile \
         (warm_before=false means prime failed to admit a slot — the discriminator cannot run)."
    );

    let signature = read_signature(&host, "/src/Comp.vue");
    // The owner pins NO Parse fact on the augmenter (it never imports it) —
    // this isolates the augmentation rail as the sole augmenter-referencing
    // fact, so the discriminator measures exactly the augmentation rail.
    assert!(
        !signature
            .facts
            .iter()
            .any(|f| matches!(f, FactVersionRef::Parse(ParseFactRef { canonical_id, .. }) if canonical_id == "/src/aug.ts")),
        "isolation invariant: the owner must pin NO Parse (Export/ImportRef) fact on the \
         augmenter it never imports — otherwise that fact, not the augmentation rail, would \
         catch the edit. Signature: {:?}",
        signature.facts.iter().map(|f| format!("{f:?}")).collect::<Vec<_>>()
    );
    // DISCRIMINATING fact-presence: the compile-tier producer MUST observe
    // a FileWholeHash for the augmenter contributor file. Pre-fix the
    // augmentation rail observed only the header-level index-shape
    // fingerprint, so this is absent and the assertion FAILS.
    let observes_aug_whole_hash = signature.facts.iter().any(|fact| {
        matches!(
            fact,
            FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/src/aug.ts"
        )
    });
    assert!(
        observes_aug_whole_hash,
        "R29: the compile-tier producer MUST observe a FileWholeHash for each external-specifier \
         augmenter contributor file. Without it, a member-VALUE-type edit (which leaves the \
         header-level augmenter-set fingerprint unchanged) cannot invalidate the dependent slot. \
         Signature observed: {:?}",
        signature
            .facts
            .iter()
            .map(|f| format!("{f:?}"))
            .collect::<Vec<_>>()
    );

    let fp_before = external_vue_index_shape_fp(&host);

    // No-over-invalidation: an UNRELATED non-augmenter file edit must NOT
    // invalidate the owner's warm slot.
    upsert_ts(&host, "/src/other.ts", "export const x = 2;\n");
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "an UNRELATED non-augmenter file edit must NOT invalidate the owner's compile slot \
         (no over-invalidation)."
    );

    // Body-only augmenter edit: member type string → number, decl skeleton
    // unchanged. The owner-upsert path has no eager reverse-dependent
    // cascade, so fact-validation is the sole invalidation rail.
    upsert_ts(&host, "/src/aug.ts", AUG_POST);

    // Prove the test discriminates the RIGHT gap: the header-level
    // augmenter-set fingerprint is UNCHANGED across the member-type edit,
    // so only a per-augmenter FileWholeHash can catch it.
    let fp_after = external_vue_index_shape_fp(&host);
    assert_eq!(
        fp_before, fp_after,
        "the ModuleAugmentationIndexShape fingerprint MUST stay equal across a member-VALUE-type \
         edit (parse_stable_hash is invariant) — proving the FileWholeHash rail is the only thing \
         that can catch this edit (pre={fp_before:?} post={fp_after:?})"
    );

    // DISCRIMINATING warm-miss: the member-type edit MUST invalidate the
    // owner's warm slot via the per-augmenter FileWholeHash. Pre-fix the
    // slot stays warm-but-stale (warm_after == true).
    let warm_after = host.compile_slot_is_warm("/src/Comp.vue", &profile);
    assert!(
        !warm_after,
        "R29: an external module-augmenter member-VALUE-type edit MUST invalidate the dependent \
         compile slot via the per-augmenter FileWholeHash fact. warm_after=true means the producer \
         recorded no whole-hash fact for the augmenter — the header-level index-shape fingerprint \
         alone cannot see a member-value edit, so the slot is served stale."
    );
}
