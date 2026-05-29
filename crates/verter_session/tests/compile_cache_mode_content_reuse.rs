//! D9 #3 — `Content` mode is content-addressed, never fact-validated.
//!
//! Positive: two compiles of a FACT-FREE SFC (no cross-file dep) with
//! identical `(content, env, Content mode)` reuse one
//! `CompileOutputNode_PureContent` entry — exactly one entry exists
//! after the first compile, and the second compile returns byte-identical
//! output without growing the store.
//!
//! Negative: a `Content` request on an SFC that DOES carry a cross-file
//! dependency (a macro type imported from a workspace `.ts`) downgrades
//! to `Stateless` per the matrix — it publishes NO content-addressed
//! entry and NO session slot.
//!
//! Discrimination against the pre-B5 tree (`204b5ef9`): no `Content`
//! mode, no content-addressed node, no entry-count accessor — does not
//! compile. Against a tree that fact-validated Content, the
//! cross-file-downgrade negative assertion (entry count stays 0) fails.

use verter_session::{
    CompileCacheMode, CompileErrorPolicy, CompileProfile, DowngradeReason, FileKind, HostConfig,
    UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
};

/// A production (non-dev) host config. The default `HostConfig` enables
/// `dev_mode` + `DevServeLastKnownGood`, which fires the
/// `HasDevLastGood` reason on EVERY compile and would downgrade every
/// `Content` request to `Stateless`. A `Content` request is only
/// reachable as `Content` when no reason fires, so these tests use a
/// production config to make the fact-free Content path actually run as
/// Content.
fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

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

fn content_profile() -> CompileProfile {
    CompileProfile {
        requested_mode: CompileCacheMode::Content,
        ..CompileProfile::default()
    }
}

fn compile(host: &VerterHost, canonical: &str, profile: &CompileProfile) -> String {
    host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical.to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile.clone(),
    })
    .expect("compile")
    .code
    .to_string()
}

// A fact-free SFC: no imports, no cross-file deps → no reason fires →
// the Content request actually runs as Content.
const FACT_FREE: &str =
    "<script setup lang=\"ts\">const n = 1</script><template><div>{{ n }}</div></template>";

#[test]
fn content_mode_reuses_one_pure_content_entry() {
    let host = host();
    upsert_vue(&host, "/Plain.vue", FACT_FREE);
    let profile = content_profile();

    let code1 = compile(&host, "/Plain.vue", &profile);
    // Exactly one content-addressed entry after the first compile.
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        1,
        "first Content compile of a fact-free SFC must publish exactly one content entry"
    );

    let code2 = compile(&host, "/Plain.vue", &profile);
    // The second compile reuses the same entry — no growth, identical code.
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        1,
        "second Content compile must REUSE the existing entry, not add a new one"
    );
    assert_eq!(
        code1, code2,
        "Content warm hit must return byte-identical output"
    );

    // Content is NOT fact-validated: it never publishes a session slot.
    assert!(
        host.compile_slot_fact_dep_signature("/Plain.vue", &profile)
            .is_none(),
        "Content mode must NOT publish a fact-validated session slot"
    );
}

#[test]
fn content_request_with_cross_file_dep_downgrades_to_stateless() {
    // Negative: a cross-file macro type dep makes the pure key unsafe, so
    // a Content request floors to Stateless — NO content entry, NO
    // session slot.
    let host = host();
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
    let profile = content_profile();

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Script),
            compile_profile: profile.clone(),
        })
        .expect("compile");

    // The request asked for Content but ran as Stateless (a reason fired).
    assert_eq!(response.requested_mode, CompileCacheMode::Content);
    assert_eq!(
        response.actual_mode,
        CompileCacheMode::Stateless,
        "a Content request on a cross-file-dependent SFC MUST downgrade to Stateless"
    );
    assert!(
        response.downgrade_reason.is_some(),
        "the downgrade must carry a reason"
    );

    // Stateless floor ⇒ NO content-addressed entry.
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        0,
        "a downgraded Content request must NOT publish a content-addressed entry"
    );
    // And NO session slot (Stateless publishes nothing).
    assert!(
        host.compile_slot_fact_dep_signature("/src/Comp.vue", &profile)
            .is_none(),
        "a downgraded Content request must NOT publish a session slot"
    );
}

