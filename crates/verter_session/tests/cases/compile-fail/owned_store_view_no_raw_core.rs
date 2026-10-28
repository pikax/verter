use verter_session::StoreViewHandoff;

fn cannot_extract(handoff: StoreViewHandoff) {
    let _ = handoff.core;
}

fn main() {}
