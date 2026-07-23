# Architecture improvements - Codex review (`perf/consolidation` vs `main`)

**Status:** Open architecture backlog.

**Verdict:** The branch is moving from a good architecture toward the best
direction for Verter. Do not revert to `main`'s simpler but duplicated model.
The current tree is not merge-ready until the correctness and ownership issues
below are closed.

## Preserve the direction

- Keep one query-time type-resolution engine:
  `ProjectSemanticDispatch` plus `SemanticGraphStore`.
- Keep shallow indexing and lazy declaration-body resolution.
- Keep fact-based cache validity and `ProjectTypeStore` as the sole owner of
  query identity and type-result caches.
- Keep external TypeScript access project-bound through `BoundProject`.
- Keep framework adapters limited to planning and normalization; they must not
  become alternate resolvers.
- Keep virtual-to-source mapping fail-closed for locations and edits.
- Preserve the parser -> compiler -> semantic -> session -> workspace/language
  dependency direction.

## Required improvements

### P0 - Correct tsserver barrel compatibility policy

`crates/verter_lsp/src/server/import_publication.rs` currently rewrites carrier
barrel imports to explicit `.vue.verter.ts` specifiers when
`allowImportingTsExtensions` is false. TypeScript rejects those specifiers with
TS5097 in exactly that configuration.

Direction:

- Preserve authored carrier specifiers for tsserver plugin routes.
- If a compatibility projection genuinely requires an emitted-style suffix,
  use the JavaScript carrier suffix rather than `.ts`.
- Derive compiler options from the resolved configured owner or bound project,
  never `nearest_config_for_path`, which is proximity rather than ownership.
- Add a real TypeScript-provider diagnostic test with
  `allowImportingTsExtensions: false`; mock call-count tests are insufficient.

Acceptance: a default tsconfig produces no TS5097 for published carrier barrel
imports.

### P1 - Make workspace-symbol completeness provider-owned

`crates/verter_lsp/src/server/provider_state.rs` uses carrier-publication state
as a general readiness authority. That is valid only when the active provider
actually consumes that publication graph. In particular, TSGO has its own
project-input frontier, and multi-claimant references can use the deterministic
default owner even though rename must remain fail-closed.

Direction:

- Introduce a provider-specific `WorkspaceSymbolCompleteness` capability.
- Let tsserver consult its plugin/store project frontier.
- Let TSGO consult explicit project-input acknowledgement from TSGO.
- Use `default_configured_owner_for_file` for multi-claimant references.
- Keep rename fail-closed until cross-project rename fanout exists.
- Gate request-local safety, not completion of a background workspace scan.

Add tests for multi-claimant references and for TSGO requests whose provider
graph is complete while the carrier publication store is incomplete.

### P1 - Finish ownership cutovers and delete compatibility re-exports

The extraction introduced the right owners, but active compatibility modules
still expose two paths to the same concepts:

- `verter_lsp::type_provider::{protocol,traits,tsgo::ipc,tsserver::ipc}`
  re-export `verter_type_runtime`.
- `verter_semantic::analysis::project_resolver` re-exports
  `verter_workspace`.
- `verter_lsp::project_resolver` re-exports the semantic re-export.

Direction:

- Import `verter_type_runtime` directly from LSP consumers, then delete the LSP
  compatibility modules.
- Import resolver authority directly from `verter_workspace`.
- Keep only semantic helpers that genuinely depend on semantic model types,
  without re-exporting workspace ownership.
- Delete the LSP project-resolver shim in the same cutover.
- Enforce the dependency boundary structurally, not with identifier-name scans.

### P1 - Complete framework cache and wire ownership

The framework architecture still records two explicit debts:

- `FrameworkSurfaceStore` and `FrameworkScriptCaches` live on registry rows
  instead of the single project type store and do not provide true
  singleflight.
- The framework wire payload remains provisional.

Direction:

- Move framework query caches into `ProjectTypeStore` /
  `TypeInfoGraphResultDb`, using the same fact rail, invalidation dimensions,
  and singleflight mechanism as native semantic queries.
