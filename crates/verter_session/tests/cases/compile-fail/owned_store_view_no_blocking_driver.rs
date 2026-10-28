use verter_session::StoreViewHandoff;

fn cannot_block(mut handoff: StoreViewHandoff) {
    handoff.wait();
}

fn main() {}
