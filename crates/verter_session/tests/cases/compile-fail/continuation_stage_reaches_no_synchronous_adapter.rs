//! A stage capability reaches no dispatcher, so a continuation's stage can
//! neither read another query, read or load a file, nor drive an execution:
//! every such dependency is a step its driver answers.
use verter_type_engine::project_semantic_dispatch::ContinuationStage;
use verter_type_engine::resolver_core::ResolverCapabilities;

fn stage<C: ResolverCapabilities>(cx: &ContinuationStage<'_, '_, C>) {
    let _ = &cx.dispatch;
    let _ = cx.dispatch().execute_read(());
    let _ = cx.execute_read(());
    let _ = cx.ensure_indexed_ready_serve("/a.ts");
}

fn main() {}
