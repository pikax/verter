//! D9 #5 — the `CompileModeDowngrade` structured audit event.
//!
//! A `Content`-requested compile on an SFC with a downgrade reason emits
//! `StructuredAuditEvent::CompileModeDowngrade` EXACTLY ONCE, with
//! `requested = Content`, `actual = Stateless`, and the full ordered
//! reason vector.
//!
//! Negative: a host-default `Session` compile of the same SFC emits NO
//! `CompileModeDowngrade` event (Session never changes mode under the
//! matrix).
//!
//! Discrimination against the pre-B5 tree (`204b5ef9`): the
//! `CompileModeDowngrade` variant and the `requested_mode` profile field
//! do not exist — does not compile. Against a tree that emitted the
//! event for Session moves, the negative assertion fails.

use std::sync::Arc;

use verter_audit::payloads::tags::{CompileCacheModeTag, DowngradeReasonTag};
use verter_session::component_meta_audit::accumulator::RequestFootprintAccumulator;
use verter_session::component_meta_audit::StructuredAuditEvent;
use verter_session::request_context::{RequestContext, RequestContextGuard};
use verter_session::{
    CompileCacheMode, CompileProfile, FileKind, HostConfig, UpsertRequest, VerterHost,
    VirtualNodeKind, VirtualQuery,
};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
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

fn profile(mode: CompileCacheMode) -> CompileProfile {
    CompileProfile {
        requested_mode: mode,
        ..CompileProfile::default()
    }
}

fn compile_collecting_events(
    host: &VerterHost,
    canonical: &str,
    mode: CompileCacheMode,
) -> Vec<StructuredAuditEvent> {
    let acc = Arc::new(RequestFootprintAccumulator::new());
    let ctx = RequestContext::new(7, Arc::from(canonical), true, Some(Arc::clone(&acc)));
    let _g = RequestContextGuard::install(Arc::clone(&ctx));
    let _ = host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical.to_string()),
        node_kind: Some(VirtualNodeKind::Script),
        compile_profile: profile(mode),
    });
    acc.drain().structured_events
}

const CROSS_FILE: &str = "<script setup lang=\"ts\">\n\
     import type { Foo } from './types';\n\
     defineProps<Foo>();\n\
     </script>\n";

#[test]
fn content_request_with_reason_emits_one_downgrade_event() {
    let host = host();
    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface Foo { a: number; }\n",
    );
    upsert_vue(&host, "/src/Comp.vue", CROSS_FILE);

    let events = compile_collecting_events(&host, "/src/Comp.vue", CompileCacheMode::Content);
    let downgrades: Vec<&StructuredAuditEvent> = events
        .iter()
        .filter(|e| matches!(e, StructuredAuditEvent::CompileModeDowngrade { .. }))
        .collect();

    assert_eq!(
        downgrades.len(),
        1,
        "a Content request with a reason MUST emit exactly one CompileModeDowngrade, got {}: {:?}",
        downgrades.len(),
        downgrades
    );

    match downgrades[0] {
        StructuredAuditEvent::CompileModeDowngrade {
            requested,
            actual,
            reasons,
        } => {
            assert_eq!(*requested, CompileCacheModeTag::Content);
            assert_eq!(*actual, CompileCacheModeTag::Stateless);
            assert!(
                !reasons.is_empty(),
                "the downgrade event must carry the ordered reason vector"
            );
            // A cross-file macro type dep fires HasMacroTypeDeps.
            assert!(
                reasons.contains(&DowngradeReasonTag::HasMacroTypeDeps),
                "the cross-file macro type dep must appear in the reason vector: {reasons:?}"
            );
        }
        other => panic!("expected CompileModeDowngrade, got {other:?}"),
    }
}

#[test]
fn session_request_emits_no_downgrade_event() {
    // Negative: the SAME cross-file SFC compiled in the host-default
    // Session mode must NOT emit a downgrade — Session never changes mode.
    let host = host();
    upsert_ts(
        &host,
        "/src/types.ts",
        "export interface Foo { a: number; }\n",
    );
    upsert_vue(&host, "/src/Comp.vue", CROSS_FILE);

    let events = compile_collecting_events(&host, "/src/Comp.vue", CompileCacheMode::Session);
    let downgrades = events
        .iter()
        .filter(|e| matches!(e, StructuredAuditEvent::CompileModeDowngrade { .. }))
        .count();
    assert_eq!(
        downgrades, 0,
        "a Session request MUST NOT emit CompileModeDowngrade (Session never moves), got {downgrades}"
    );
}
