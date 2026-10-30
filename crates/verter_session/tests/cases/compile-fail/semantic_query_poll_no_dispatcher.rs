//! The polled entry and its task expose no dispatcher, so a poller cannot
//! reach the resolver capabilities and their synchronous adapters.
use verter_type_engine::project_semantic_dispatch::{SemanticQueryPoll, SemanticTask};
use verter_type_engine::resolver_core::ResolverCapabilities;

fn entry_dispatcher<C: ResolverCapabilities>(entry: &SemanticQueryPoll<'_, '_, C>) {
    let _ = entry.lender;
}

fn task_dispatcher<C: ResolverCapabilities>(task: &SemanticTask<'_, '_, C>) {
    let _ = &task.program;
}

fn main() {}
