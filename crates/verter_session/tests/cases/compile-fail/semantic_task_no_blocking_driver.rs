//! A polled semantic task has no blocking driver: polling is its only way
//! forward.
use verter_type_engine::project_semantic_dispatch::SemanticTask;
use verter_type_engine::resolver_core::ResolverCapabilities;

fn cannot_block<C: ResolverCapabilities>(task: &mut SemanticTask<'_, '_, C>) {
    let _ = task.drive();
}

fn main() {}
