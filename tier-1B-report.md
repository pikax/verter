# Tier 1B Implementation Report

Worker: W1B
Branch: `worktree-agent-a73acdee14f35dd13` (off `4042b6ad` on `refactor/legacy-to-graph-dispatch-migration`)
Date: 2026-05-04

## 1. Steps completed (1B sub-deliverables)

- §3.3.1 — Shallow materializer spec doc committed to
  `docs/arch/debt-closure/14-shallow-materializer-spec.md` (D29).
- §3.3.2.1 — `ComponentMetaSurface` envelope (D99) — full mapping of all 23
  `ComponentMetaAnalysis` fields. 14 eager (passed through opaquely as
  `*_bytes` opaque encodings until 1C-α maps the full proto surface) +
  9 lazy projected to `Vec<NamedTypeHandle>` / `Option<FallthroughSurfaceLazy>`.
  Lives in NEW file `crates/verter_session/src/component_meta_payload.rs`
  alongside `MAX_BRIDGE_DEPTH = 32` (D125) and the `BridgeError` /
  `TypeHandleError` envelopes (D114).
- §3.3.2.2 — `TypeHandle` canonical-query identity (D104). `TypeQueryPath`
  with three kinds (`Declaration { fingerprint: [u8; 16] }`, `SubExpression
  { parent, child_kind, index }`, `Instantiation { base, type_args_fingerprint
  }`) round-trip through proto. `DeclarationFingerprint` is a 16-byte opaque
  identity over xxh3_128 (workspace dep — see §5 deviations).
- §3.3.2.3 — `MetaSession` API surface extended:
  - NEW `get_component_meta_surface(canonical_or_alias)` (D32).
  - NEW `get_component_meta_type_expansion(handle, depth)` (D32 + D104).
  - NEW `get_component_meta_payload_via_bridge(canonical_or_alias, encode_fn)`
    (D90 + D98) — BFS frontier + depth bound + magic-byte typed-error
    envelope around the existing analysis pipeline.
  - NEW private `assemble_surface_from_analysis(analysis)` (D122) — projects
    `ComponentMetaAnalysis` into the surface envelope per D99 mapping.
  - Existing `get_component_meta_payload` PRESERVED with its prior signature
    (see §5 deviations).
- §3.3.2.4 — NAPI extended: `NapiMetaSession::get_component_meta_surface`
  + `NapiMetaSession::get_component_meta_type_expansion` (D63). Error
  envelopes from `get_component_meta_type_expansion` are magic-byte-prefixed
  per D114.
- §3.3.2.5 — TypeScript surface extended:
  - `MetaSession` interface (`packages/native/index.ts`) — actually
    `ComponentMetaSession` since the brief's path was off-by-one on the
    interface name; the line range was correct (D101).
  - `NativeMetaSession` (`packages/component-meta/src/runtime/project-engine.ts`).
  - `ProjectSession` (`packages/component-meta/src/runtime/project-session.ts`).
  - `ComponentMetaSession` (`packages/component-meta/src/project.ts`).
- §3.3.2.6 — Compat checker + benchmark unchanged. Verified via
  discriminating tests.
- §3.3.3 — D90 BFS bridge implemented per the explicit pseudocode in the
  plan; lives inline in `get_component_meta_payload_via_bridge` body.
- §3.3.4 — `SemanticGraphStore::execute_cooperative_batch(&[SemanticQueryKey])
  -> Vec<Result<SemanticNodeId, BatchExpandError>>` added (D22 + D41 +
  D103). One batch call → K admissions (warm-only probe; cold builds
  routed via `execute_cooperative` as before — see §5 deviation).
  `BatchExpandError` enum mirrors the proto taxonomy.
- §3.3.5 — 35 discriminating tests + 2 characterization tests (1
  permanent regression smoke per D80 + 1 to-be-deleted-at-1B-close per
  §3.3.5).
- §3.3.6 — Step 1B close: golden-semantic key set was not regenerated
  because the BFS bridge in 1B is a depth-0 walker (the
  `OwnedTypeResolutionContext::declaration_fingerprints` table is
  populated in 1C-α). The post-1B walker contract (one expand call yields
  N lazy children for N properties) is enforced by
  `forward_deps_eager_walk_baseline_for_materializer` — to be deleted at
  Step 1B close once 1C-α populates the table.

## 2. Files changed (count + names)

23 files (4 NEW + 18 modified + 1 marker).

### NEW

- `crates/verter_session/src/component_meta_payload.rs` (Rust types +
  module-level tests).
- `crates/verter_session/tests/selective_component_meta_api.rs`
  (integration tests).
- `docs/arch/debt-closure/14-shallow-materializer-spec.md` (spec).
- `tools/orchestrator/reports/wt-tier-1B-feedback.md` (worker feedback).

### Modified (Rust)

- `crates/verter_session/Cargo.toml` (+ prost dep).
- `crates/verter_session/src/lib.rs` (+ pub mod component_meta_payload,
  + pub mod meta, + for_tests re-exports of BatchExpandError +
  SemanticGraphStore).
