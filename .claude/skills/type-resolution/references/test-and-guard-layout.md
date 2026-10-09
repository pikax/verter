## Frontier Engine Tests

Tests in `crates/verter_session/src/frontier_tests.rs` cover diamond dedup, barrel ordering, cycle termination, budget enforcement, export routing, and store-view consistency. Run with `cargo test --package verter_session frontier_tests`.

## Responsibility suites

Session component-meta tests live in `src/tests/meta/`, and host management
tests in `src/tests/host_manage/`. Their existing module identities remain
`meta::meta_tests` and `host_manage::tests`; each adds scenario modules below
that prefix. Shared fixture helpers stay in the suite index.

Pure graph-store tests live with `verter_type_engine` under
`semantic_query_memo::tests::substrate`. Task wait-graph tests live with
`verter_execution::tasks::tests::wait_cycles`. Dispatch, flow-return, and
null-policy tests that construct a host, parse source, or compose workspace
inputs remain in session. The engine has no session/source dev-dependency.

Session architecture guards live under `tests/cases/architecture/`, wired
through the same `tests/main.rs`. The dependency, capability, cache, lifecycle,
hermeticity, production-feature, and mapping families keep shared syntax
predicates in `support.rs` or the foundation helper module. Cross-crate
dependency and capability checks live in
`verter_source_policy_gate/tests/cases/architecture_dependencies.rs`; its
existing production dependency-closure suite remains the authority for renamed
and target-specific dependency edges. Compile contracts retain their fixtures.

Old source comments naming the former architecture monolith refer to these
family files and `support.rs`. Content-reading guards must read the predicate's
current owner, and scanner self-exclusions must use the exact relocated
guard-file paths rather than excluding unrelated files by basename.

## Discovery preservation

`tests/test-layout/relocations.json` records old and new nextest identities for
relocated tests. It contains names, not execution receipts or checkout pins.
Capture independent before/after discovery with
`cargo nextest list --workspace --message-format json`, then compare both
inventories with:

```sh
node scripts/check-test-relocation.mjs BEFORE.json AFTER.json tests/test-layout/relocations.json
```

The comparison preserves all unchanged tests and ignored dispositions and
rejects missing, additional, duplicate, or stale identities. Its discriminator
tests run in the script-test CI lane. Full runtime verification belongs to the
canonical gate; package-scoped runs provide focused local evidence.

Allocation canaries stay in exact process-isolation exceptions at their actual
owners: session cache/source composition, engine flow/signatures, type-expression
absolutization, query receipt summaries, and compiler styles. Ordinary integration
cases use `tests/cases/` under one `tests/main.rs`; a historical fixture filename
does not require another binary.

Compiler compile, Svelte client, IDE template, and SSR tests use child scenario
modules under their existing test indices. LSP server suites live in
`src/server/tests/`; real-provider exact selectors include the scenario module.
Architecture inventories bind current importer files and compiled witness paths;
test-only moves do not change production feature closures or native/WASM APIs.
