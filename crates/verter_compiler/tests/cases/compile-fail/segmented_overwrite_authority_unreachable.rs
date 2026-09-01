// Negative control for the segmented-overwrite call-site guard.
//
// This fixture belongs to the bench-feature standalone compile-contract
// runner invoked by `node scripts/compile-contracts.mjs`; it never enters
// the Rust test inventory. The runner enables `verter_compiler/bench`,
// reaching `SegmentedOverwriteAuthority::new()`'s own narrower
// `pub(in crate::template::code_gen)` restriction instead — the item-level
// claim this fixture was originally written to isolate.
//
// It MUST NOT compile. If it ever does, ANY code
// anywhere — this fixture stands in for `ide`/`svelte`/any future consumer
// of the public `CodeGenOutput` type — could mint an authority and call
// `overwrite_segmented` directly, which is exactly the false-provenance
// gap this token exists to close.

fn main() {
    let _authority =
        verter_compiler::template::code_gen::types::SegmentedOverwriteAuthority::new();
}
