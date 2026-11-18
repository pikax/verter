//! A target executor is chosen once per request and called statically: the
//! executor trait cannot become a trait object, so no node loop can
//! dispatch through one.

use verter_compiler::target_plan::TargetExecutor;

fn dispatch_per_node(_executor: &dyn TargetExecutor) {}

fn main() {}
