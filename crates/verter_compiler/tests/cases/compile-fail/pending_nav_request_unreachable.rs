// Negative control for `PendingNavRequest`'s Vapor-private visibility.
//
// Which specific wall this fixture hits depends on how the harness is
// invoked — see `pending_nav_request_compile_fail.rs`'s own doc comment for
// the full mechanism (`trybuild`'s feature-detection walk, and why it
// succeeds under a live `cargo test --features bench` but not under the
// canonical gate's archived-and-directly-executed test binaries). Either
// way this fixture MUST NOT compile: under a live `--features bench` run,
// `template::code_gen::vapor` is NAMEABLE (module-level `pub` all the way
// down) and the wall is `PendingNavRequest`'s own item-level `pub(in
// crate::template::code_gen::vapor)` restriction (and, before that,
// `vapor::mod`'s private `use nav_request::PendingNavRequest;`, which no
// longer re-exports it at all); under the canonical gate, `template` stays
// `pub(crate)` and THAT is the wall instead. If this ever compiles under
// EITHER mechanism, ANY module anywhere in `verter_compiler` — this fixture
// stands in for a hypothetical future `ide`/-adjacent consumer, or any
// other sibling of `vapor` — could name and construct a `PendingNavRequest`
// directly, defeating the point of routing every access through
// `VaporElementState`'s opaque `PendingNavQueue` field.

fn main() {
    let _request: verter_compiler::template::code_gen::vapor::PendingNavRequest = todo!();
}
