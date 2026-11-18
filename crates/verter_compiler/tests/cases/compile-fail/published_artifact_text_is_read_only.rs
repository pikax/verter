// Written artifact text is final. It is read through `text()`; there is no
// mutable access, no public field, and no way to take the text apart from its
// map outside assembly.

use verter_audit::observe::Uncaptured;
use verter_compiler::assembly::emit_plan::{EmitPlanBuilder, MapMode};
use verter_compiler::work_ledger::CompilerWorkLedger;

fn main() {
    let mut builder = EmitPlanBuilder::new(MapMode::NoMap);
    builder.push_unmapped("const a = 1;");
    let plan = builder.finish().unwrap();
    let mut artifact = plan
        .write(&mut CompilerWorkLedger::<Uncaptured>::new())
        .unwrap();
    artifact.text_mut().push_str("// appended");
    artifact.text.push_str("// appended");
    let (_text, _map) = artifact.into_parts();
}
