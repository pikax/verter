// Negative control for the segmented-overwrite call-site guard.
//
// This fixture belongs to the bench-feature group in
// `compiler_compile_fail.rs`. The canonical archive gate excludes the complete
// trybuild class. Under the supported live
// `cargo test -p verter_compiler --features bench --test main
// bench_compiler_compile_fail_contracts_are_enforced`, trybuild's probe DOES see `bench`
// (its fingerprint-directory lookup succeeds against a live, non-archived
// build), reaching `SegmentedOverwriteAuthority::new()`'s own narrower
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
