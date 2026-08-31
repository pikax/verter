// Negative control for `PendingNavRequest`'s Vapor-private visibility.
//
// This fixture belongs to the bench-feature group in
// `compiler_compile_fail.rs`. Run that group through a live
// `cargo test --features bench` invocation so trybuild can recover the parent
// feature set from Cargo's fingerprint metadata. The canonical archive gate
// excludes the complete trybuild class and does not execute this fixture.
// Under the supported live bench run,
// `template::code_gen::vapor` is NAMEABLE (module-level `pub` all the way
// down) and the wall is `PendingNavRequest`'s own item-level `pub(in
// crate::template::code_gen::vapor)` restriction (and, before that,
// `vapor::mod`'s private `use nav_request::PendingNavRequest;`, which no
// longer re-exports it at all). If this ever compiles, ANY module anywhere in
// `verter_compiler` — this fixture
// stands in for a hypothetical future `ide`/-adjacent consumer, or any
// other sibling of `vapor` — could name and construct a `PendingNavRequest`
// directly, defeating the point of routing every access through
// `VaporElementState`'s opaque `PendingNavQueue` field.

fn main() {
    let _request: verter_compiler::template::code_gen::vapor::PendingNavRequest = todo!();
}