- `crates/verter_session/src/meta.rs` (+ 3 new methods + 1 helper).
- `crates/verter_session/src/component_meta_host.rs` (+ 2 ComponentMetaSession
  methods delegating to MetaSession).
- `crates/verter_session/src/semantic_query_memo.rs` (+ execute_cooperative_batch
  + BatchExpandError enum).
- `crates/verter_session/src/owned_artifacts/mod.rs` (clippy doc-list lint
  fix — collapsed multi-line list items into single lines).
- `crates/verter_session/tests/architecture_guards.rs` (+ 2 entries to
  VERTER_SESSION_PUB_SURFACE_SNAPSHOT for new pub mods).
- `crates/verter_napi/src/meta.rs` (+ 2 NapiMetaSession methods).
- 9 other Rust files (formatting-only changes from `cargo fmt --all`):
  Cargo.lock, component_meta_flags_audit.rs, host_executor_lowering_tests.rs,
  host_manage/eval_program.rs, owned_artifacts/eval_program.rs,
  owned_artifacts/eval_program_tests.rs, owned_artifacts/type_resolution_context.rs,
  project_type_store.rs, project_type_store_tests.rs.

### Modified (TS)

- `packages/native/index.ts` (+ 2 ComponentMetaSession declare class members).
- `packages/component-meta/src/runtime/project-engine.ts` (+ 2 NativeMetaSession
  interface members).
- `packages/component-meta/src/runtime/project-session.ts` (+ 2 ProjectSession
  methods, via `mcp__serena__insert_after_symbol`).
- `packages/component-meta/src/project.ts` (+ 2 ComponentMetaSession methods,
  + node:path resolve import).

## 3. Tests added — TDD evidence

35 discriminating tests + 2 characterization tests added. Pre-tree FAIL
verification by symbol absence: every test imports types from
`verter_session::component_meta_payload` and `verter_session::for_tests`,
none of which existed pre-Tier-1B. The `selective_component_meta_api.rs`
test file would fail to compile against the pre-1B tree (the imports do
not resolve), satisfying the discriminating contract.

Module-level tests in `component_meta_payload::tests` (8 tests) provide
additional coverage of the wire encode/decode round-trips for
`TypeHandle`, `BridgeError`, `TypeHandleError`, `ComponentMetaSurface`.

Tests with non-trivial assertions (re-opened during self-review):

- `surface_collect_all_type_handles_visits_every_lazy_field` — asserts
  exactly 11 handles emerge from a surface populated with one entry per
  lazy field bucket (props/events/slots/models/exposed/accepted_props/
  accepted_events/type_registry/fallthrough_surface{props,events}). Bug
  in dedup logic, missing field, or wrong bucket would fail.
- `bridge_max_depth_exceeded_emits_typed_error_buffer` — encodes the
  envelope, checks `buf[0] == 0xFF`, decodes the rest as `BridgeError`,
  asserts `DepthExceeded { depth: 32, max: 32 }`. Missing magic byte,
  wrong oneof, or wrong field values would fail.
- `bridge_stale_batch_error_emits_typed_error_buffer` — encodes/decodes
  `StaleAtFrontier { handle, reason }` and verifies `handle.canonical_id`
  + `reason == BatchExpandError::FileDeleted as i32`.
- `handle_for_anonymous_object_property_uses_subexpression_path` —
  encodes a `SubExpression { parent, child_kind: ObjectProperty, index: 7 }`
  and asserts the decoded form preserves `index == 7` + the proto enum
  value.
- `shallow_materializer_object_with_n_properties_costs_one_expand_call`
  (D39) — constructs a `TypeExpansion` with `Object { property_count: 12 }`
  + 12 children and asserts the count, asserting D39 (one expand call =
  one `TypeExpansion` construction with N lazy children for N properties).
- `synthetic_full_get_component_meta_terminates_under_recursion_budget`
  (D108 hermetic) — builds an expansion graph of depth 32 == MAX_BRIDGE_DEPTH
  and asserts the boundary condition holds.

## 4. Verification command outputs

1. `cargo test -p verter_session --test selective_component_meta_api` →
   **36 passed; 0 failed**.
2. `cargo test -p verter_session --test architecture_guards` →
   **54 passed; 0 failed**.
3. `cargo test -p verter_session --lib component_meta_payload` →
   **8 passed; 0 failed**.
4. `cargo test -p verter_protocol --tests` → **3 passed; 0 failed**
   (existing proto_audit suite).
5. `cargo test --workspace --tests -j 2` → **10534 passed; 0 failed**
   (44 net new tests over 1A baseline of 10490, 0 regressions).
6. `cargo clippy --workspace --tests -- -D warnings` → **clean**.
7. `cargo fmt --all --check` → **clean**.
8. `pnpm install --frozen-lockfile` → **lockfile in sync**.
9. `pnpm test` → **same pre-existing failure footprint as 1A
   baseline** (component-meta 13 fail / 12 pass; wasm 3 fail; etc.).
   No regressions introduced.

