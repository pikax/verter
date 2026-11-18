// A segmented overwrite takes a `SegmentedCorrespondence`, which only the
// transform's own check mints after validating every anchor against the
// source. A struct literal must not compile: a forged correspondence would
// let any producer claim exact bytes the source does not hold.

fn main() {
    let _forged = verter_compiler::code_transform::SegmentedCorrespondence {
        content: "_ctx.foo",
        anchors: &[],
        checked_against: "{{ bar }}",
    };
}
