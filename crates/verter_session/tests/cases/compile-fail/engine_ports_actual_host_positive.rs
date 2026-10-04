use verter_session::for_tests::{Cancellation, ExecutionSubmission, FactValidation, HostResolverContext, IndexedInputs, OwnedLowering, RouteLookup};

fn implements_all<P: IndexedInputs + OwnedLowering + RouteLookup + FactValidation + Cancellation + ExecutionSubmission>() {}

fn owned_answers(ports: &HostResolverContext<'_>) {
    if let Some(record) = IndexedInputs::shallow_file_state(ports, "/fixture.ts") {
        let _whole_hash = record.whole_hash;
    }
    if let Some((record, source)) = OwnedLowering::indexed_expression_source(ports, "/fixture.ts") {
        drop(record);
        drop(source);
    }
    drop(RouteLookup::routed_shallow_state(ports, "/fixture.ts"));
    drop(FactValidation::aggregate_clock_reader(ports));
    let _checkpoint = Cancellation::cancellation_checkpoint(ports);
    drop(ExecutionSubmission::attach_engine(ports));
}

fn main() { implements_all::<HostResolverContext<'static>>(); }
