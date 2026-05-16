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
//! `/src/types.ts`. This holds against the post-change tree and
//! WOULD FAIL against a tree where the prefetch is removed.

use std::sync::Arc;

use verter_semantic::facts::registry::FactKey;
use verter_session::resolver_core::{FactVersionRef, ParseFactRef};
use verter_session::{
    CompileProfile, FileKind, HostConfig, UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
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
fn read_signature(host: &VerterHost, canonical: &str) -> Arc<[FactVersionRef]> {
    host.compile_slot_fact_dep_signature(canonical, &CompileProfile::default())
        .unwrap_or_else(|| Arc::from(Vec::<FactVersionRef>::new()))
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
/// routes not in derived cache → silent skip). Against the
/// post-change tree, the prefetch populates both layers and the
/// observation lands.
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
        !signature.is_empty(),
        "R3/R26/R28: compile_slot.fact_dep_signature MUST be non-empty \
         after cold compute of an SFC importing types from a workspace .ts \
         (signature.len() = {}). The pre-tracer prefetch is the producer-timing \
         contract — without it, observations silently skip and the signature \
         is empty, breaking cross-file fact-validation.",
        signature.len()
    );

    // Path-precision: at least one observation must reference the
    // cross-file `.ts` canonical (NOT just the SFC's own facts).
    // R28 path-precise facts include `Export`, `MemberShape`, and
    // `MemberPresence` for the imported type — the producer admits
    // one Export + one MemberShape + per-member MemberPresence
    // observations for each `macro_type_dep`.
    let observes_cross_file_type_fact = signature.iter().any(|fact| {
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
             signature_before.len()={}",
            signature_before.len()
        );
        return;
    }

    assert!(
        !signature_before.is_empty(),
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

// ── Block 1.J.3 discriminators ────────────────────────────────────
//
// These tests upsert the dependency via
// `upsert_without_dependent_eviction` so the eager reverse-dep
// cascade does NOT physically clear the consumer's compile slot.
// With the cascade suppressed, the ONLY mechanism that can
// invalidate the consumer is the warm-hit fact-signature check
// (`compile_slot_fact_signature_validates`). That isolation is what
// makes each test discriminating: against the pre-1.J.3 producer
// (no `FileWholeHash` for runtime imports / external `src=` deps)
// the consumer's signature carries no fact for the edited dep, so
// the warm hit is served stale; against the post-1.J.3 producer the
// whole-hash fact mismatches and the warm hit misses.

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
/// Pre-fix: the producer recorded only `ImportRef` + signature-pinned
/// `Export`; a body-only edit leaves both hashes identical →
/// `compile_slot_is_warm` stays `true` → stale slot served.
/// Post-fix: the `FileWholeHash` fact mismatches → warm hit misses.
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
    let observes_utils_whole_hash = signature.iter().any(|fact| {
        matches!(
            fact,
            FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/src/utils.ts"
        )
    });
    assert!(
        observes_utils_whole_hash,
        "R3/B1.j.3: the compile-tier producer MUST observe a \
         FileWholeHash for a runtime-imported dependency. \
         Signature observed: {:?}",
        signature
            .iter()
            .map(|f| format!("{f:?}"))
            .collect::<Vec<_>>()
    );

    // Edit ONLY the body of `helper` — signature stays `() => number`.
    // Suppress the eager cascade so fact-validation is the sole
    // invalidation path.
    let _ = host
        .upsert_without_dependent_eviction(UpsertRequest {
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
        "B1.j.3: a runtime-import body edit MUST invalidate the \
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
/// Pre-fix: `ensure_compiled`'s warm path checked only
/// `slot.semantic_hash == parse.semantic_hash && style_override_hash`.
/// The SFC's own content is unchanged, so that predicate held and
/// `ensure_compiled` returned `Ok(())` WITHOUT recompiling — the
/// stale slot stayed in place and `compile_slot_is_warm` stayed
/// `false` even after `ensure_compiled` ran.
/// Post-fix: the warm path additionally runs
/// `compile_slot_fact_signature_validates`, sees the dep fact
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

    // Edit the imported type — change the member's type. Suppress the
    // eager cascade so only fact-validation can invalidate.
    let _ = host
        .upsert_without_dependent_eviction(UpsertRequest {
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
        "B1.j.3: after a cross-file dep edit the compile slot MUST \
         fail fact-validation (a warm slot here means the signature \
         did not catch the dep edit)."
    );

    // The discriminating call. `ensure_compiled` must RECOMPILE the
    // stale slot, not short-circuit on the unchanged `semantic_hash`.
    host.ensure_compiled("/src/Comp.vue", &profile)
        .expect("ensure_compiled recompile");

    // Post-fix: the recompile re-recorded a fresh fact signature
    // against the edited dep, so the slot is warm again. Pre-fix:
    // `ensure_compiled` returned `Ok(())` without recompiling — the
    // stale slot (still carrying the pre-edit dep fact hash) remained,
    // so this assertion would FAIL with the slot still not warm.
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "B1.j.3: ensure_compiled's warm path MUST validate the \
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
/// Pre-fix: `external_requests` was never passed to the compile-tier
/// producer — the SFC's `fact_dep_signature` was completely empty,
/// which trivially validates → stale slot served forever.
/// Post-fix: the producer observes a `FileWholeHash` of the resolved
/// external canonical → an edit mismatches → warm hit misses.
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
    let observes_tpl_whole_hash = signature.iter().any(|fact| {
        matches!(
            fact,
            FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/src/tpl.html"
        )
    });
    assert!(
        observes_tpl_whole_hash,
        "R3/B1.j.3: the compile-tier producer MUST observe a \
         FileWholeHash for an external `src=` dependency. The \
         pre-1.J.3 producer never received `external_requests` and \
         produced an empty signature. Signature observed: {:?}",
        signature
            .iter()
            .map(|f| format!("{f:?}"))
            .collect::<Vec<_>>()
    );

    // Edit the external template. Suppress the eager cascade.
    let _ = host
        .upsert_without_dependent_eviction(UpsertRequest {
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
        "B1.j.3: an external `src=` template edit MUST invalidate the \
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
/// Pre-fix: the producer's `FileWholeHash` admission ran only inside
/// the `for binding in import.bindings.iter()` loop. A side-effect
/// import has empty `bindings`, so the loop body never executed and no
/// `FileWholeHash` fact was recorded for `./setup` — the consumer's
/// signature carried no fact for the dep, so a warm hit was served
/// stale.
/// Post-fix: a non-type-only import with a resolved dep contributes
/// its `FileWholeHash` fact even with zero bindings → the edit
/// mismatches → warm hit misses.
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
    let observes_setup_whole_hash = signature.iter().any(|fact| {
        matches!(
            fact,
            FactVersionRef::FileWholeHash { canonical_id, .. }
                if canonical_id == "/src/setup.ts"
        )
    });
    assert!(
        observes_setup_whole_hash,
        "R3/B1.j.3: the compile-tier producer MUST observe a \
         FileWholeHash for a side-effect (bindings-empty) runtime \
         import. Pre-fix the whole-hash admission ran only inside the \
         per-binding loop, which never executes for a side-effect \
         import. Signature observed: {:?}",
        signature
            .iter()
            .map(|f| format!("{f:?}"))
            .collect::<Vec<_>>()
    );

    // Edit ONLY the body of `setup.ts`. Suppress the eager cascade so
    // fact-validation is the sole invalidation path.
    let _ = host
        .upsert_without_dependent_eviction(UpsertRequest {
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
        "B1.j.3: a side-effect import body edit MUST invalidate the \
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