- Complete the wire retag, bump the schema version, and reserve the retired
  field identifiers.
- Do not describe the framework architecture as final until both debts close.

### P1 - Keep integration tests consolidated

The working-tree `crates/verter_lsp/tests/client_lifetime.rs` adds another
top-level integration binary without a process-global isolation requirement.
That violates the repository's consolidated integration-test layout.

Direction:

- Move it under `crates/verter_lsp/tests/cases/`.
- Register it through the existing consolidated test entry point.
- Do not add it to the exception allowlist.

### P2 - Publish prop semantics from the semantic owner

`crates/verter_lsp/src/documents/analysis.rs` contains a local
`type_expr_contains_boolean` walker. It recognizes a subset of `TypeExpr` and
can disagree with shallow aliases, imported references, or future type forms.
The result affects completion syntax and lint behavior.

Direction:

- Produce a positional prop-kind or boolean semantic fact from the shared
  session/component-meta result.
- Let LSP merge facts and spans only; it should not reinterpret type
  expressions.
- Test direct boolean, literal/union, imported alias, generic, and unknown
  cases, with unknowns failing closed.

### P2 - Reduce session complexity without creating another engine

The branch grows from 18 to 37 crates, while `verter_session` remains both large
and central. Further decomposition should clarify ownership rather than add
another query path.

Direction:

- Split large production modules around sealed capabilities and private result
  databases.
- Keep all query identity and result caches on `ProjectTypeStore`.
- Create a crate only when it establishes a real dependency firewall and has
  more than one credible consumer; otherwise prefer an internal module.
- Add no second resolver, compatibility database, or parallel cache owner.

### P2 - Keep protocol crates transport-only

`crates/verter_protocol/Cargo.toml` has a regular dependency on
`verter_semantic`, but protocol source does not require semantic
implementation types.

Direction:

- Remove the dependency if the full workspace build confirms it is unused.
- Keep conversions in adapter, FFI, or session boundaries so the protocol
  schema remains a low-level transport contract.

### P2 - Make verification prove execution

The documented verification-inventory gate is not yet self-proving.

Direction:

- Derive the expected inventory from the tree.
- Add mutation-style negative controls showing the gate fails when a test
  target is omitted.
- Promote the gate only after those controls pass in CI.

### P3 - Give generic process containment a neutral owner

`ClientProcessGuard` is general client-process lifecycle infrastructure but
currently lives in the TSGO API crate.

Direction:

- If it gains another consumer, extract the generic containment primitive to a
  neutral low-level owner.
- Otherwise document that the TSGO crate deliberately owns this process
  substrate and keep provider-specific discovery and protocol logic separate.

## Product boundaries to state explicitly

- Do not claim full TypeScript checker parity.
- Keep unsupported Svelte SSR/hydration behavior fail-closed.
- Keep multi-claimant rename fail-closed until fanout is implemented.
- Require real-provider, cross-region tests before treating carrier IDE
  publication as complete.

## Anti-goals

- Reintroducing a second type resolver.
- Letting framework adapters own independent semantic caches.
- Using path proximity as project ownership.
- Making request readiness depend on unrelated background discovery.
- Keeping migration shims indefinitely.
- Splitting crates solely to reduce file size.

## Suggested execution order

1. Fix barrel publication correctness and consolidate the new integration test.
2. Introduce provider-owned workspace-symbol completeness.
3. Delete compatibility re-export paths.
4. Close framework cache and wire-format debts.
5. Move prop classification to shared semantic facts.
6. Remove the protocol dependency and complete verification guardrails.

## Review evidence

- Branch base: current `main`; branch is 485 commits ahead.
- Tracked diff: roughly 11,569 files and 2.06 million added lines.
- Crates: 37 on this branch versus 18 on `main`.
- `cargo check -p verter_lsp -p verter_tsgo_api -p verter_workspace` passed.
- Existing mock barrel-resolution tests passed but do not exercise TS5097.
- `node scripts/check-integration-test-layout.mjs` rejects the new top-level
  `client_lifetime` test, as intended.
- The full canonical Rust verification pair was not run for this review.
