// A finished fragment exposes its length, policy, source and declared slots.
// Its pieces are read only by the writer and the map products inside the
// compiler; a consumer cannot reach them to rewrite their text or meaning.

use verter_compiler::assembly::emit_plan::{EmissionBuilder, MapMode};

fn main() {
    let mut builder = EmissionBuilder::new(MapMode::NoMap);
    builder.push_unmapped("const a = 1;");
    let fragment = builder.finish().unwrap();
    let _pieces = fragment.pieces();
}
