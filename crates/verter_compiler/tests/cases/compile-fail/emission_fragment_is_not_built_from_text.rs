// An emission fragment carries what each of its bytes means, so it cannot be
// made from finished text: there is no conversion from a `String` and no
// struct literal outside the emission owner. A producer builds one through
// `EmissionBuilder` or a transform's freeze.

use verter_compiler::assembly::emit_plan::{EmissionFragment, MapMode};

fn main() {
    let _from_text: EmissionFragment<'static> = String::from("const a = 1;").into();
    let _literal = EmissionFragment {
        len: 0,
        mode: MapMode::NoMap,
    };
}
