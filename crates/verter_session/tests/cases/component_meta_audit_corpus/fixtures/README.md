# Vendored Vue corpus fixtures

The `.vue` files in this directory are vendored from
[`nuxt/ui`](https://github.com/nuxt/ui), specifically the runtime
component set under `src/runtime/components/`. They are used as
hermetic fixtures by the `corpus_audit_*` tests in
`crates/verter_session/tests/component_meta_audit_corpus/`.

Upstream license is MIT (see `LICENSE.md` in this directory). The
fixtures are reproduced verbatim and used for component-meta
resolution audit coverage; no code path in Verter consumes them at
runtime.

## Why vendored?

The default workspace test run (`cargo test --workspace --tests
--verbose`) must compile and pass on a fresh checkout. Earlier
revisions referenced `.integration-tests/repos/nuxt-ui/...`,
which was a sibling-clone the agent harness might or might not
provision. The vendoring removes that external dependency from the
default run; tests that genuinely need the live integration-tests
clone are gated behind the `external-corpus` Cargo feature.
