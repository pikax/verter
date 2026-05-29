//! D9 #1 — integration-scope classifier table.
//!
//! Exercises the real classifier through `get_virtual_file` (the
//! production path; `classify_compile_mode` / `EligibilityInputs` are
//! crate-private, so the integration scope observes the classification
//! via the response's `requested_mode` / `actual_mode` /
//! `downgrade_reason` fields). For each (requested_mode, eligibility)
//! combination, the response must match the corrected matrix:
//!
//!   - Session + any reason  -> Session  (no public downgrade_reason move)
//!   - Content + a reason     -> Stateless (downgrade_reason populated)
//!   - Content + no reason    -> Content   (no downgrade)
//!   - Stateless + anything   -> Stateless (no reasons)
//!
//! Discrimination against the pre-B5 tree (`204b5ef9`): no
//! `requested_mode` / `actual_mode` / `downgrade_reason` fields — does
//! not compile. Against the buggy `c8b8d709` fold (Session->Stateless),
//! the `Session + reason -> Session` rows fail.

use verter_session::{
    CompileCacheMode, CompileErrorPolicy, CompileProfile, FileKind, HostConfig, UpsertRequest,
    VerterHost, VirtualNodeKind, VirtualQuery,
};

/// Production (non-dev) host config. The default `HostConfig` enables
/// `dev_mode` + `DevServeLastKnownGood`, which fires `HasDevLastGood` on
/// every compile (a config-driven reason). These matrix rows isolate the
/// INPUT-driven reasons (or their absence), so they use a production
/// config where no config-driven reason fires.
fn new_host() -> VerterHost {
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

/// Compile `canonical` under `mode` and return `(actual_mode,
/// downgrade_fired)`.
fn classify_via_compile(
    host: &VerterHost,
    canonical: &str,
    mode: CompileCacheMode,
) -> (CompileCacheMode, bool) {
    let profile = CompileProfile {
        requested_mode: mode,
        ..CompileProfile::default()
    };
    let r = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical.to_string()),
            node_kind: Some(VirtualNodeKind::Script),
            compile_profile: profile,
        })
        .expect("compile");
    assert_eq!(r.requested_mode, mode, "requested_mode must round-trip");
    (r.actual_mode, r.downgrade_reason.is_some())
}

// A fact-free SFC: no cross-file dep → no reason fires.
const FACT_FREE: &str =
    "<script setup lang=\"ts\">const n = 1</script><template><div>{{ n }}</div></template>";

// A cross-file SFC: a macro type dep imported from a workspace `.ts`
// → HasMacroTypeDeps fires.
const CROSS_FILE: &str = "<script setup lang=\"ts\">\n\
     import type { Foo } from './types';\n\
     defineProps<Foo>();\n\
     </script>\n";

fn seed_cross_file(host: &VerterHost, canonical: &str) {
    upsert_ts(
        host,
        "/src/types.ts",
        "export interface Foo { a: number; }\n",
    );
    upsert_vue(host, canonical, CROSS_FILE);
}

#[test]
fn matrix_fact_free_input() {
    // No reason fires: each mode passes through unchanged (Content stays
    // Content; Session stays Session; Stateless stays Stateless).
    let host = new_host();
    upsert_vue(&host, "/F1.vue", FACT_FREE);
    assert_eq!(
        classify_via_compile(&host, "/F1.vue", CompileCacheMode::Session),
        (CompileCacheMode::Session, false)
    );

    let host = new_host();
    upsert_vue(&host, "/F2.vue", FACT_FREE);
    assert_eq!(
        classify_via_compile(&host, "/F2.vue", CompileCacheMode::Content),
        (CompileCacheMode::Content, false),
        "a fact-free Content request must stay Content"
    );

    let host = new_host();
    upsert_vue(&host, "/F3.vue", FACT_FREE);
    assert_eq!(
        classify_via_compile(&host, "/F3.vue", CompileCacheMode::Stateless),
        (CompileCacheMode::Stateless, false)
    );
}

#[test]
fn matrix_cross_file_input() {
    // A reason fires (HasMacroTypeDeps). Session stays Session but
    // RECORDS the reason for telemetry (the public downgrade_reason is
    // populated even though the mode is unchanged); Content downgrades to
    // Stateless with a reason; Stateless stays Stateless and records NO
    // reason (the floor ignores reasons).
    let host = new_host();
    seed_cross_file(&host, "/src/S.vue");
    assert_eq!(
        classify_via_compile(&host, "/src/S.vue", CompileCacheMode::Session),
        (CompileCacheMode::Session, true),
        "Session + reason MUST stay Session (mode unchanged) while recording the reason \
         for telemetry. Against the c8b8d709 fold this actual_mode would be Stateless."
    );

    let host = new_host();
    seed_cross_file(&host, "/src/C.vue");
    assert_eq!(
        classify_via_compile(&host, "/src/C.vue", CompileCacheMode::Content),
        (CompileCacheMode::Stateless, true),
        "Content + reason MUST downgrade to Stateless with a reason"
    );

    let host = new_host();
    seed_cross_file(&host, "/src/X.vue");
    assert_eq!(
        classify_via_compile(&host, "/src/X.vue", CompileCacheMode::Stateless),
        (CompileCacheMode::Stateless, false),
        "Stateless ignores reasons (the floor records none)"
    );
}
