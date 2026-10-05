// Frame and capture records are sealed: a frame is built only from a
// checked discovery, and a capture chain and nested context only from a
// frame at a child position. Literals assembling them from unrelated
// parts do not compile.
use std::sync::Arc;
use verter_session_query::flow::slice::{
    CaptureScope, CapturedFrame, DefiningFrameGate, NestedFlowContext,
};

fn frame(gate: &DefiningFrameGate) -> DefiningFrameGate {
    DefiningFrameGate {
        skeleton: Arc::clone(gate.skeleton()),
        ..gate.clone()
    }
}

fn chain(gate: Arc<DefiningFrameGate>, scope: &CaptureScope) -> CaptureScope {
    CaptureScope {
        enclosing: Some(Arc::new(CapturedFrame {
            gate,
            region: scope.enclosing().unwrap().region(),
        })),
    }
}

fn context(captures: CaptureScope) -> NestedFlowContext {
    NestedFlowContext {
        captures,
        this: None,
    }
}

fn main() {}
