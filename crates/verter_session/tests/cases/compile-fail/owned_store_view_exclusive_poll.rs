use verter_session::StoreViewHandoff;

fn cannot_share(handoff: &StoreViewHandoff) {
    handoff.poll(std::task::Waker::noop());
}

fn main() {}
