use std::task::Waker;
use verter_session::carrier_publication_store::OwnedStableParse;

fn cannot_share(handoff: &OwnedStableParse, waker: &Waker) {
    handoff.poll(waker);
}

fn main() {}
