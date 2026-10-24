use verter_workspace::resolution_handoff::{
    ResolutionProducer, ResolutionWork, WorkspaceResolution,
};
fn send<T: Send>() {}
fn main() {
    send::<WorkspaceResolution>();
    send::<ResolutionProducer>();
    send::<ResolutionWork>();
}
