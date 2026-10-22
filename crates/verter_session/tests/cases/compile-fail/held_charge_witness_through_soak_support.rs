//! The soak/tooling runner's capture-off profile (`soak-support` alone)
//! observes a held answer's allocation charges: a flow return delivered by
//! the host's public request keeps a payload and its delivery's bookkeeping,
//! each charged exactly what it measures; across the public close the same
//! allocations keep the same bytes while the payload changes class, and the
//! host drains through its public release boundary.

use std::sync::Arc;

use verter_session::{HostConfig, UpsertRequest, VerterHost};
use verter_type_engine::semantic_query::ReturnProjectionDemand;
use verter_type_engine::semantic_query_memo::charge_witness::HeldChargeWitness;
use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};

fn main() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/src/held.ts".to_string()),
        input_id: "/src/held.ts".to_string(),
        source: Arc::from(
            "declare function take(x: number): string;\n\
             export function pick() { return take(\"s\"); }\n",
        ),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static("/src/held.ts")
            .static_resolution(),
        aliases: Vec::new(),
    });
    let function = FlowFunctionReturnIdentity {
        anchor: AuthoredAnchor {
            canonical_id: verter_type_expr::OwnedStr::from("/src/held.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: verter_type_expr::OwnedStr::from("pick"),
            space: LocatorSymbolSpace::Value,
        },
        function_part: FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let held = host
        .get_flow_return_type_with_audit(&function, ReturnProjectionDemand::whole_return())
        .into_parts()
        .0
        .expect("the flow return resolves");
    let cached = HeldChargeWitness::of_flow_return(&held);
    assert!(!cached.payload().is_empty() && !cached.root_metadata().is_empty());
    assert!(cached.is_exact());
    host.close();
    host.release_completion().wait().expect("the queued releases apply");
    let remaining = HeldChargeWitness::of_flow_return(&held);
    assert!(remaining.is_exact());
    assert_eq!(
        remaining.payload().allocations().collect::<Vec<_>>(),
        cached.payload().allocations().collect::<Vec<_>>()
    );
    assert_eq!(
        remaining.payload().charged_bytes(),
        cached.payload().charged_bytes()
    );
    assert_ne!(remaining.payload(), cached.payload(), "the close moved its class");
    drop(held);
    host.close();
    host.release_completion().wait().expect("the queued releases apply");
}
