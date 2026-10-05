//! Selecting a source's expression-demand capability hands out an owned handle
//! over the source's retained state. A repeated selection — one per call
//! argument on the hot dispatch path — must allocate nothing beyond the
//! indexed serve it already performs: no per-selection capability box, for
//! either the expression or the flow source.

use std::sync::Arc;

use verter_session::for_tests::{
    with_host_resolver_context_for_tests, ExpressionSourceSelection, IndexedInputs,
};
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use super::{alloc_count, reset_alloc_counter};

const SOURCE: &str = "export function pick(a: number, b: string) {\n    return a > 0 ? a : b;\n}\n";

#[test]
fn repeated_expression_source_selection_allocates_no_capability() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = VerterHost::new(HostConfig::default(), workspace);
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/pick.ts".into()),
        input_id: "/pick.ts".into(),
        source: Arc::from(SOURCE),
        file_language: FileLanguage::script_ts(),
        aliases: vec![],
    });

    with_host_resolver_context_for_tests(&host, |ctx| {
        // Warm the indexed serve and the retained source.
        let primed = ExpressionSourceSelection::indexed_expression_source(ctx, "/pick.ts");
        assert!(
            primed.is_some(),
            "precondition: the source must be selectable"
        );
        drop(primed);

        reset_alloc_counter();
        let serve = IndexedInputs::ensure_indexed_ready_serve(ctx, "/pick.ts");
        let serve_allocations = alloc_count();
        assert!(serve.is_some(), "precondition: the warm serve must answer");
        drop(serve);

        for _ in 0..2 {
            reset_alloc_counter();
            let selected = ExpressionSourceSelection::indexed_expression_source(ctx, "/pick.ts");
            let selection_allocations = alloc_count();
            assert!(selected.is_some(), "a warm selection must answer");
            drop(selected);
            assert_eq!(
                selection_allocations, serve_allocations,
                "selecting the expression source allocated beyond its indexed serve \
                 ({selection_allocations} vs {serve_allocations}): the capability must be \
                 the owned handle, not a fresh allocation per selection"
            );

            reset_alloc_counter();
            let selected = ExpressionSourceSelection::indexed_flow_source(ctx, "/pick.ts");
            let selection_allocations = alloc_count();
            assert!(
                matches!(selected, Some((_, Some(_)))),
                "a warm flow-source selection must answer"
            );
            drop(selected);
            assert_eq!(
                selection_allocations, serve_allocations,
                "selecting the flow source allocated beyond its indexed serve \
                 ({selection_allocations} vs {serve_allocations})"
            );
        }
    });
}
