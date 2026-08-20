// `assemble_sequence` takes `&[&ValidatedFragment]` exclusively — there is
// no raw `{code, source_map}` overload a caller could reach for instead.
// This is a compile-time proof, not a documented convention: constructing
// something that merely LOOKS like a sequenced fragment and handing it to
// `assemble_sequence` must fail to compile.

use verter_compiler::assembly::assemble_sequence;

struct LooksLikeAFragment<'a> {
    code: &'a str,
    source_map: Option<&'a str>,
}

fn main() {
    let raw = LooksLikeAFragment {
        code: "const x = 1",
        source_map: None,
    };
    let _ = assemble_sequence(&[&raw], None);
}
