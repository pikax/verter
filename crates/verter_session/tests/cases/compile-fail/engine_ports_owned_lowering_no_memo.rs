use verter_type_engine::resolver_core::request_ports::ExpressionSourceSelection;
use verter_session::for_tests::HostResolverContext;

fn cannot_escape(port: &HostResolverContext<'_>) {
    if let Some((_record, source)) = port.indexed_expression_source("/fixture.ts") {
        source.decl_bodies();
    }
}

fn main() {}