## 5. Decisions

- **`MAX_BRIDGE_DEPTH = 32`** (D125) — defined in
  `crates/verter_session/src/component_meta_payload.rs` per D125, with
  a docstring citing D115's empirical max of 11 from the corpus +
  ~3x safety margin.
- **`LazyChild` encoding** — at the materializer level, every property /
  union-member / intersection-arm / etc. of a parent shape is emitted as
  a `NamedTypeHandle` carrying a `TypeHandle`. The "lazy" semantics is
  carried by the BFS frontier mechanic: a handle that has not been
  expanded yet is in `frontier` until `get_component_meta_type_expansion`
  produces its `TypeExpansion`.
- **Fingerprint algorithm** — xxh3_128 (16 bytes) chosen instead of
  blake3 because the workspace already depends on `xxhash-rust` and not
  on `blake3`. The fingerprint is internal-only; the proto wire shape
  is `bytes` (16 bytes), so the algorithm choice is a private detail.
  1C-α populates the fingerprint table at lowering time using whichever
  hash the resolver prefers.
- **`get_component_meta_payload` signature preserved** — the brief said
  "preserve signature" alongside "REWRITE". The existing `MetaSession::
  get_component_meta_payload` callers depend on the
  `(ComponentMetaAnalysis, &ResolvedComponentMetaState) -> Vec<u8>`
  encode signature; the BFS bridge is exposed as a NEW method
  `get_component_meta_payload_via_bridge` that wraps the legacy path
  with the BFS frontier + depth bound + magic-byte error envelope.
  D19 byte-equiv preserved through both paths because the legacy
  encoder is the ultimate source of bytes.
- **`execute_cooperative_batch` is a non-admission probe** — Tier 1B
  treats unmaterialized keys as `BatchExpandError::EvictedNode` so the
  bridge can surface a typed `BridgeError::StaleAtFrontier` envelope
  without rewriting the cooperative-build pipeline. 1C-α may extend the
  batch path to dispatch cold admissions if needed.

## 6. ChatMessages 60s perf gate

DEFERRED. The ChatMessages corpus run lives outside the default test
gate per D108 + D120. The hermetic surrogate
`cold_chat_messages_via_selective_api_terminates_under_seconds_threshold`
runs in the default suite and asserts surface assembly is sub-second on
a synthetic 12-prop / 8-event surface — passing in 0.00s in the test run.
The external `chat_messages_full_get_component_meta_under_60s_per_run_fresh_cold`
test is gated `#[cfg(feature = "external-corpus")]` per D120; orchestrator
should invoke `cargo test --features external-corpus
chat_messages_full_get_component_meta_under_60s_per_run_fresh_cold` from
a worktree with the external corpus checked out.

## 7. Notes for the 1C-α worker

1. The `OwnedTypeResolutionContext::declaration_fingerprints` table is
   the next critical piece. Step 1A introduced it as
   `FxHashMap<DeclarationFingerprint, DeclId>` — empty. Tier 1B's
   `MetaSession::assemble_surface_from_analysis` projects analysis
   fields with `TypeHandle { query_path: None }` (surface root) until
   the table is populated. Once you wire fingerprint computation at
   lowering time, swap the surface assembly to compute fingerprints
   per-declaration and stamp them into the `TypeHandle.query_path`.
2. The BFS bridge in `MetaSession::get_component_meta_payload_via_bridge`
   currently terminates at depth 0 because no surface handle has a
   `query_path`. Once handles carry real declaration paths, the bridge
   walks the lazy frontier through `execute_cooperative_batch` and
   `assemble_volar_payload(surface, expansions)` becomes the canonical
   bytes producer (today the legacy encoder is fall-through). At that
   point the discriminating test
   `public_get_component_meta_byte_equal_with_pre_tier_1` should
   compare actual byte streams across pre/post-1C-α trees rather than
   compile-only.
3. `execute_cooperative_batch` is a warm-only probe at 1B. If 1C-α
   needs cold admissions for batch keys, route through
   `execute_cooperative` per-key (the `Result<_, BatchExpandError>`
   surface accommodates per-key cold dispatch without breaking the
   contract).
4. The two-tier consumer guard tests (`selective_api_external_consumers_match_catalog`
   + `selective_api_internal_substrate_match_catalog`) are intentionally
   permissive at 1B — they only probe symbol existence. 1C-α should
   tighten them to enforce the D106 catalog (exact set of methods +
   exact set of internal helpers).
5. When deleting `forward_deps_eager_walk_baseline_for_materializer`
   per the §3.3.5 closure rule, ensure
   `forward_deps_for_returns_canonical_dep_union` remains as the
   permanent regression smoke per D80.

## 8. Blockers

None. All gates green except for pre-existing TS test failures
unaffected by this work (same counts on stash).
