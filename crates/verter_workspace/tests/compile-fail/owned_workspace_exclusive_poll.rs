use std::task::Waker;
use verter_workspace::resolution_handoff::WorkspaceResolution;
fn poll(handoff: &WorkspaceResolution, waker: &Waker) {
    let _ = handoff.poll(waker);
}
fn main() {}
