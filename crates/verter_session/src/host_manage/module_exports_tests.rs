use super::*;
use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount, StoreAccount};

#[test]
fn export_table_refusals_keep_the_complete_answer_and_typed_reason() {
    for reason in [
        None,
        Some(NonAdmissionReason::MutationUnstable),
        Some(NonAdmissionReason::UnresolvedProvenance),
        Some(NonAdmissionReason::RetentionPressure),
    ] {
        let mut host = VerterHost::new_standalone(crate::HostConfig::default());
        for (canonical, source) in [
            ("/leaf.ts", "export const x = 1 as const;"),
            (
                "/index.ts",
                "export * from './leaf'; export const y = 2 as const;",
            ),
        ] {
            let _ = host
                .upsert(crate::UpsertRequest {
                    canonical_id: Some(canonical.into()),
                    input_id: canonical.into(),
                    source: Arc::from(source),
                    file_language: crate::FileLanguage::script_ts(),
                    aliases: vec![],
                })
                .unwrap();
        }
        if reason == Some(NonAdmissionReason::RetentionPressure) {
            let account = SemanticRetentionAccount::new(RetentionLimits {
                aggregate_ceiling_bytes: 1,
                max_entry_bytes: 1,
                pin_threshold_bytes: 1,
            });
            let mut routes = crate::resolver_core::route_db::RouteDb::new();
            routes.module_export_account = StoreAccount::new(account);
            host.resolver.runtime.routes = Arc::new(routes);
        }
        let view = host.resolver_store_view_read().into_owned_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let ctx =
            crate::resolver_core::HostResolverContext::<'_, verter_audit::observe::Uncaptured>::new(
                &host, &view, overlay,
            );
        let flight = host
            .build_module_export_tables(&ctx, &view, "/index.ts", || match reason {
                Some(NonAdmissionReason::MutationUnstable) => {
                    match SignatureAdmission::from_finalise(FactReadSetFinalise::MutationUnstable) {
                        SignatureAdmission::NonCacheable(reason) => Err(reason),
                        _ => panic!("mutation instability cannot admit"),
                    }
                }
                Some(NonAdmissionReason::UnresolvedProvenance) => {
                    match SignatureAdmission::from_finalise(FactReadSetFinalise::NonCacheable(
                        Arc::from([]),
                    )) {
                        SignatureAdmission::NonCacheable(reason) => Err(reason),
                        _ => panic!("non-cacheable provenance cannot admit"),
                    }
                }
                _ => Ok(()),
            })
            .unwrap();
        assert_eq!(flight.admission, reason.map_or(Ok(()), Err));
        assert_eq!(flight.table.bindings.len(), 2);
        for name in ["x", "y"] {
            assert!(matches!(
                flight.table.bindings.get(name),
                Some(Some(ModuleExportBinding::Value(_)))
            ));
        }
        assert_eq!(flight.table.retention.is_some(), reason.is_none());
        assert_eq!(
            host.resolver.runtime.routes.module_exports.len(),
            if reason.is_none() { 2 } else { 0 }
        );
    }
}
