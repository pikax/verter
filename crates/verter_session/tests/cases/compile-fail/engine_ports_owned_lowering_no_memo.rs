use verter_session::for_tests::{ExpressionSourceSelection, HostResolverContext};

fn cannot_escape(port: &HostResolverContext<'_>) {
    if let Some((_record, source)) = port.indexed_expression_source("/fixture.ts") {
        source.decl_bodies();
    }
}

fn main() {}