#[test]
fn content_request_with_imported_module_augmentation_downgrades_to_stateless() {
    // An augmenter that augments a module the owner imports leaves NO
    // trace on the owner's OWN declared augmentations — it lives in a
    // separate file (`declare module 'vue' { ... }`). The owner imports
    // from 'vue' but declares no augmentation itself. A content-addressed
    // key carries no augmenter fingerprint, so editing the augmenter would
    // leave the key byte-identical and serve stale output; the classifier
    // MUST recognise the imported augmentation (via the augmentation
    // target index) and floor the Content request to Stateless.
    let host = host();
    // The augmenter both exports a value (so a relative import pulls it
    // into the program) AND augments the imported module 'vue'.
    upsert_ts(
        &host,
        "/src/augment.ts",
        "export const marker = 1;\n\
         declare module 'vue' {\n\
         \x20 interface ComponentCustomProperties { $foo: string }\n\
         }\n",
    );
    // The owner imports the augmenter relatively (forcing it into the
    // program) and imports a value from 'vue' (the augmented module). It
    // uses neither in a macro, so the ONLY cross-file reason available is
    // the module augmentation — no HasMacroTypeDeps.
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import { marker } from './augment';\n\
         import { ref } from 'vue';\n\
         const n = ref(marker);\n\
         </script>\n\
         <template><div>{{ n }}</div></template>\n",
    );
    let profile = content_profile();

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Script),
            compile_profile: profile.clone(),
        })
        .expect("compile");

    // The FIRST request already downgrades — no edit-replay needed.
    assert_eq!(response.requested_mode, CompileCacheMode::Content);
    assert_eq!(
        response.actual_mode,
        CompileCacheMode::Stateless,
        "a Content request whose owner imports a module-augmented dependency MUST downgrade to Stateless"
    );
    assert_eq!(
        response.downgrade_reason,
        Some(DowngradeReason::HasModuleAugmentation),
        "the highest-priority firing reason must be HasModuleAugmentation, got {:?}",
        response.downgrade_reason
    );
    // Stateless floor ⇒ NO content-addressed entry was published.
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        0,
        "a downgraded Content request must NOT publish a content-addressed entry"
    );
}

#[test]
fn targeted_invalidation_evicts_content_entry_and_forces_recompile() {
    // A `Content` key carries no fact rail, so a targeted
    // `invalidate_compile_slots` MUST evict the content-addressed entry
    // for that canonical — otherwise a same-content recompile would warm-
    // hit and report `cache_hit = true`, breaking the force-recompute
    // contract. The per-canonical reverse index on the content node is the
    // eviction authority.
    let host = host();
    upsert_vue(&host, "/Plain.vue", FACT_FREE);
    let profile = content_profile();

    // First Content compile publishes exactly one content entry.
    let _ = compile(&host, "/Plain.vue", &profile);
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        1,
        "first Content compile of a fact-free SFC must publish exactly one content entry"
    );

    // Targeted invalidation must flush the content-addressed entry.
    host.invalidate_compile_slots("/Plain.vue");
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        0,
        "invalidate_compile_slots MUST evict the content-addressed entry for the canonical"
    );

    // The next request recompiles (cold) instead of warm-hitting.
    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/Plain.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile.clone(),
        })
        .expect("recompile");
    assert_eq!(
        response.actual_mode,
        CompileCacheMode::Content,
        "the fact-free SFC still classifies as Content after invalidation"
    );
    assert!(
        !response.cache_hit,
        "after a targeted invalidation the next Content request MUST recompile (cache_hit == false), \
         not warm-hit a stale content entry"
    );
    // The cold recompute re-publishes exactly one entry.
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        1,
        "the cold recompute must re-publish exactly one content entry"
    );
}
