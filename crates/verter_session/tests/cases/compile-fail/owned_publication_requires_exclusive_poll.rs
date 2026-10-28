use std::task::Waker;
use verter_session::carrier_publication_store::OwnedPublication;

fn cannot_share(handoff: &OwnedPublication, waker: &Waker) {
    handoff.poll(waker);
}

fn main() {}
