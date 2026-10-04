use verter_session::for_tests::RouteLookup;

fn cannot_escape<P: RouteLookup + ?Sized>(port: &P) {
    if let Some(record) = port.routed_shallow_state("/fixture.ts") {
        record.decl_bodies();
    }
}

fn main() {}
