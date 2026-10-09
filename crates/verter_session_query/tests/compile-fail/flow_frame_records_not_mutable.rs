// Sealed frame records cannot be rewritten in place: a parameter locator's
// ordinal, and a capture authority locator's frame and parameter ordinal,
// stay as the frame derived them.
use std::sync::Arc;
use verter_session_query::flow::slice::{
    CaptureParameterLocator, DefiningFrameGate, SliceCaptureAuthorityLocator,
};

fn parameter(locator: &mut CaptureParameterLocator) {
    locator.ordinal = 0;
}

fn authority(locator: &mut SliceCaptureAuthorityLocator, gate: Arc<DefiningFrameGate>) {
    locator.gate = gate;
    locator.parameter_ordinal = None;
}

fn main() {}
