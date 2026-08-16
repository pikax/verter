// Negative control for the segmented-overwrite call-site guard.
//
// The OUTER test crate (`verter_compiler`'s own `--test main`) always runs
// with the `bench` feature active under the canonical gate (workspace
// feature unification: `verter_bench` depends on `verter_compiler` with
// `features = ["bench"]`, and that's a normal, not dev, dependency, so it
// unifies across every `--workspace` build). Whether trybuild's OWN nested
// probe crate ALSO sees `bench` active is a SEPARATE question depending on
// how the harness is invoked — see `pending_nav_request_compile_fail.rs`'s
// doc comment for the full mechanism. Under the canonical gate (an ARCHIVED,
// then directly-executed test binary), it does not, so `template` stays
// `pub(crate)` here and this fixture hits that wall FIRST — proving the
// coarser "this whole subtree is crate-private" claim. Under a live
// `cargo test -p verter_compiler --features bench --test main
// segmented_overwrite_compile_fail`, trybuild's probe DOES see `bench`
// (its fingerprint-directory lookup succeeds against a live, non-archived
// build), reaching `SegmentedOverwriteAuthority::new()`'s own narrower
// `pub(in crate::template::code_gen)` restriction instead — the item-level
// claim this fixture was originally written to isolate.
//
// It MUST NOT compile, under either mechanism. If it ever does, ANY code
// anywhere — this fixture stands in for `ide`/`svelte`/any future consumer
// of the public `CodeGenOutput` type — could mint an authority and call
// `overwrite_segmented` directly, which is exactly the false-provenance
// gap this token exists to close.

fn main() {
    let _authority =
        verter_compiler::template::code_gen::types::SegmentedOverwriteAuthority::new();
}
