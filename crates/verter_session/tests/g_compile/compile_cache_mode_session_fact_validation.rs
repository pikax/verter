//! D9 #4 — `Session`-mode fact-validated warm hit + cross-file miss.
//!
//! This is the load-bearing proof that the corrected downgrade matrix
//! kept `Session` for cross-file-dependent SFCs. A host-default
//! (`Session`) compile of an SFC that imports a type from a workspace
//! `.ts` file:
//!
//!   1. routes to `Session` (NOT `Stateless`), so a non-empty
//!      `fact_dep_signature` is recorded on the compile slot,
//!   2. warm-hits while the imported type is unchanged,
//!   3. MISSES when the imported type's body changes (the recorded
//!      cross-file fact's signature flips).
//!
//! Discrimination against the pre-correction tree (`c8b8d709`, whose
//! fold collapsed `Session -> Stateless` on any reason): a cross-file
//! SFC would route to `Stateless`, publish NO session slot, record an
//! EMPTY `fact_dep_signature`, and never warm-hit through the session
//! node. The non-empty-signature + warm-then-miss assertions below
//! fail against that tree and pass against the corrected one.

use verter_session::{
    CompileCacheMode, CompileProfile, FileKind, ReadSetSignature, UpsertRequest, VerterHost,
    VirtualNodeKind, VirtualQuery,
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

/// Session-mode compile (host default). The profile's `requested_mode`
/// defaults to `Session`.
fn session_profile() -> CompileProfile {
    let p = CompileProfile::default();
    assert_eq!(
        p.requested_mode,
        CompileCacheMode::Session,
        "the host-default profile must request Session"
    );
    p
}

fn prime_compile(host: &VerterHost, canonical: &str) {
    let _ = host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical.to_string()),
        node_kind: Some(VirtualNodeKind::Script),
        compile_profile: session_profile(),
    });
}

fn read_signature(host: &VerterHost, canonical: &str) -> ReadSetSignature {
    host.compile_slot_fact_dep_signature(canonical, &session_profile())
        .unwrap_or_else(ReadSetSignature::empty)
}

#[test]
fn cross_file_session_compile_records_facts_warm_hits_and_misses_on_edit() {
    let host = VerterHost::new_standalone(verter_session::HostConfig::default());
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

    // Cold Session compile.
    prime_compile(&host, "/src/Comp.vue");
    let profile = session_profile();

    // (1) Session route ⇒ the slot carries a NON-EMPTY cross-file fact
    // signature. Against `c8b8d709`'s Session->Stateless collapse, the
    // Stateless route publishes no slot and this signature is empty.
    let signature = read_signature(&host, "/src/Comp.vue");
    assert!(
        !signature.facts.is_empty(),
        "Session-mode cross-file compile MUST publish a session slot with a non-empty \
         fact_dep_signature (got {} facts). A Session->Stateless collapse would leave it empty.",
        signature.facts.len()
    );

    // (2) Warm hit while the imported type is unchanged.
    let warm_before = host.compile_slot_is_warm("/src/Comp.vue", &profile);
    assert!(
        warm_before,
        "Session-mode slot MUST be warm immediately after a successful cold compile"
    );

    // (3) Edit the imported type's member body → the recorded
    // cross-file fact's signature flips → warm hit MUST miss.
    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface Foo { a: string; }\n",
    );
    let warm_after = host.compile_slot_is_warm("/src/Comp.vue", &profile);
    assert!(
        !warm_after,
        "editing the imported type body MUST invalidate the Session warm hit via \
         fact-validation; a stale warm hit means the fact rail was not consulted"
    );
}

#[test]
fn unrelated_edit_keeps_session_warm_hit() {
    // Negative discriminator: an over-eager "always cold" routing would
    // also pass the miss-on-edit test. This pins that an UNRELATED edit
    // leaves the Session warm hit intact.
    let host = VerterHost::new_standalone(verter_session::HostConfig::default());
    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface Foo { a: number; }\n",
    );
    upsert_ts(&host, "/src/unrelated.ts", "export const x = 1;\n");
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { Foo } from './types';\n\
         defineProps<Foo>();\n\
         </script>\n",
    );

    prime_compile(&host, "/src/Comp.vue");
    let profile = session_profile();
    let warm_before = host.compile_slot_is_warm("/src/Comp.vue", &profile);
    assert!(warm_before, "slot must be warm after cold compile");

    // Edit a file the SFC does NOT depend on.
    upsert_ts(&host, "/src/unrelated.ts", "export const x = 2;\n");
    let warm_after = host.compile_slot_is_warm("/src/Comp.vue", &profile);
    assert!(
        warm_after,
        "an unrelated edit MUST NOT invalidate the Session warm hit (path-precise facts)"
    );
}
