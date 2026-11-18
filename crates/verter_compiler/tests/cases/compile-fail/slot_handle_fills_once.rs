// A slot handle is consumed by its fill, so filling one slot twice does not
// compile.

use verter_compiler::assembly::emit_plan::{EmitPlanBuilder, MapMode};
use verter_compiler::assembly::SyntacticContract;

fn main() {
    let mut builder = EmitPlanBuilder::new(MapMode::NoMap);
    let handle = builder.reserve_slot(SyntacticContract::Expression);
    builder.fill_empty(handle).unwrap();
    builder.fill_empty(handle).unwrap();
}
