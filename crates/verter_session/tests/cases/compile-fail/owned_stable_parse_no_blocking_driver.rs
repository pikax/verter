use verter_session::carrier_publication_store::OwnedStableParse;

fn cannot_block(mut handoff: OwnedStableParse) {
    handoff.wait();
}

fn main() {}
