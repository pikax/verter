//! Type-resolution audit — long alias chain stack safety.
//!
//! Construct a 200-level deep `type T0 = T1; type T1 = T2; … type T199 = number;`
//! chain and resolve `T0`. The substrate must terminate without a
//! stack overflow AND `depth_high_water` must reflect the
//! truncation: bounded by `WALKER_DEPTH_CAP`.
//!
//! Discrimination contract: a regression that did NOT clamp the
//! recursion budget would either crash with a stack overflow
//! (test process aborts) or surface a `depth_high_water` value
//! beyond the cap. The post-change tree clamps at the cap and
//! returns a resolved value (the chain bottoms out at `number`).

use std::sync::Arc;

use verter_session::semantic_query::{ResolveDeclKey, ScopeId, SemanticQueryKey};
use verter_session::{HostConfig, UpsertRequest, VerterHost};

const CHAIN_LENGTH: usize = 200;

fn build_chain_source() -> String {
    let mut s = String::new();
    for i in 0..CHAIN_LENGTH {
        let next = i + 1;
        s.push_str(&format!("export type T{i} = T{next};\n"));
    }
    // Terminator: T200 = number.
    s.push_str(&format!("export type T{CHAIN_LENGTH} = number;\n"));
    s
}

#[test]
fn type_resolution_audit_long_chain_terminates_without_stack_overflow() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    }));
    let chain_source = build_chain_source();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/chain.ts".to_string()),
        input_id: "/chain.ts".to_string(),
        source: Arc::from(chain_source.as_str()),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static("/chain.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });

    // Resolve the head of the chain. The dispatcher walks
    // T0 -> T1 -> ... -> T200 = number; the substrate must not
    // overflow the call stack and must bound the recorded
    // `depth_high_water` at WALKER_DEPTH_CAP.
    let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from("/chain.ts"),
            local_scope: None,
        },
        name: Arc::from("T0"),
    });

    let (resolved, record) = host.resolve_type_with_audit(key, "/chain.ts").into_parts();
    let resolved = resolved
        .expect(
            "long chain must resolve — terminator T200 = number is a primitive that \
             cannot recurse further",
        )
        .expect("resolved node must be present");
    let _ = resolved;

    // record is always present now (carrier `audit` mandatory).
    let payload = record
        .type_resolution_payload()
        .expect("kind must be TypeResolution");

    // The high-water mark must NEVER exceed WALKER_DEPTH_CAP — this
    // is the load-bearing safety property the test characterises.
    assert!(
        payload.depth_high_water <= verter_audit::WALKER_DEPTH_CAP,
        "depth_high_water must be clamped at WALKER_DEPTH_CAP for long chains. \
         observed = {}, cap = {}",
        payload.depth_high_water,
        verter_audit::WALKER_DEPTH_CAP,
    );

    // Sanity: the chain produced at least some hops.
    assert!(
        payload.hops >= 1,
        "long chain must produce at least one hop — got {}",
        payload.hops
    );
}
