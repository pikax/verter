use verter_type_engine::resolver_core::request_ports::{ExecutionSubmission, ExpressionSourceSelection, HostAttachmentPort, IndexedInputs, OwnedLowering, RouteLookup};
use verter_type_engine::resolver_core::fact_validation_port::{FactValidation, LiveFactValidation};
use verter_session::for_tests::HostResolverContext;

fn implements_all<P: IndexedInputs + OwnedLowering + ExpressionSourceSelection + RouteLookup + FactValidation + LiveFactValidation + ExecutionSubmission + HostAttachmentPort>() {}

fn owned_answers(ports: &HostResolverContext<'_>) {
    if let Some(record) = IndexedInputs::shallow_file_state(ports, "/fixture.ts") {
        let _whole_hash = record.whole_hash;
    }
    if let Some((record, source)) = ExpressionSourceSelection::indexed_expression_source(ports, "/fixture.ts") {
        drop(record);
        drop(source);
    }
    drop(RouteLookup::routed_shallow_state(ports, "/fixture.ts"));
    let _clocks = LiveFactValidation::request_snapshot(ports).clocks().live();
    let _checkpoint = FactValidation::request_flags(ports).cancellation_checkpoint();
    drop(ExecutionSubmission::attach_engine(ports));
    let _attachment = HostAttachmentPort::host_attachment(ports);
}

fn main() { implements_all::<HostResolverContext<'static>>(); }
