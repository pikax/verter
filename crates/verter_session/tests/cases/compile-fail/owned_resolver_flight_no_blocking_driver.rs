use verter_session::resolver_core::OwnedResolverFlight;

fn cannot_block(mut handoff: OwnedResolverFlight<String, usize, ()>) {
    handoff.wait();
}

fn main() {}
