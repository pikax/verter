use crate::types::HostConfig;
use crate::types::UpsertRequest;
use crate::VerterHost;
use std::sync::Arc;
use verter_type_engine::project_semantic_dispatch::reactive_wrapper::*;
use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_expr::ReactiveWrapperRole;
use verter_type_expr::ReactiveWrapperUnresolvedReason;
use verter_type_expr::TopLevelOwnerId;

/// A CLAMPED connected-work envelope degrades the return role to the exact
/// typed envelope reason with no provenance — never a wrapper role guessed
/// from the authored spelling. The unclamped control on the same host proves
/// the subject is exactly resolvable, so the clamp is what degrades it.
///
/// The envelope limit is settable only inside this module tree, which is why
/// the host-boundary acceptance test
/// (`return_wrapper_role_degrades_typed_and_is_never_warmed`) delegates this
/// arm here: this test constructs its dispatch through the direct-host test seam, so
/// `current_request_budget()` is `None` at the sole projection-op charge
/// site (`project_semantic_dispatch/mod.rs`) and the envelope class is
/// proven through the connected-work limit rather than the projection fuse.
///
/// The production consumer resolves under the component-meta request's
/// already-installed `RequestContext` instead — that the demand runs through
/// the request's own context is proven at the public
/// boundary by
/// `component_meta_binding_return_wrapper_role_demand_is_request_bound`
/// (a session overlay decides the role). No test asserts a projection-fuse
/// TRIP on the consumer path; the envelope class stays proven here.
#[test]
fn clamped_connected_work_envelope_degrades_the_return_role_typed() {
    let upsert = |host: &VerterHost, canonical: &str, source: &str| {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical.to_string()),
                input_id: canonical.to_string(),
                source: Arc::from(source),
                file_language: verter_language::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("upsert");
    };
    let make = || {
        let host = VerterHost::new_standalone(HostConfig::default());
        upsert(
            &host,
            "/workspace/node_modules/vue/index.d.ts",
            "export interface Ref<T> { value: T }\n",
        );
        upsert(
            &host,
            "/workspace/src/subject.ts",
            "import type { Ref } from 'vue'\n\
             export function getValue(): Ref<number> { return null as never; }\n",
        );
        host.set_import_dependencies(
            "/workspace/src/subject.ts",
            vec![crate::types::DependencyResolution {
                specifier: "vue".to_string(),
                resolved_canonical_id: Some("/workspace/node_modules/vue/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        host
    };
    let role_for = |host: &VerterHost, work_limit: Option<usize>| {
        let dispatch = ProjectSemanticDispatch::new(host);
        if let Some(work_limit) = work_limit {
            dispatch.set_connected_limits_for_tests(work_limit, u16::MAX);
        }
        wrapper_role_for_value_signature_return(
            &dispatch,
            "/workspace/src/subject.ts",
            TopLevelOwnerId::ordinary_file(),
            "getValue",
            0,
        )
    };

    // Control: unclamped, on its own cold host — exactly resolvable.
    let (role, provenance) = role_for(&make(), None);
    assert_eq!(role, ReactiveWrapperRole::Ref);
    assert_eq!(
        provenance
            .expect("route proof")
            .terminal_import_source
            .as_ref(),
        "vue"
    );

    // Clamped: the shared envelope trips and the role degrades typed.
    let (role, provenance) = role_for(&make(), Some(0));
    assert_eq!(
        role,
        ReactiveWrapperRole::Unresolved {
            reason: ReactiveWrapperUnresolvedReason::WorkLimitExceeded
        },
        "a tripped connected-work envelope must publish the exact typed reason"
    );
    assert!(
        provenance.is_none(),
        "a truncated demand must publish no provenance"
    );
    assert_ne!(
        role,
        ReactiveWrapperRole::None,
        "an envelope trip is NOT a completed non-wrapper proof"
    );
}
