use verter_session::carrier_publication_store::OwnedPublication;

fn cannot_block(mut handoff: OwnedPublication) {
    handoff.wait();
}

fn main() {}
