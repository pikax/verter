# Recursive / mutually-recursive `async fn` — negative finding

## Symptom

Unboxed recursive `async fn` (direct or mutual) grows the future frame
**per recursion level** and is a latent stack-overflow class — the same
failure mode as an unbounded recursive classifier, but through async
state machines.

This investigation searched for that class on the LSP / type-runtime /
session async surfaces relevant to the serve path.

## Mechanism (what was searched)

- Production `async fn` definitions under `crates/verter_lsp/src` and
  `crates/verter_type_runtime/src` (excluding `*_tests.rs` / test modules).
- Patterns: self-call by name across an `.await`, mutual pairs, and
  unboxed recursive `async` blocks.
- Contrast with intentional `Box::pin` at the `TypeProvider` boundary
  (`ProviderFuture = Pin<Box<dyn Future + Send>>` in
  `verter_type_runtime::traits`).

## Reproduction

Static search only (no runtime recursion probe):

```text
rg -n "async fn" crates/verter_lsp/src crates/verter_type_runtime/src --glob "*.rs"
# manual review of large handlers + transports
# no production self-await of the same async fn found
```

## Evidence

| area | result |
|---|---|
| LSP handlers (`handle_*`, `LanguageServer` methods) | sequential awaits; no self-recursion |
| `ensure_*_synced` family | nested calls, not recursive re-entry of the same async fn |
| `goto_definition` nav-probe loop | **loop** of `get_definition` awaits — iterative, not recursive async |
| `TypeProvider` impls (tsgo/tsserver/mock/resilient) | methods return `Box::pin(async move { … })` — heap, non-recursive |
| Completion-detail enrichment | `JoinSet` + semaphore (spawned tasks), not recursive async |
| `verter_scheduler` | **no** `async fn` in production surface |
| `join_all` / unboxed concurrent collections of large futures | **tests only** in `verter_lsp` |

**Conclusion:** no unboxed recursive or mutually-recursive `async fn` was
found on the LSP serve path or type-provider transports. This is a
**recorded negative** so the next investigator does not re-hunt the same
class without new code.

(Semantic-engine recursion is a different class — sync call stacks / graph
walks — and is out of scope here; peer docs on connected-depth budgets cover
that space.)

## Project-wide extension

The same search was extended to every non-test production module under
`crates/` with an async surface:

| area | result |
|---|---|
| `verter_mcp` tools | no self-await; await-free sync bodies behind `Box::pin` |
| `verter_tsgo_api` (actor, jsonrpc, relay pumps, attach) | loops / select, not recursive async |
| `verter_relay_shim` | select/setup loops; no recursive async fn |
| `verter_napi` VFS async | no await of self |
| `verter_dx_baseline` dispatch | sequential `handle` → one request; no recursion |
| `verter_scheduler` / live `verter_session` host / `verter_wasm` / `verter_compiler` | no `async fn` |
| Semantic `reduce_awaited` | **sync** Promise unwrap — not Rust async recursion |

**Conclusion (workspace):** no unboxed recursive `async fn` class found
anywhere in production. Recorded so the next pass does not re-scan without
new code.

## Why deferred

Nothing to fix. Keep the negative on file.

## Proposed fix + falsifiable prediction

If a future change introduces recursive async, the standard remedy is
`Box::pin` (or an explicit heap stack) at the recursive edge.

**Prediction:** a regression test that constructs an unboxed recursive
async future of depth `N` and asserts `size_of_val` grows with `N` would
fail closed if such a function is reintroduced without boxing.

## Blast radius

None today. Leaving the negative undocumented would waste the next pass.
