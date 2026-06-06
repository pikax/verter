# Native Typeinfo Parity — U2 Reducers + U0 Foundations

Parent architecture: docs/arch/native-typeinfo-parity.md
Sequencing authority: docs/arch/semantic-db-overhaul-unified-remaining-plan.md
Owning U-block(s): U0, U2
Prerequisites: the recovered graph / wire / cache foundation (`docs/arch/semantic-type-graph-plan-recovered.md`); the live one-resolver substrate (`SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore`); the fact-based cache architecture (env-hash split R21, `ReadSetSignature.facts` validity rail, multi-candidate `FamilySlots`, content-free query-identity keys R6 — the family-keyed `Instantiate` / `ResolveMacroPayload` carry the env-bearing content-free `ResolvedDeclSlotIdentity` slot). U2 reducer blocks additionally depend on U0 (the manifest/ledger/coverage substrate that gates every block's row-lift, via the CI coverage gate) and on the U2 query/value-domain foundation block `U2.QUERY_VALUE_DOMAIN` (the typed `SemanticQueryValue` layer + the seven U2 keys + the three U2-landed added keys — `ResolveClassSurface`, `ApparentType`, `TemplateLiteralReduce` — plus the finalized slot-identity shape/model for the U6-landed `FlowReturn` / `ResolveCall`; the `SemanticQueryKeySpec` table grows per block, with spec rows only for the variants present in that block, keeping `semantic_query_key_spec_table_equals_enum` green incrementally). U1/U4/U5 (non-parity prerequisites — the persistent cache-runtime node substrate, the scheduler DAG, the artifact-store rehoming) are depended upon, not owned here.
Consumers: U6 (flow / call solver consumes the U2-provided shape / type-surface facts — `Relate`, `ResolveOverloadSet`, `ResolveClassSurface`, `ApparentType`, `TemplateLiteralReduce`, and the typed value domain — and backfills the U6-owned `ResolveCall`, which lands in U6.CALL_RESOLVE, not in U2), U8 (exporter / `ProgramAnalysisGraph` / `DeclarationAnalysisGraph` projections read the U2 value-domain arms), U10 (mode / demand / expansion-boundary exactness over the U2 reducers), U11 (fact validation / route invalidation / footprint over the U2 cache families), U13 (published `GraphTypeNode` projection of U2 type values), U15 (composite adapter surfaces).
Progress ledger: crates/verter_session/tests/typeinfo_ignored_test_manifest.rs

---

## Scope and authority

This child subplan owns two things and cites — never restates — the parent for
the engine architecture:

1. **U0 foundations** — the manifest re-derivation to exactly 362
   `IgnoredTestRow`s, the separate coverage-only `AdditionalProofRow` table, the
   per-row `ProofRequirement` model, the generated proof registry + typed
   row-test wrapper, the U0 row-exact capability→mechanism→proof coverage table
   (the parent §10.4.1 partition), and the git/CI landing protocol (branch per
   block → CI gate → three-reviewer LAND → squash-merge with the `Typeinfo-Block:`
   trailer; no tracked cursor — git=log, branch-protection=accept, revert=rollback).
   (The `SemanticQueryKeySpec` table generator is NOT a U0 foundation — it is a
   `U2.QUERY_VALUE_DOMAIN` deliverable, item 2.) Parent authority: PART 2 §§9–14
   and the Capability Map.

2. **U2 reducer / foundation parity** — the relation engine (with coinductive-SCC
   discharge) and relation-owned `InferBind`; the utility intrinsics as graph
   reductions; indexed access (union-key distribution, index-signature precedence,
   path projection); mapped types (`-?` optional-origin) and template-literal
   reduction; class surfaces (abstract construct signatures, decorators /
   auto-accessors, the `ResolveClassSurface` / `ApparentType` keys, the U2 overload
   key); enums; module / ambient / merged declarations plus the generalized
   `ResolveDeclarationAugmentation`; JSX foundations (no new query keys); and the
   U2 `SemanticQueryKey` / typed-value-domain / `SemanticQueryKeySpec` context
   closure. Parent authority: PART 1 §§1–8.

The parent is the architecture authority. Each block below cites the parent
section that defines the architecture it implements and states only the concrete
**block contract** — what changes, what is deleted, which named guards land,
which exact manifest rows lift, and how it is verified. No block restates the
engine spec, the wire-purity closure, the per-key cache-soundness rules, or the
budget contracts; those live in the parent and are referenced by section number.

Every block contract uses the parent's per-block contract template (PART 2 §9).
"Done" for any block is the parent's done predicate (PART 2 §11.5 / §11.7 — its
`Typeinfo-Block:` trailer merged + rows `Lifted` + required guards present); a
block's rows may flip `Lifted` only after their coverage is complete and
non-placeholder (PART 2 §10.4); landing is the git/CI protocol — branch per block →
green CI → three-reviewer LAND → squash-merge with the `Typeinfo-Block:` trailer
(PART 2 §§11–14). None of that machinery is re-specified here.

### Block dependency graph (within this subplan)

```
U0.MANIFEST_SUBSTRATE  (no U-block prereqs; gates every other block's row-lift via the CI coverage gate)
        │
        ▼
U2.QUERY_VALUE_DOMAIN  (typed value domain + seven U2 keys + three U2-landed added keys + ProjectionDemand/EvalPolicy demand lattice + mode presets + finalized shape for U6-landed FlowReturn/ResolveCall + per-block SemanticQueryKeySpec)
        │
        ├─► U2.RELATION_INFER       (Relate upgrade + coinductive SCC + InferBind)
        │        │
        │        ├─► U2.INDEXED_ACCESS     (KeyspaceBudget; indexed/union/path)
        │        ├─► U2.MAPPED_TEMPLATE    (mapped -?; TemplateLiteralReduce; ← also INDEXED_ACCESS)
        │        ├─► U2.UTILITIES          (intrinsics consume Relate bindings / Conditional; ← also INDEXED_ACCESS + MAPPED_TEMPLATE)
        │        ├─► U2.CLASS_SURFACES     (ResolveClassSurface / ApparentType / ResolveOverloadSet)
        │        ├─► U2.ENUMS              (ResolveEnum; ← also INDEXED_ACCESS + MAPPED_TEMPLATE)
        │        ├─► U2.MODULE_AUGMENTATION(ResolveDeclarationAugmentation / merged / ambient; ← also INDEXED_ACCESS)
        │        └─► U2.JSX_FOUNDATIONS    (no new keys; reuses ambient/indexed/class + Parameters via UTILITIES; ← also UTILITIES)
```

The spine above shows each block's `U2.RELATION_INFER` keystone edge; the
`← also …` annotations are the additional reducer-to-reducer prerequisite edges
(a block also depends on the reducer whose behavior its rows consume — `KeyOf` /
`IndexedAccess` / `MappedType` / `TemplateLiteralReduce` / `Parameters`-intrinsic).
These cross-edges only ever point "leftward" (toward `INDEXED_ACCESS` /
`MAPPED_TEMPLATE` / `UTILITIES`, none of which depends back on `ENUMS` /
`MODULE_AUGMENTATION` / `JSX_FOUNDATIONS` or on each other in the reverse
direction), so the block prerequisite graph stays acyclic — pinned by
`typeinfo_parity_block_dag_is_acyclic_and_consumed_keys_and_mechanisms_are_prereqs`.

`U2.RELATION_INFER` is the keystone reducer: the utility, indexed, mapped,
template, conditional, and JSX reductions all consume relation bindings or the
coinductive-cycle discharge, so they declare it a prerequisite. The parent U2
token is an aggregate over every block below (PART 2 §11.9): U2 is done only when
every row in the union of all U2-block row-sets is `Lifted`. Downstream
U3 / U8 / U10 / U11 / U13 stay blocked until the whole U2 parent is done (no
`U2.5`).

---

# U0 — Foundations

## U0.MANIFEST_SUBSTRATE

ID: U0.MANIFEST_SUBSTRATE
Parent U-block: U0
Subplan: docs/arch/native-typeinfo-parity-u2-reducers.md

Prerequisites: none (this is the foundation block; it owns no U-block prerequisite).
Blocked until: nothing — U0 is the first block. Every other block in this subplan (and the U6/U3/U8/U10/U11/U13/U15 blocks in the sibling subplans) is blocked until U0 is done, because U0 owns the ledger, the coverage gate, the proof registry, and the DAG guard that decide when any block's rows may lift.

Context: The live manifest at `crates/verter_session/tests/typeinfo_ignored_test_manifest.rs` is the OLD four-field schema (`IgnoredTestRow { file, function, substrate: TargetSubstrate, unblocker }`) with `EXPECTED_TOTAL_IGNORED_COUNT = 362`; the rows live in `manifest_data/typeinfo_ignored_test_manifest_rows.rs` (verified 362 rows). It has no `block_id` / `status` / `proof` / `capability` / `organ` / `owning_u_block` / `semantic_queries` fields, no second `AdditionalProofRow` table, no coverage table, no proof registry, and no row-test wrapper. Full-parity tracking needs the parent's two-table ledger (PART 2 §10), the per-row executable `ProofRequirement` (PART 2 §10.2–10.3), and the row-exact capability→mechanism→proof coverage table that DEFINES completeness (PART 2 §10.4). U0 builds all of it as one clean cutover, and re-derives the binding total to EXACTLY 362 with the manifest parser (the authoritative count; 356 / 371 are stale, 384 is the raw `#[ignore]` line count including macro-body lines — Capability Map preamble). U0 also pins the oracle toolchain version and lands the two U0 generators (the proof registry + row-test wrapper, and the coverage table — the parent §10.4.1 partition). The `SemanticQueryKeySpec` table generator is owned by `U2.QUERY_VALUE_DOMAIN` (which adds the keys it tabulates), NOT U0. Block LANDING is the git/CI protocol (PART 2 §§11–14): branch per block → green CI → three-reviewer LAND → squash-merge with the `Typeinfo-Block:` trailer — there is NO tracked `.cutover-state.typeinfo_parity` cursor, no namespaced xtask, and no crash-recovery substrate for U0 to build. This block exists now because no U2 (or U6) block's rows can flip `Lifted` until they are tracked in the extended ledger and gated by the coverage table.

Changes (exact files / functions):
- `crates/verter_session/tests/typeinfo_ignored_test_manifest.rs` — extend the manifest module IN PLACE (no second ledger file). Replace the four-field `IgnoredTestRow` with the parent's field set (PART 2 §10.1): `{ file, function, substrate: TargetSubstrate, capability: TypeInfoCapability, organ: ArchitectureOrgan, owning_u_block: UBlock, block_id: TypeInfoParityBlockId, semantic_queries: &'static [SemanticQueryName], proof: ProofRequirement, status: IgnoreStatus, unblocker }`. Add the separate coverage-only `AdditionalProofRow` (same fields minus `status`/`unblocker`, never in the count/bijection). Add `enum ProofRequirement { Ts7Oracle(OracleId), StructuralGuard(GuardId), NegativeGuard(GuardId), OracleAndGuard{oracle,guard}, RowTestGuard{file,function} }` (NO `NotTsOracleApplicable` arm) and `enum IgnoreStatus { Ignored, Lifted{block_id} }` (binary — no tracked `Verifying`/`lease_id`; the "in-flight, not-yet-landed" state of a block is simply its unmerged branch — PART 2 §10.1 / §11.1). Add `TYPEINFO_PARITY_BLOCKS: &[BlockContractRow]` (block id, U-block, prereqs, subplan path, required guards, verification-command labels — PART 2 §9), plus per-key `key_owning_block` metadata mapping each `SemanticQueryName` to its owning `block_id` (`ResolveCall → U6.CALL_RESOLVE`, etc.). Add the block-MECHANISM metadata (parent §11.5): a `mechanism_id: MechanismId` (the dominant producer) AND a `consumed_mechanisms: &'static [MechanismId]` list on `IgnoredTestRow`, `AdditionalProofRow`, and `BlockContractRow`, plus a `fn mechanism_owning_block(MechanismId) -> TypeInfoParityBlockId` map naming the single block that owns (produces) each mechanism. Add the guard `typeinfo_parity_block_dag_is_acyclic_and_consumed_keys_and_mechanisms_are_prereqs` (parent §11.5): it builds the block-dependency graph from `TYPEINFO_PARITY_BLOCKS` + the key-owning-block metadata + the mechanism metadata + each `IgnoredTestRow` / `AdditionalProofRow`'s `semantic_queries` and `consumed_mechanisms` lists, and FAILS on (1) a cycle in the block prerequisite graph, (2) a row whose dominant `mechanism_id` owner ≠ its own `block_id`, (3) any row or block-level `consumed_mechanism` whose `mechanism_owning_block` is not itself or a transitive prerequisite, (4) any block or row consuming a key whose owning block is not itself or a transitive prerequisite (retained keys-only check) OR a row whose `block_id` disagrees with the prerequisites implied by its consumed keys, and (5) prints the offending row/block, consumed key/mechanism, owning block, and missing-prerequisite path on failure. It catches the fi08-class deadlock (a `FlowNarrowing`-substrate row whose narrowing siblings live in `U6.NARROW_INVALIDATION`, consuming `PredicateAssertion.assertion_effect_dotted_member_path` owned by `U6.PREDICATE_ASSERTION` when that was not a prerequisite — check 2 forces the row's `block_id` to `U6.PREDICATE_ASSERTION`). Keep the existing carried-forward guards (`every_ignored_typeinfo_test_*`, `every_ignore_reason_meets_minimum_quality_bar`, the reason-equality and per-file partition guards).
- `crates/verter_session/tests/manifest_data/typeinfo_ignored_test_manifest_rows.rs` — regenerate the 362 rows with the extended schema (every row carries `capability`/`organ`/`owning_u_block`/`block_id`/`semantic_queries`/`proof`/`status: Ignored`), re-derived by the manifest parser to exactly 362.
- New generator binaries (each a dedicated `cargo run` target, checked-in output, generated-not-hand-maintained): (1) the proof registry + typed row-test wrapper generator (PART 2 §10.3) mapping each row's `(file::function, ProofRequirement)` to its executable proof artifact and emitting the per-row wrapper invocation; (2) the U0 row-exact coverage-table generator (PART 2 §10.4) mapping every row through `capability → mechanism_id → semantic_queries/facts → ProofRequirement → block_id` (the authoritative `row → block_id` partition is parent §10.4.1). House them under the workspace generator convention (a `cargo run`-wrapped binary invoked by a `pnpm` script, mirroring the existing ts-rs/schemars generator discipline); the corresponding Rust tests only DIFF and FAIL, never write tracked source. (The `SemanticQueryKeySpec` table generator is NOT a U0 deliverable — it lands in `U2.QUERY_VALUE_DOMAIN`, which adds the keys it tabulates; U0 owns only the manifest-derived coverage-table and proof-registry tooling.)
- `crates/verter_session/tests/typeinfo_ignored_test_manifest.rs` (or the umbrella guard harness it is wired into) — register the git/CI landing-protocol guards (PART 2 §§11.5, 11.8, 11.9, 11.10, 11.11, 11.12): `landed_typeinfo_blocks_have_required_guards` (the §11.5 done-predicate part), `no_vacuous_parent_u_block_landing`, `zero_row_blocks_land_exactly_once` (each landed block's `Typeinfo-Block:` trailer appears on exactly one merged target-branch commit), the one-commit-per-block history guard `typeinfo_block_lands_as_single_squashed_commit` (PART 2 §11.11 — trailer only, no tree-hash binding), and the three-reviewer LAND gate `typeinfo_block_accept_requires_review_land_verdict` (PART 2 §11.12 — a PROCESS/branch-protection rule, no persisted receipt). These run in the CI gate. The retired tracked-cursor transaction guards (snapshot/lock/CAS/WAL/lease/receipt/namespace) are NOT introduced — git history + branch protection + `git revert` provide the landing/accept/rollback boundary (PART 2 §13). If a legacy `cutover_state_arch_guard.rs` carried the old typeinfo-namespace assertions, delete those (the legacy top-level `.cutover-state` guards are unrelated and stay).
- No namespaced xtask surface — the typeinfo-parity landing protocol writes NO tracked orchestration file (PART 2 §13). There is NO `xtask cutover-state typeinfo {dispatch,heartbeat,adopt,prepare-verify,gate-receipt,review-receipt,accept,abort}` subcommand, no `[typeinfo_parity]` TOML namespace, no CAS `revision` / `active_blocks` / `gate_receipts` / `review_receipts` / `land_records` / `pending_transactions` state, and no WAL/lease/receipt persistence. A block lands by its branch merging (one squash commit carrying the `Typeinfo-Block:` trailer) after green CI + the three-reviewer LAND. The legacy top-level `.cutover-state` cutover (its own `active_block` / `landed_blocks` / `land` / `dispatch`) is the broader-plan cursor, unrelated to typeinfo parity, and is untouched.
- `package.json` — pin `"@typescript/native-preview"` from the floating `"latest"` to the exact oracle version `7.0.0-dev.20260526.1` (Deliverables / legacy). Land the oracle row generator (deterministic `OracleId` from `(fixture, query, compiler_options_hash, tsgo_version, oracle_schema_version)`, checked-in normalized snapshots, feature/env-gated regeneration) and the guard forbidding `tsgo` execution in runtime/default tests except the gated drift generator (PART 1 §4.4).

Deliverables:
- The two-table ledger (`IgnoredTestRow` × 362 + `AdditionalProofRow`) extended in place, with `EXPECTED_TOTAL_IGNORED_COUNT` defined as the live `count(IgnoredTestRow where status == Ignored)` (never a frozen constant — PART 2 §10.5).
- The per-row `ProofRequirement` model with no escape hatch; every row in BOTH tables resolves to an executable proof artifact.
- The two checked-in U0 generated artifacts: the proof registry + row-test wrapper, and the U0 row-exact coverage table (the §10.4.1 partition) — each produced by a dedicated `cargo run` generator. (The `SemanticQueryKeySpec` table is produced by `U2.QUERY_VALUE_DOMAIN`'s generator, not U0.)
- The git/CI landing protocol (PART 2 §§11–14): the per-block branch discipline, the CI gate (the full Rust+JS workspace gate + the coverage/proof/required/DAG guards), the branch-protection three-reviewer LAND required approval, and the squash-merge `Typeinfo-Block:` trailer convention — together with the one-commit-per-block history guard `typeinfo_block_lands_as_single_squashed_commit` (PART 2 §11.11 — trailer only, no tree-hash binding) and the three-reviewer LAND gate `typeinfo_block_accept_requires_review_land_verdict` (PART 2 §11.12 — PROCESS/branch-protection rule, no persisted receipt). There is NO tracked `.cutover-state.typeinfo_parity` namespace, NO two-namespace TOML schema, NO namespaced xtask, NO WAL/lease/receipt machinery, and NO legacy-deletion lifecycle to deliver (PART 2 §13) — git history + branch protection + `git revert` are the transaction log / accept gate / rollback.
- The pinned oracle toolchain (`7.0.0-dev.20260526.1`), the oracle row generator, and the `tsgo`-forbidden runtime guard.

Legacy deletions:
- The bare four-field `IgnoredTestRow` shape (replaced in place by the extended row; not kept as a parallel type).
- The frozen `const EXPECTED_TOTAL_IGNORED_COUNT: usize = 362` as a hand-maintained constant decoupled from row state — replaced by the derived live `count(status == Ignored)` (the literal `362` survives only as the U0 binding-total assertion, not as a lagging constant).
- The floating `"@typescript/native-preview": "latest"` range in `package.json`.
- No second ledger file is introduced and none is deleted; the manifest is extended in place (PART 2 §10).
- No tracked `.cutover-state.typeinfo_parity` cursor / namespaced xtask / WAL / lease / receipt machinery is built (PART 2 §13) — git/CI is the landing protocol. Any typeinfo-namespace assertions in a legacy `cutover_state_arch_guard.rs` are deleted (the unrelated legacy top-level `.cutover-state` cutover guards stay).
- No projection-repair / second-engine path is removed by this block (it adds substrate only) — stated explicitly per the template.

SemanticQueryKey/facts touched: none directly (U0 is the ledger/coverage substrate). U0 neither adds, changes, nor tabulates a `SemanticQueryKey` — the `SemanticQueryKeySpec` table generator and the U2 key changes both land in `U2.QUERY_VALUE_DOMAIN`. U0's coverage-table generator reads only the manifest (not the key enum).

Exact test rows lifted: none. U0 lifts no `#[ignore]` rows — it builds the substrate that tracks and gates the lifting of all 362 rows. Its own coverage is the META-coverage: every one of the 362 `IgnoredTestRow`s plus every `AdditionalProofRow` acquires a non-placeholder `mechanism_id`, an executable `ProofRequirement`, and a `semantic_queries`/facts mapping in the generated coverage table (PART 2 §10.4) before any block's rows lift.

Required new guards (PART 2 §§10.2–10.5, 11.5, 11.8–11.12):
- Proof + coverage: `every_oracle_id_resolves_to_checked_in_snapshot`, `every_guard_or_row_proof_resolves_to_default_suite_test`, `lifted_row_executes_declared_proof`, `every_manifest_row_has_non_placeholder_mechanism_and_executable_proof`, `capability_rows_map_to_expected_query_fact_mechanisms`, `block_rows_cannot_lift_without_complete_coverage` (a branch flipping rows `Lifted` without complete coverage fails CI), `ignored_test_row_table_holds_exactly_362_rows`, the source-`#[ignore]` ↔ `Ignored`-rows ↔ `EXPECTED_TOTAL_IGNORED_COUNT` bijection/count guards, `no_landed_typeinfo_block_has_live_ignored_rows`.
- Landing protocol (git/CI — all run in the CI gate): `landed_typeinfo_blocks_have_required_guards` (the §11.5 done-predicate part), `no_vacuous_parent_u_block_landing`, `zero_row_blocks_land_exactly_once` (each landed block's `Typeinfo-Block:` trailer appears on exactly one merged target-branch commit), `typeinfo_parity_block_dag_is_acyclic_and_consumed_keys_and_mechanisms_are_prereqs` (builds the block-dependency graph from `TYPEINFO_PARITY_BLOCKS` + the key-owning-block metadata + the block-mechanism metadata (`mechanism_id` + `consumed_mechanisms` on `IgnoredTestRow` / `AdditionalProofRow` / `BlockContractRow` + the `mechanism_owning_block` map) + each row's `semantic_queries` and `consumed_mechanisms` lists, and FAILS on (1) a cycle in the block prerequisite graph, (2) a row whose dominant `mechanism_id` owner ≠ its own `block_id`, (3) any row or block-level `consumed_mechanism` whose owning block is not itself or a transitive prerequisite, (4) any block or row consuming a key whose owning block is not itself or a transitive prerequisite OR a row whose `block_id` disagrees with its consumed-key prerequisites, and (5) prints the offending row/block + consumed key/mechanism + owning block + missing-prerequisite path — the guard that pins the `ResolveCall` U6-ownership so the preceding U2.CLASS_SURFACES / U2.JSX_FOUNDATIONS blocks consume it neither directly nor through an owned row, AND catches the fi08-class mechanism deadlock — parent §11.5), `typeinfo_block_lands_as_single_squashed_commit` (each block lands as exactly one target-branch commit carrying the `Typeinfo-Block:` trailer; trailer only, no tree-hash binding — PART 2 §11.11), `typeinfo_block_accept_requires_review_land_verdict` (the merge gate requires the three-reviewer LAND / NITs-only verdict — 1 Claude Code + 2 codex — a PROCESS/branch-protection rule, no persisted receipt — PART 2 §11.12). The retired tracked-cursor transaction guards (snapshot/lock/CAS/WAL/lease/receipt/namespace) are NOT introduced (PART 2 §13).
- The `tsgo`-execution-forbidden runtime/default-test guard (PART 1 §4.4).

Critical-rule guards: U0 introduces the ledger substrate but no NEW `(CRITICAL)` engine rule of its own — its guards (above) ARE the named guards the parent's PART 2 sections register. If U0 lands any new `(CRITICAL)` rule text in `CLAUDE.md`/skills (e.g. a "manifest is authoritative / 362 binding" rule), it registers the corresponding count/bijection guard in the same change per R6.

Proof requirement: U0 owns no `IgnoredTestRow`; its proof is the suite of structural guards above (each a `StructuralGuard`-class default-suite test), plus the generation-time failure of any generator whose row has no resolvable proof / placeholder mechanism. The coverage gate `block_rows_cannot_lift_without_complete_coverage` is the CI precondition every later block's row-lift rides on.

Exit acceptance:
- The manifest holds EXACTLY 362 `IgnoredTestRow`s (count + bijection green), disjoint from `AdditionalProofRow`s, with `EXPECTED_TOTAL_IGNORED_COUNT == count(status == Ignored)`.
- The two U0 generated artifacts (the proof registry + row-test wrapper, and the §10.4.1 coverage table) plus the gated oracle-row generator are checked in and their diff-tests are green; every manifest row across both tables has a non-placeholder `mechanism_id` + executable `ProofRequirement` + capability-consistent `semantic_queries`/facts. (The `SemanticQueryKeySpec` table is verified in `U2.QUERY_VALUE_DOMAIN`, not here.)
- The git/CI landing-protocol guards (`typeinfo_block_lands_as_single_squashed_commit`, `typeinfo_block_accept_requires_review_land_verdict`, `no_vacuous_parent_u_block_landing`, `zero_row_blocks_land_exactly_once`, `landed_typeinfo_blocks_have_required_guards`, the DAG guard) are registered and green in CI; no tracked `.cutover-state.typeinfo_parity` cursor exists (PART 2 §13).
- `package.json` pins the exact oracle version; the `tsgo`-forbidden guard is green; the oracle generator produces deterministic checked-in snapshots.

Verification commands:
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (count/bijection/proof/coverage + DAG + landing-protocol guards).
- The two U0 generator `cargo run` targets (proof-registry + row-test wrapper, and the §10.4.1 coverage table) + their diff-tests; the oracle-row generator (feature/env-gated) producing checked-in snapshots.
- The full workspace gate (the complete Rust **AND** JavaScript gate, green only when BOTH pass — this IS the CI gate, PART 2 §11.2): `cargo test --workspace --tests`; `cargo clippy --workspace -- -D warnings`; `cargo fmt --all --check`; `pnpm test`; `pnpm install --frozen-lockfile`.
- `node scripts/gen-corpus-audit-tests.mjs` (idempotent; if audit-record schema/fixtures change).
- Commit cadence / review gate: PARENT-UNIFORM — the uniform discipline for EVERY block in this subplan (parent PART 2 §11.11 / §11.12), stated once and not restated per block: each block lands as ONE squash-merge commit (WIP series on the branch during the work, no per-commit gate) carrying the `Typeinfo-Block:` trailer, after green CI + the three-reviewer LAND verdict (1 Claude Code + 2 codex).

Docs updated: keep the `/testing` skill's unignore-manifest + ledger notes current (the two-table ledger, `ProofRequirement`, the §10.4.1 coverage table, the git/CI landing protocol — branch per block → CI gate → three-reviewer LAND → squash-merge with the `Typeinfo-Block:` trailer); record the oracle-version pin and the generated-artifact discipline (generators are `cargo run` targets, tests only diff) in `/build-and-profiling`.

Re-entry notes: U0 is idempotent — re-running re-derives 362 and regenerates its two artifacts (proof registry + row-test wrapper, and the §10.4.1 coverage table) plus the oracle-row snapshots deterministically. A partial U0 leaves an unmerged branch, never a torn tracked cursor — there is nothing to reconcile (PART 2 §13); pick up the branch (or re-cut one) and continue. The manifest tells exactly which generated artifact is stale (its diff-test fails). Do not hand-edit any generated file; re-run its generator.

---

## U0.RESOLVER_CORE_FOUNDATIONS

ID: U0.RESOLVER_CORE_FOUNDATIONS
Parent U-block: U0
Subplan: docs/arch/native-typeinfo-parity-u2-reducers.md

Prerequisites: U0.MANIFEST_SUBSTRATE.

Context: Two resolver-core foundations are U0-owned ownership surfaces, distinct from the manifest/ledger substrate (`U0.MANIFEST_SUBSTRATE`) and from the U2 value-domain SHAPE + keying contract (`U2.QUERY_VALUE_DOMAIN` §174). Both are locked by the design gate `docs/arch/u2-query-value-domain-design.md` and both live in `crates/verter_session/src/resolver_core`. They are recorded here so the U0/U2 boundary stays explicit and no U2 block silently absorbs them.

- **#21 module-resolution MATRIX (FORK-C).** The resolution matrix WALKER — the engine that selects a resolution lane per specifier (relative / absolute / bare-package / package-subpath / package-import) under the active `moduleResolution` mode and `exports`/`imports` condition set, walks the candidate targets, and applies the TS-first `effective_target()` priority — is U0 `resolver_core`. It consumes the content-free module-resolution SHAPE vocabulary (`ModuleResolutionMode`, `SpecifierKind`, `ConditionSet`) defined in `verter_workspace::module_resolution`. The matrix is bounded by `workspace_root` (node_modules / `#imports` ancestor walks stop there) and routes through the one shared cross-file resolver — it is NOT a second per-surface resolution engine.
- **#18 broken-input taint PRODUCERS (FORK-B).** The producers that mint and propagate the broken-input taint marker — the analyzer-side detection of malformed / unresolvable / cyclic input and the propagation rule that carries the taint through the typed-IR so a downstream consumer can degrade typed-honestly rather than fabricate a result — are U0 `resolver_core`. The taint is a typed carrier on the value domain, not a sentinel string; consumers read it, they do not re-derive it.

KEYING boundary (owned elsewhere, NOT by this block): the split-env module-resolution KEYING contract — every import-resolving key/`*Context` carries the split env dims and the lib corpus is NEVER folded into `resolve_env_hash` (R21), with `ModuleResolutionMode` + the `exports`/`imports` `ConditionSet` wired as `resolve_env_hash` inputs — landed in U2B.13 (the `### Module-Resolution Keying (CRITICAL)` rule + the `module_resolution_keys_on_resolve_env_not_type_or_lib` / `resolve_env_does_not_fold_lib_dims` guards in `crates/verter_workspace/src/env_hash_tests.rs`, and the SHAPE vocabulary in `verter_workspace::module_resolution`). U2B.13 lands ONLY the keying vocabulary + env-hash contract; the matrix walker and the taint producers above are NOT in its scope.

SemanticQueryKey/facts touched: the matrix walker resolves through the existing shared cross-file resolver surface and adds no new `SemanticQueryKey`; the taint marker rides the existing typed value domain as an additive carrier.

Critical-rule guards: the keying half is guarded by U2B.13's two env-hash guards (above). The matrix-walker and taint-producer halves are guarded by the FORK-C / FORK-B acceptance tests defined under the design gate `docs/arch/u2-query-value-domain-design.md`.

---

# U2 — Reducer / foundation parity

## U2.QUERY_VALUE_DOMAIN

ID: U2.QUERY_VALUE_DOMAIN
Parent U-block: U2
Subplan: docs/arch/native-typeinfo-parity-u2-reducers.md

Prerequisites: U0.MANIFEST_SUBSTRATE.
Blocked until: U0.MANIFEST_SUBSTRATE done (four-part predicate). This block is the U2 query/value-domain foundation every U2 reducer block depends on.

Context: The live `SemanticQueryKey` enum (`crates/verter_session/src/semantic_query.rs:1564`) returns a single uniform `QueryResult<SemanticNodeId>` and lacks the U2 seven keys (only their predecessors exist) and all five added keys across the plan (the three U2-landed — `ResolveClassSurface` / `ApparentType` / `TemplateLiteralReduce` — plus the two U6-landed `FlowReturn` / `ResolveCall`); the pre-U2 `Relate` was the bare `{ source, target }` shape, which was an unsound cache identity that this plan upgraded to the full relation identity (PART 1 §2.7). The parent requires (a) the typed `SemanticQueryValue` value-domain layer so flow/contextual keys return `ProgramAnalysisGraph` values and augmentation keys return `DeclarationAnalysisGraph` values rather than type nodes (PART 1 §3); (b) the seven U2 keys with the seventh GENERALIZED to `ResolveDeclarationAugmentation` covering Module + Global (PART 1 §§2.1–2.2); (c) the three U2-landed added keys' SHAPES — `ResolveClassSurface`, `ApparentType`, `TemplateLiteralReduce` (PART 1 §§2.3–2.6) — with explicit R21/R6-clean per-key `*Context` structs, plus the finalized slot-identity SHAPE/model that the U6-landed `FlowReturn` / `ResolveCall` variants will reuse (their enum variants + spec rows + dispatch behavior land in U6, NOT here); (d) the `Relate` existing-key upgrade to the full relation identity + `InferenceContextKey` (PART 1 §2.7); (e) explicit contexts on every remaining variant (PART 1 §2.8); (f) the generated `SemanticQueryKeySpec` table closing the per-key class by enum/table equality (PART 1 §2.9) — a STANDING per-block invariant (table == enum on every committed tree, validated incrementally after each block) rather than a one-shot "U2 proves all five rows" gate; and (g) the `ProjectionDemand` / `EvalPolicy` **demand lattice** as the PRIMARY semantic-demand + cache-identity dimension carried on every projection / flow `demand` field, with the five mode names (`Identity` / `Navigate` / `Shallow` / `Expanded` / `Skeleton`) as public presets over it (`Skeleton` = `generic_open = TypeParamShells` + carrier-stop, not a special mode) and cache satisfaction by lattice relation, not enum order (PART 1 §2.10). This block lands the U2 key/value-domain SHAPE + the demand lattice + presets + the spec table for the keys it adds; the reducer BEHAVIOR lands in the per-reducer blocks, the demand-lattice EXACTNESS gating lands in U10.RESULT_DB, and the `FlowReturn` / `ResolveCall` rows + variants land in U6. It exists now because every U2 reducer dispatches through these keys, resolves into these value-domain arms, and carries a `(ProjectionDemand, EvalPolicy)` demand point.

U0 / U2.QUERY_VALUE_DOMAIN boundary: this block owns the value-domain SHAPE + keying contract only. The #18 broken-input taint PRODUCERS and the #21 module-resolution matrix IMPLEMENTATION are U0-owned (FORK-B / FORK-C, locked by the design gate `docs/arch/u2-query-value-domain-design.md`). This block owns the value-domain SHAPE + `admit_decision` and the module-resolution KEYING contract; the producers and the matrix walker live in U0 `resolver_core`.

Changes (exact files / functions):
- `crates/verter_session/src/semantic_query.rs` — add the typed value-domain framework `enum SemanticQueryValue { TypeNode(SemanticNodeId), ProgramAnalysis(ProgramAnalysisValue), DeclarationAnalysis(DeclarationAnalysisValue), OverloadSet(Arc<[SignatureRef]>), Relation(RelationPayload) }` — only the arms for the variants U2 lands (PART 1 §3). The enum is EXTENDED in U6 with the `FlowReturn(Arc<FlowReturnResult>)` and `ResolvedCall(Arc<ResolvedCallResult>)` arms — together with the `FlowReturnResult` / `ResolvedCallResult` result types and the `FlowReturn` / `ResolveCall` enum variants + spec rows + dispatch — an additive enum-arm addition consistent with the standing `semantic_query_key_spec_table_equals_enum` invariant (table == enum after every block). The enum ALSO declares the RESERVED-SEAM arm `DiagnosticAnalysis(CheckResult)` (PART 1 §3) — **NON-LIVE**: no live `SemanticQueryKey` maps to it and the generated `SemanticQueryKeySpec` table carries NO row for it (the spec table tabulates only LIVE query variants, so enum==table holds over the live query surface; a reserved value name with no live query is not a live spec row). The `Check*` query names (`CheckProgram` / `CheckFile` / `CheckRegion` / `CheckExpression` / `CheckAssignable` / `CheckCall` / `CheckDeclaration`) are RESERVED but NOT added to the live `SemanticQueryKey` enum or the spec table; they exist only so a future native-checker block lands cleanly over the same dispatch, and typeinfo NEVER routes through whole-body checking. Add the seven U2 `SemanticQueryKey` variants — `ResolveMergedDeclaration`, `ResolveDeclarationAugmentation` (the generalized seventh, replacing the former `ResolveModuleAugmentation` slot), `ResolveAmbientNamespace`, `ResolveOverloadSet`, `ResolveEnum`, `FlowNarrowingAt`, `ContextualTypeAt` — and the U2-landed added keys `ResolveClassSurface`, `ApparentType`, `TemplateLiteralReduce` plus the U2 overload key — each as enum variant + `SemanticQueryKeySpec` row + dispatch behavior together. `FlowReturn` / `ResolveCall` are NOT added here: U2 finalizes the slot-identity SHAPE/model they will use, but their enum variants + spec rows + dispatch behavior land together in U6, so the standing meta-guard `semantic_query_key_spec_table_equals_enum` holds incrementally (table == enum after every block) rather than as a "U2 proves all five rows" gate. Add each key's named `*Context` struct (`MergedDeclarationContext`, `DeclarationAnalysisContext`, `AmbientNamespaceContext`, `OverloadSetContext`, `EnumContext`, `ProgramAnalysisContext`, `ClassSurfaceContext`, `ApparentTypeContext`, `TemplateLiteralReduceContext`, `RelationContext`, `InferenceContextKey`) carrying only the split env hashes each depends on plus, where applicable, the `substitution` axis — never `project_config_hash` (R21), never content/parse-stable hashes or `fact_dep_signature` (R6). The flow / contextual set-identity axes are NOT on the shared `ProgramAnalysisContext` (which carries only env + `substitution`); they ride as per-variant key fields (`flow: FlowNarrowingKey` on `FlowNarrowingAt`, `contextual: ContextualTypingKey` on `ContextualTypeAt`). Upgrade `Relate` from `{ source, target }` to the full identity `{ source, target, relation: RelationKind, policy: RelationPolicy, source_freshness: FreshnessKey, inference_context: Option<InferenceContextKey>, context: RelationContext }` (PART 1 §2.7). `InferenceContextKey` is the content-free cache-identity projection of the active `InferenceSession` (PART 1 §4.2); this block lands only its KEY SHAPE — the `CheckerTransaction` + `InferenceSession` + `InferenceInfo` substrate it fingerprints lands in U2.RELATION_INFER. Add `enum DeclarationAugmentationTarget { Module(ModuleSpecifier), Global(GlobalEnvScope) }` (env-free — PART 1 §2.2).
- `crates/verter_session/src/semantic_query.rs` (+ a new `semantic_query/demand.rs`) — define the demand lattice (PART 1 §2.10): `struct ProjectionDemand { path, facets, member_demand, call_signatures, construct_signatures, index_signatures, display_needs }` and `struct EvalPolicy { alias_preservation, normalization_depth, generic_open: GenericOpenPolicy, operator_reduction, surface_role, provenance, merge_role, carrier_stop }` as the PRIMARY demand + cache-identity dimension carried on every projection / flow `demand` field (the `demand` on `ResolveClassSurface` / `ApparentType` / `ResolveMergedDeclaration` / `ResolveAmbientNamespace`, the `ProjectionReductionContext` projection axes, and the U6-landed `ReturnProjectionDemand`). Define the five mode names as public preset constructors over the lattice (each = one `(ProjectionDemand, EvalPolicy)` point; `Skeleton` = `generic_open = TypeParamShells` + carrier-stop, NOT a special mode), and the lattice partial order (`dominates` / `meet`) that cache satisfaction / backfill reads — NOT mode-enum ordering. The former coarse `ProjectionMode` enum becomes the preset constructors only; no cache keys on a bare mode tag.
- `crates/verter_session/src/project_semantic_dispatch/mod.rs` — make `execute` return the typed value domain (or expose typed `execute_type_node` / `execute_program_analysis` / `execute_declaration_analysis` / `execute_overload_set` / `execute_relation` wrappers — the five U2-present executors — over the SAME shared `SemanticGraphStore` admission/inflight substrate — PART 1 §3). Warm-hit / backfill on the multi-candidate slots consults the demand-lattice dominance relation (`ProjectionDemand` / `EvalPolicy`), never a mode-enum rank. The `execute_flow_return` / `execute_resolved_call` wrappers land in U6 together with the `FlowReturn` / `ResolveCall` variants (NOT here). The dedup/admission path (`execute_cooperative`) is unchanged; only the value type becomes typed. Map each variant to exactly one value domain; derive `AugmentationTargetKey { project_identity, resolve_env_hash, lib_env_hash, target }` from `DeclarationAnalysisContext` at execution time (PART 1 §2.2) so the augmentation-target env has one source.
- `crates/verter_session/src/semantic_query_memo/mod.rs` — keep the multi-candidate `FamilySlots` substrate (cap 4, FIFO eviction, `ReadSetSignature.validate_with_self_roots`) as the validity rail for the new keys; ensure each new query-identity key carries a content-free identity where applicable (R6 — the env-bearing content-free `ResolvedDeclSlotIdentity` slot for declaration-keyed families) and re-sources the live content version at value-compute time.
- New generator binary — the `SemanticQueryKeySpec` table generator (PART 1 §2.9): for every variant **present on the current committed tree** emit `(lifecycle, context shape, value domain, cross-context guard, admission/budget)`; checked in; a `live` row missing any field, an omitted live variant, or a non-existent variant fails generation. The table grows per block as variants land (it does NOT pre-emit `FlowReturn` / `ResolveCall` rows — those rows enter the table when those variants land in U6), staying enum == table on every committed tree. Dedicated `cargo run` target (generated-not-hand-maintained); its Rust test only diffs.

Deliverables:
- The typed `SemanticQueryValue` value-domain layer over the one shared dispatch/admission substrate, with every variant mapped to exactly one value domain.
- The seven U2 keys (seventh generalized to `ResolveDeclarationAugmentation`), the U2-landed added keys (`ResolveClassSurface`, `ApparentType`, `TemplateLiteralReduce`, the U2 overload key), the `Relate` full-identity upgrade, and every remaining variant's explicit R21/R6-clean context (PART 1 §§2.1–2.8).
- The `ProjectionDemand` / `EvalPolicy` demand lattice + the lattice partial order as the PRIMARY demand + cache-identity dimension, with the five mode names as public presets over it (`Skeleton` = `generic_open = TypeParamShells` + carrier-stop) and cache satisfaction by lattice relation, not enum order (PART 1 §2.10).
- The generated, checked-in `SemanticQueryKeySpec` table closing the per-key class by enum/table EQUALITY (PART 1 §2.9).

Legacy deletions:
- The former `ResolveModuleAugmentation` key slot — generalized into `ResolveDeclarationAugmentation { target: Module | Global }` (the narrow module-only key is removed, not kept alongside).
- The bare `Relate { source, target }` shape — replaced by the full identity (the bare-pair memo key is removed; the `RelationBudget` pair memo re-keys on the full identity — PART 1 §6).
- The soft meta-guard `every_semantic_query_key_has_explicit_context_and_cross_context_warm_hit_guard` — replaced by the mechanical `semantic_query_key_spec_table_equals_enum` (PART 1 §2.9).
- The uniform `QueryResult<SemanticNodeId>` return contract where it implied a single type-node domain for non-type-value keys — reconciled to the typed `SemanticQueryValue` layer.

SemanticQueryKey/facts touched: adds/upgrades the entire U2 key surface; introduces `SemanticQueryValue::{TypeNode, ProgramAnalysis, DeclarationAnalysis, OverloadSet, Relation}` value-domain arms (the U2-present arms). The `SemanticQueryValue::{FlowReturn, ResolvedCall}` arms are introduced in U6 together with the `FlowReturn` / `ResolveCall` variants. Fact reads are unchanged at this block (the reducers' fact reads land per-reducer); the spec table records each variant's admission/budget behavior.

Exact test rows lifted: none directly. This is the shared key/value-domain foundation; it lifts no `#[ignore]` row by itself — every U2 reducer block lifts its rows THROUGH these keys. (The spec-table + value-domain guards below are this block's discriminating proof.)

Required new guards (PART 1 §§2.2, 2.6–2.9, 3):
- Value domain: `every_semantic_query_key_maps_to_exactly_one_value_domain`, `flow_contextual_keys_return_program_analysis_value`, `augmentation_keys_return_declaration_analysis_value`, `declaration_augmentation_facts_not_type_nodes`, `relate_query_value_carries_relation_proof_and_budget_state`.
- Reserved native-checker seam (PART 1 §3 — NON-LIVE): `reserved_checker_queries_are_non_live_typeinfo_does_not_whole_body_check` — the reserved `SemanticQueryValue::DiagnosticAnalysis(CheckResult)` arm and the reserved `Check*` query names (`CheckProgram` / `CheckFile` / `CheckRegion` / `CheckExpression` / `CheckAssignable` / `CheckCall` / `CheckDeclaration`) are NON-LIVE — no live `SemanticQueryKey` variant maps to the arm, the generated `SemanticQueryKeySpec` table carries NO row for it (so `semantic_query_key_spec_table_equals_enum` counts only live variants and stays green), and no typeinfo query whole-body type-checks a region to answer a typeinfo request. The reserved arm/names exist only so a future native-checker block is a clean ADDITION over the same `ProjectSemanticDispatch::execute`.
- Generalized augmentation key: `global_augmentation_query_has_declaration_analysis_identity`, `declaration_augmentation_target_is_env_free_env_comes_from_context`, `declaration_augmentation_doc_wire_query_placement_match`.
- Added-key cache identity (the three U2-landed added keys only): `resolve_class_surface_key_covers_side_demand_type_args_and_context`, `apparent_type_key_covers_lib_env_demand_and_context`, `template_literal_reduce_key_covers_context`. The `flow_return_*` / `resolve_call_*` cache-identity guards (`flow_return_key_covers_env_dimensions`, `flow_return_key_covers_input_context_and_projection_demand`, `resolve_call_key_covers_args_this_contextual_type_overload_policy_and_context`, `resolve_call_same_expr_different_flow_or_substitution_does_not_warm_hit`) land in U6 together with the `FlowReturn` / `ResolveCall` keys they test (a cache-identity guard needs its key's variant to exist on the committed tree).
- `Relate` identity upgrade: `relate_key_covers_relation_kind_policy_freshness_and_context`, `relate_same_nodes_different_relation_kind_policy_or_env_do_not_warm_hit`, `relate_same_nodes_different_inference_context_do_not_warm_hit`.
- Per-key cross-context closure + meta-guard: the full PART 1 §2.8 set (`resolve_merged_declaration_…`, `resolve_ambient_namespace_…`, `resolve_overload_set_…`, `resolve_enum_…`, `flow_narrowing_at_…`, `contextual_type_at_…`, `declaration_augmentation_key_…`, `resolve_decl_…`, `instantiate_…`, `indexed_access_…`, `key_of_…`, `mapped_type_…`, `conditional_…`, `type_of_…`, `normalize_union_…`, `normalize_intersection_…`, `project_path_…`, `resolved_named_type_key_identity_is_env_scoped`, `resolve_macro_payload_…`) and the meta-guard `semantic_query_key_spec_table_equals_enum`, plus dispatch-completeness + schema-version guards for any public wire arm.
- Demand-lattice definition (PART 1 §2.10): `query_modes_are_presets_over_projection_demand_eval_policy` (each of the five mode names resolves to exactly its `(ProjectionDemand, EvalPolicy)` preset; no mode name is a primary key dimension on any cache) and `skeleton_is_typeparamshells_plus_carrier_stop_not_special_mode` (the `Skeleton` preset is exactly `generic_open = TypeParamShells` + carrier-stop, with no special-cased semantic branch keyed on a `Skeleton` mode tag). The satisfaction guard `cache_satisfaction_is_demand_lattice_not_enum_order` lands in U10.RESULT_DB (it gates the published-boundary exactness); the dispatch's multi-candidate dominance here must keep it green.
- Cache-axis minimality (PART 1 §2.10 + §6.2 perf hardening): `cache_key_axes_are_minimal_and_normalized` — every context / substitution / demand / env axis carried on a query-identity key (the `(ProjectionDemand, EvalPolicy)` point, `InferenceContextKey`, the substitution canonical hash, the split env hashes) is benchmark-proven MINIMAL and NORMALIZED — substitution + demand axes canonicalize before entering a key (equivalent forms hash identically), and an axis a family never branches on is not carried on that family's key. Discriminating bench: removing or denormalizing an axis must either break a correctness fixture (the axis was load-bearing) or leave the benched hit rate unchanged (the axis was dead). Lands HERE with the key/context shapes; exercised under benchmark pressure at U3.CACHE_FACT_MODEL + the U15 bench deliverable.

Critical-rule guards: this block implements the parent's `(CRITICAL)` typed-value-domain, per-key-identity, and query-modes-as-demand-lattice-presets (PART 1 §2.10) rules; the value-domain + spec-table + per-key guards above plus the two demand-lattice definition guards (`query_modes_are_presets_over_projection_demand_eval_policy`, `skeleton_is_typeparamshells_plus_carrier_stop_not_special_mode`) ARE their R6 guards (the satisfaction guard `cache_satisfaction_is_demand_lattice_not_enum_order` lands at U10.RESULT_DB). Any new `(CRITICAL)` rule text added to docs in this change registers its guard here in the same change.

Proof requirement: structural guards (the value-domain, identity, cross-context, and spec-table guards), all default-suite tests. The discriminating property: the `SemanticQueryKeySpec` table EXACTLY EQUALS the closed enum, and two queries differing only in env / context / relation-kind / inference-context do NOT warm-hit.

Exit acceptance:
- `semantic_query_key_spec_table_equals_enum` green: every enum variant has exactly one fully-specified spec row; no omissions/extras; `retired` rows absent from the live enum + `reserved` on the wire; `renamed` rows' old name absent.
- Every value-domain, generalized-augmentation, added-key, `Relate`-identity, and per-key cross-context guard green.
- The `ProjectionDemand` / `EvalPolicy` lattice + the five mode presets are landed; each mode name resolves to exactly its `(ProjectionDemand, EvalPolicy)` preset; `Skeleton` is the `generic_open = TypeParamShells` + carrier-stop preset (not a special mode); no cache keys on a bare mode tag (`query_modes_are_presets_over_projection_demand_eval_policy` + `skeleton_is_typeparamshells_plus_carrier_stop_not_special_mode` green).
- `ResolveModuleAugmentation` is gone; `Relate { source, target }` is gone; the soft meta-guard is gone.

Verification commands:
- `cargo test --package verter_session semantic_query` and the dispatch/memo invariants tests (`project_semantic_dispatch_invariants_tests`, `semantic_graph_self_root_tests`, `query_db_self_root_tests`).
- The `SemanticQueryKeySpec` generator `cargo run` target + its diff-test.
- Full workspace gate (as U0).

Docs updated: update the `/type-resolution` skill's Query Mode Contract / `SemanticQueryKey` surface notes for the seven U2 keys + the three U2-landed added keys (`ResolveClassSurface`, `ApparentType`, `TemplateLiteralReduce`) + the finalized slot-identity shape for the U6-landed `FlowReturn` / `ResolveCall` + the generalized `ResolveDeclarationAugmentation` + the typed `SemanticQueryValue` value domain + the `ProjectionDemand` / `EvalPolicy` demand lattice with the five mode names as presets (satisfaction by lattice relation, not enum order) + the per-block `SemanticQueryKeySpec` table; update the `/type-cache-architecture` skill for the upgraded `Relate` identity + `InferenceContextKey` and the per-key R21/R6-clean `*Context` discipline.

Re-entry notes: idempotent. The spec-table generator is the source of truth for the key surface — if a key was added/renamed without regenerating, its diff-test fails. Re-running re-emits the table deterministically.

Checker-readiness: keep the reserved `SemanticQueryValue::DiagnosticAnalysis(CheckResult)` arm and the `Check*` query names (NON-LIVE here, pinned by `reserved_checker_queries_are_non_live_typeinfo_does_not_whole_body_check`) available so the future native checker (`docs/arch/native-checker.md`) can land diagnostics as a query-result VALUE DOMAIN; the value domain must stay shaped so a diagnostic result arm can be added additively, never so one can't. The three hard constraints (`docs/arch/native-checker.md`) hold over this surface: diagnostics are query-results / side-tables, never `GraphTypeNode` arms; no checker-specific resolver (every future `Check*` key routes through the same `ProjectSemanticDispatch::execute`); no whole-body diagnostic walker. This block adds no live checker work — it only keeps that reserved seam open.

---

## U2.RELATION_INFER

ID: U2.RELATION_INFER
Parent U-block: U2
Subplan: docs/arch/native-typeinfo-parity-u2-reducers.md

Prerequisites: U2.QUERY_VALUE_DOMAIN (which depends on U0.MANIFEST_SUBSTRATE).
Blocked until: U2.QUERY_VALUE_DOMAIN done. This is the keystone reducer; the utility / indexed / mapped / template / class / enum / module / JSX blocks all declare it a prerequisite because they consume relation bindings or the coinductive-cycle discharge.

Context: `Relate` must become the SOLE assignability authority (parent §4) handling top/bottom/any/unknown/never, optional/readonly/exact-optional, tuple rest, call/construct + abstract-vs-concrete construct signatures, private/protected compatibility, apparent types, enum/unique-symbol identity, and relation-kind differences — under the full upgraded key (PART 1 §2.7), producing a public `RelationPayload` (outcome / bindings / proof + typed `BudgetExceeded`), not a bare tri-state. `InferBind` is relation-owned AND inference is session-owned (parent §4, §4.2): this block lands the first-class `CheckerTransaction` + `InferenceSession` + `InferenceInfo` substrate — the SOLE inference engine. The `InferTargetPattern` set grows to `{ObjectProperty, TupleHead, TupleTail, TupleInit, TupleLast, ParamTuple, ReturnPosition, TemplatePart}`; binding-producing `Relate` MUTATES the active `InferenceSession` and returns session-local inference deltas (never globally cacheable partials); conditional `infer` extraction runs inside that same session (it CONSUMES relation bindings, never a private matcher); and the explicit candidate-combination rule (parent §4.2 — same-priority covariant candidates union, contravariant intersect, higher priority replaces lower unless the closed-enum rung is `combinable`, a fixed parameter collects no further candidates, the widening-vs-literal fork on first inference) is realized here over the `InferencePriority` closed ladder. Reverse-mapped inference, contextual-callback inference, overload applicability, and final substitution all run through this one substrate (one inference engine, not per-surface matchers). Only a COMPLETED, DETERMINISTIC session is admitted, and what is admitted is the final `RelationPayload` / `Conditional` / instantiation, never the mutable session or a session-local delta; a cancelled / budget-exceeded / mid-flight session is `ReturnOnly`. Same-stack re-entry needs the explicit coinductive-SCC / obligation-discharge protocol (parent §4.1), and the relation assumption stack participates in the shared `CheckerReentryGraph` transaction stack (parent §4.2): scoped assumptions keyed by the FULL `Relate` identity, SCC closure over outgoing non-assumptive obligations, a valid recursive SCC publishing `Assignable` + a `CoinductiveCycle { keys }` proof, a negative obligation yielding publishable `NotAssignable`, and only `Unknown`/cancelled/`BudgetExceeded` making the SCC `ReturnOnly`. The cycle sentinel is a transient relation-stack value, never warm-admitted, never the published proof. This block exists now because relation + inference is the substrate every later reducer needs.

Changes (exact files / functions):
- `crates/verter_session/src/project_semantic_dispatch/inference_session.rs` (new) — the first-class inference substrate (PART 1 §4.2): `CheckerTransaction` (root key, the five split env hashes, contextual target, overload + freshness/excess policy, the shared relation-cycle + `CheckerReentryGraph` stacks, budget, `ReadSetSignature` read-set accumulator, the `InferenceSession` stack), `InferenceSession` (one per inference scope; speculative per overload candidate), `InferenceInfo` per inferable type parameter (`candidates`, `contra_candidates`, `priority`, `top_level`, `is_fixed`, constraint, default, const-param policy), the CLOSED `enum InferencePriority` ladder with the per-rung `combinable` marker bit, and the explicit candidate-combination function (same-priority covariant union, contravariant intersect, higher priority replaces lower unless `combinable`, fixed parameters frozen, widening-vs-literal fork on first inference). Admission is typed: only a `SessionState::CompletedDeterministic` session yields a publishable final result; cancelled / budget-exceeded / mid-flight sessions are `ReturnOnly`.
- `crates/verter_session/src/project_semantic_dispatch/relation.rs` — implement the full relation engine over the upgraded `Relate` key: the relation-kind matrix, abstract-vs-concrete construct rejection (`SignatureKind::AbstractConstruct` not assignable where a concrete constructor is required — PART 1 §1.6), private/protected same-origin compatibility (parent §7), apparent-type relation (via `ApparentType`), enum / unique-symbol identity. Produce `RelationPayload`. Binding-producing `Relate` (`inference_context = Some`) runs INSIDE the active `InferenceSession`, mutating it and returning session-local deltas (PART 1 §4.2); pure non-binding `Relate` (`inference_context = None`) caches its `RelationPayload` normally. Implement the coinductive-SCC assumption protocol: a per-relation-root scoped assumption table keyed by the full identity (incl. `InferenceContextKey`), participating in the shared `CheckerReentryGraph` transaction stack (PART 1 §4.2), SCC discharge on outgoing non-assumptive obligations, `CoinductiveCycle { keys }` proof emission off the type-values surface (carried through `RelationPayload` / the payload-side proof table — PART 1 §4.1). MEASURED variance (PART 1 §4.0): the marker-type-probe fixed-point computes each generic's per-parameter variance through this same relation engine (instantiate with super/sub markers + relate; SCC-aware monotone fixed-point; budget-abandoned ⇒ invariant), cached by declaration/env/TS-version; the relation reads the MEASURED variance (not the `variance_phase` pass marker) when relating generic instantiations, and reads `RelationPolicy` for the bivariant-method quirk. Per-property fresh-object-literal excess checking with spread-taint propagation (PART 1 §4.2): the excess-check relation consults the per-property freshness/taint bit, not a whole-object `FreshnessKey` flag.
- `crates/verter_session/src/project_semantic_dispatch/inference_session.rs` (reverse-mapped pass) — the relation-owned reverse-mapped inference pass INSIDE the `InferenceSession` (PART 1 §4.2): inferring `T` from a value assigned to a homomorphic mapped target reverses the mapping per source property through binding-producing `Relate` (recovering each `T[K]` candidate into the relevant `InferenceInfo`), and the session's final substitution reassembles `T`. The mapped surface it reverses is owned by U2.MAPPED_TEMPLATE; the inference is owned here. No standalone reverse-mapping matcher.
- `crates/verter_session/src/project_semantic_dispatch/build.rs` (and the conditional reducer site) — route `Conditional` reduction through relation bindings (`any` evaluates both branches and unions; distributive `never` collapses; open conditionals distribute the remaining `ProjectPath` into both branches; closed conditionals reduce immediately — parent §4.3). The conditional reducer consumes `Relate` bindings, never a private matcher; `infer` extraction runs inside the active `InferenceSession` (parent §4.2), not a private `infer` matcher.
- `crates/verter_session/src/semantic_query.rs` — add the `InferTargetPattern` variants (`ObjectProperty, TupleHead, TupleTail, TupleInit, TupleLast, ParamTuple, ReturnPosition, TemplatePart`) and `enum RelationKind` / `RelationPolicy` / `FreshnessKey` / the `CoinductiveCycle { keys }` proof node as semantic carriers, plus the closed `enum InferencePriority` (the inference priority ladder with the per-rung `combinable` marker). `NoInferMask` is the occurrence-local `NoInfer` suppression mask on `InferenceContextKey` (PART 1 §§1.2, 2.7); `InferenceContextKey` is the content-free cache-identity PROJECTION of the active `InferenceSession` (the completed-session fingerprint — PART 1 §4.2), not a standalone bag assembled independently of the session.
- `crates/verter_session/src/semantic_query_memo/budgeted_caches.rs` — key the `RelationBudget` pair memo on the FULL `Relate` identity (not the bare pair — PART 1 §6), with typed `BudgetExceeded` non-admission (`ReturnOnly`: no semantic result, no artifact, no fact signature/backfill).

Deliverables:
- The first-class `CheckerTransaction` + `InferenceSession` + `InferenceInfo` substrate (PART 1 §4.2): one inference engine that drives binding-producing relation, conditional `infer`, reverse-mapped inference, contextual-callback inference, overload applicability, and final substitution — with the closed `InferencePriority` ladder and the explicit candidate-combination rule, and typed admission (only completed deterministic sessions; session-local deltas never cached).
- `Relate` as the sole assignability authority producing `RelationPayload`, under the full upgraded key, with relation-owned `InferBind` running inside the active `InferenceSession` and the grown `InferTargetPattern` set.
- The coinductive-SCC discharge protocol with `CoinductiveCycle { keys }` proofs off the type-values surface, participating in the shared `CheckerReentryGraph` transaction stack.
- Conditional reduction consuming relation bindings (no parallel matcher); `infer` extraction inside the session.
- `RelationBudget` keyed on the full identity with `BudgetExceeded` three-layer non-admission.

Legacy deletions:
- Any bare tri-state `RelationResult` return on the public path (replaced by `RelationPayload`).
- Any parallel inference matcher (conditional `infer`, reverse-mapped, contextual-callback, overload applicability, final substitution) outside the `InferenceSession` substrate — all folded into the one session-owned engine (PART 1 §4.2).
- Any globally-cached binding-producing-relation partial result (binding deltas are session-local; only the completed result is admitted).
- Any `RelationBudget` pair memo keyed on the bare `(source, target)` (re-keyed on the full identity in U2.QUERY_VALUE_DOMAIN; this block removes any residual bare-pair memo use).
- No projection-repair / second-engine relation path remains (the one relation engine is authoritative).

SemanticQueryKey/facts touched: `Relate` (value domain `Relation(RelationPayload)`), `Conditional` (consumes `Relate` bindings). Facts read: `Member` / `MemberPresence` (structural member relation), `TypeEnvOptions` (exact-optional / strict relation), `LibIntrinsic` (apparent-type relation), project-generation facts. Admission: `RelationBudget`; coinductive-SCC discharge; `ReturnOnly` on `Unknown` / cancel / `BudgetExceeded`.

Exact test rows lifted (capability `RelationSemantics`, `relation_semantics.rs`; capability `ConditionalInfer`, `conditional_infer.rs` / `no_infer.rs` / `recursive_conditional.rs`; the distributive-conditional `TypeScriptRules` row, `typescript_rules.rs`; the `satisfies`-widening `ModernTsFeatures` row, `modern_ts_features.rs`):
- relation_semantics.rs::relation_any_extends_string_distributes_both_branches
- relation_semantics.rs::relation_never_via_generic_helper_collapses_to_never
- relation_semantics.rs::relation_optional_property_not_assignable_to_required
- relation_semantics.rs::relation_readonly_property_assignable_to_mutable
- relation_semantics.rs::relation_fixed_tuple_assignable_to_first_plus_rest
- relation_semantics.rs::relation_distributive_conditional_over_union_emits_branch_union
- relation_semantics.rs::relation_infer_value_of_object_property
- relation_semantics.rs::relation_infer_head_of_tuple_pattern
- relation_semantics.rs::relation_infer_tail_of_tuple_pattern
- relation_semantics.rs::relation_infer_params_of_function_preserves_optional_undefined
- conditional_infer.rs::conditional_infer_aliases_reduce_when_requested_directly
- conditional_infer.rs::conditional_infer_tuple_pattern_resolves_each_slot
- no_infer.rs::no_infer_literal_call_returns_pinned_literal_from_first_argument
- no_infer.rs::no_infer_component_helper_pins_variant_from_props_argument
- recursive_conditional.rs::recursive_conditional_flatten_unwraps_three_deep_array_to_primitive
- recursive_conditional.rs::recursive_conditional_deep_readonly_marks_every_nested_property
- recursive_conditional.rs::recursive_conditional_deep_partial_marks_every_nested_property_optional
- recursive_conditional.rs::recursive_conditional_awaited_recursive_unwraps_nested_promises
- typescript_rules.rs::typescript_rules_distributive_conditional_expands_each_union_arm
- modern_ts_features.rs::satisfies_array_literal_widens_to_primitive_array

(20 rows. The two added rows ride this block's relation/conditional substrate: `typescript_rules_distributive_conditional_expands_each_union_arm` is the distributive-conditional reduction over a union (mechanism `Conditional` consuming `Relate` bindings), and `satisfies_array_literal_widens_to_primitive_array` is the `satisfies` target-contextual validation + array-literal widening landed here per the "Row-level-split capabilities" note — §10.4.1 assigns both to this block.)

Required new guards (parent §4.1, §4.2, §6):
- `relation_cycle_assumptions_are_scoped_to_full_relate_identity`
- `relation_coinductive_scc_discharges_on_outgoing_obligations`
- `relation_cycle_sentinel_is_never_warm_admitted`
- `relation_proofs_not_graph_type_nodes`, `typeinfo_relate_payload_exposes_relation_proof_without_graph_type_node`
- `relation_budget_exceeded_admits_nothing`
- `inference_runs_in_checker_transaction_not_per_surface_matcher` — generic call inference, conditional `infer`, reverse-mapped inference, contextual-callback inference, overload applicability, and final substitution all enter the `InferenceSession` substrate; fails against any second inference matcher (PART 1 §4.2).
- `only_completed_deterministic_sessions_are_admitted` — a session-local inference delta / an in-flight session is never warm-admitted; a cancelled / budget-exceeded / mid-flight session is `ReturnOnly`, and only the final `RelationPayload` / `ResolvedCall` / `Conditional` / instantiation publishes (PART 1 §4.2 admission rule).
- `inference_candidate_combination_matches_priority_and_variance` — same-priority covariant candidates union, contravariant intersect, higher priority replaces lower unless the closed `InferencePriority` rung is `combinable`, and a fixed parameter collects no further candidates; an oracle-pinned discriminating fixture exercises return-position vs argument-position candidate competition (PART 1 §4.2).
- `variance_is_measured_by_marker_probe_fixed_point_not_assumed` — a generic's per-parameter variance is computed by the SCC-aware marker-type-probe fixed-point (instantiate with super/sub markers and relate through the one `Relate` engine; iterate the SCC to a monotone fixed-point; budget-abandoned ⇒ invariant) and CACHED by declaration/env/TS-version; the `variance_phase` field names the pass, NOT the measured variance; bivariant-method quirks live in `RelationPolicy` (PART 1 §4.0). Replaces any bare `variance_phase` enum stand-in. Discriminating fixture: a contravariant-parameter case and a bivariant-method case pinned against the oracle.
- `reverse_mapped_inference_is_relation_owned_in_session` — reverse-mapped inference (inferring `T` from a value assigned to a homomorphic mapped target `{ [K in keyof T]: F<T[K]> }`) is a relation-owned `InferenceSession` pass: per-key recovery via binding-producing `Relate` depositing into the relevant `InferenceInfo`, reassembled by the session's final substitution — NOT a private reverse-mapping matcher (PART 1 §4.2). Shared with U2.MAPPED_TEMPLATE (the mapped-type reducer owns the mapped surface; the session owns the inference). Discriminating fixture: infer `T` from a value assigned to a homomorphic mapped target and assert recovery routes through the session.
- `freshness_tracks_per_property_spread_taint` — fresh-object-literal excess checking is decided PER PROPERTY with spread-taint propagation, in the session (own-written = excess-checked; spread-in from a non-fresh source = tainted/not-checked; spread-in from a fresh source propagates that source's per-property bits) — the spread-aware extension of `FreshnessKey`, NOT a whole-object freshness bit (PART 1 §4.2). Exercised on the return path at `docs/arch/native-flow-return.md::U6.VALUE_INFERENCE`. Discriminating fixture: spread a non-fresh source into a fresh literal with an extra own property; only the own property is excess-checked.
- `relation_negative_and_unknown_paths_are_fast` — the PART 1 §6.2 perf-hardening guard for the hottest reducer: the common not-assignable / no-match / unknown outcome is decided by cheap structural discriminators (primitive / literal / shape-tag / brand-identity / arity mismatch) BEFORE opening the coinductive-SCC scope or relating members pairwise, and a repeat negative is served from the full-`Relate`-identity pair memo (§6) — both WITHOUT entering the SCC / member-recursion machinery and without allocating a relation proof, a fact payload, or a session transaction; memo locality (interned-ID keys, contiguous per-relation-root assumption scope) is benched. Discriminating fixture: a mismatched pair fast-rejects and a re-ask is a memo hit, neither paying for member recursion or an SCC scope.

Critical-rule guards: implements the parent's `(CRITICAL)` "exactly one type-resolution engine" / relation-cycle / `CheckerTransaction`+`InferenceSession` rules (PART 1 §§4.0, 4.2); the coinductive-SCC + cycle-sentinel + relation-proof + relation-budget guards plus the three inference-session guards above are their R6 guards, joined by the measured-variance / reverse-mapped / per-property-freshness guards (the variance, reverse-mapped, and freshness mechanisms run inside the one relation/session engine, never a second matcher). The shared `CheckerReentryGraph` cross-engine cycle guard `checker_reentry_graph_spans_flow_call_contextual_narrowing` lands in U6.CALL_RESOLVE (the `ResolveCall → FlowReturn → narrowing → ResolveCall` cycle is only fully realizable once `ResolveCall` lands); the relation assumption stack defined here participates in that shared transaction stack. The `NoInfer` occurrence-local rule shares `no_infer_not_type_parameter_metadata` (landed in the wire block / U2.QUERY_VALUE_DOMAIN) and is consumed here via `NoInferMask`.

Proof requirement: per-row — the `RelationSemantics` / `ConditionalInfer` rows are TS7-oracle-pinned (`Ts7Oracle`) where the outcome is an exact TS judgement (e.g. `relation_optional_property_not_assignable_to_required`, the distributive-conditional branch union, the `no_infer` pinned-literal rows), and `OracleAndGuard` where a row also pins a non-admission / cycle property (the recursive-conditional rows pair an oracle with `relation_cycle_sentinel_is_never_warm_admitted`). The candidate-COMBINATION semantics are covered by MAPPING existing inference rows' mechanism to the `InferenceSession` substrate rather than adding rows (the 362 count is unchanged): the `no_infer_*` pinned-literal rows here, and the `f<T>(a:T,b:T)`-style same-priority competition + return-position-vs-argument-position cases in U6.CALL_RESOLVE (`function_advanced_overload_generic_first_*`, `call_resolution_generic_infers_*`) and U6.CONTEXTUAL_CALLBACK (`call_resolution_contextual_callback_return_picks_first_overload` — a contextual callback param competing with a positional arg), each pairing its oracle with `inference_candidate_combination_matches_priority_and_variance` (`OracleAndGuard`). Each row's declared proof is consumed by its generated row-test wrapper (PART 2 §10.3).

Exit acceptance: all 20 rows above lift and pass on the normal `lib*.d.ts` corpus; a genuinely recursive relation (`interface A { next: A }` vs `interface B { next: B }`) discharges POSITIVE and publishes `CoinductiveCycle`; a negative obligation yields publishable `NotAssignable`; only `Unknown`/cancel/`BudgetExceeded` is `ReturnOnly`; the cycle sentinel never warm-admits; `Relate` returns `RelationPayload` with the proof off `GraphTypeNode`.

Verification commands:
- `cargo test --package verter_session relation` and the project-semantic-dispatch relation tests.
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate for this block's rows).
- The block's lifted-row proofs via the generated wrapper (or `-- --ignored` before the branch strips the `#[ignore]`s).
- Full workspace gate (the CI gate, as U0).

Docs updated: update the `/type-resolution` skill's relation / `InferBind` notes (the `CheckerTransaction` + `InferenceSession` substrate, the closed `InferencePriority` ladder + explicit candidate-combination rule, the coinductive-SCC discharge protocol, the grown `InferTargetPattern` set, conditional `infer` running inside the session); keep the `/type-cache-architecture` `RelationBudget` full-identity-keyed non-admission notes current and add the session-admission rule (only completed deterministic sessions; session-local deltas never cached) + `InferenceContextKey`-as-session-fingerprint.

Re-entry notes: relation is mutually recursive (parent §4.1) — re-entry is bounded by the scoped-assumption SCC; same-path recursion records an assumption, never self-awaits. Idempotent under the one-engine guards. If partial, the manifest shows which `relation_*` / `conditional_*` rows still carry `#[ignore]`.

---

## U2.UTILITIES

ID: U2.UTILITIES
Parent U-block: U2
Subplan: docs/arch/native-typeinfo-parity-u2-reducers.md

Prerequisites: U2.QUERY_VALUE_DOMAIN, U2.RELATION_INFER, U2.INDEXED_ACCESS, U2.MAPPED_TEMPLATE.
Blocked until: all four prerequisites done — utility reductions consume `Relate` bindings / `Conditional` discharge and the typed value domain, AND reduce through the indexed-access / `keyof` reducer (`Parameters` / `NonNullable` indexed payloads, `Pick` / `Omit` keyspaces — `IndexedAccess` / `KeyOf` from U2.INDEXED_ACCESS) and the mapped / template reducer behavior (`Required` / `Partial` / `Readonly` modifier mapping — `MappedType` / `TemplateLiteralReduce` from U2.MAPPED_TEMPLATE).

Context: The TS intrinsic utilities are graph REDUCTIONS, not a per-utility engine: `Pick` / `Omit` / `Required` / `Partial` / `Readonly` / `Exclude` / `Extract` / `NonNullable` / `Parameters` / `ConstructorParameters` / `ReturnType` / `InstanceType` / `Awaited` reduce through the shared `Instantiate` / `IndexedAccess` / `KeyOf` / `MappedType` / `Conditional` keys (parent Capability Map → `UtilityComposition`; PART 2 §10.4 `capability_rows_map_to_expected_query_fact_mechanisms`). Built-in utilities behave IDENTICALLY to a userland implementation referencing the same keys (Component-Meta Shallow-By-Default Rule). `Awaited` and the generator/async-generator carriers are first-class semantic carriers (PART 1 §1.1). Top/bottom propagation (`ReturnType<any> = any`, `Parameters<never> = never`, `Awaited<nested Promise> = inner`, `NonNullable<unknown> = {}`, etc.) is exact and oracle-pinned. Variadic tuple utilities (`Head`/`Tail`/`Init`/`Last`/`Concat`) reduce through tuple-pattern `InferBind` (parent §4) and tuple carriers (PART 1 §1.1). This block exists now to land the utility-as-reduction substrate the composite surfaces and component-meta depend on.

Changes (exact files / functions):
- `crates/verter_session/src/project_semantic_dispatch/build.rs` — route every intrinsic utility through the shared keys via `IntrinsicRegistry::lookup` (no per-utility resolver). Top/bottom propagation rules for `ReturnType` / `Parameters` / `ConstructorParameters` / `InstanceType` / `Awaited` / `NonNullable` / `Exclude` over `any` / `unknown` / `never` / `null` / `undefined`. `Awaited` recursive unwrap as a first-class carrier reduction.
- `crates/verter_session/src/semantic_query.rs` — ensure tuple carriers (rest / labels / `Head`/`Tail`/`Init`/`Last` shells — PART 1 §1.1) and the `Awaited` / generator carriers are present in `SemanticNodeData`; the variadic concat path uses tuple-pattern `InferBind` from U2.RELATION_INFER.
- `crates/verter_type_expr_oxc/src/lib.rs` — confirm `lower_ts_type` lowers utility references, tuple rest/label syntax, and `infer` tuple patterns ONCE during shallow analysis (the OXC front-end lowers; it never resolves at query time — parent one-resolver rule). No query-time re-parse.

Deliverables:
- The intrinsic utilities as shared-key graph reductions with exact top/bottom propagation, behaving identically to userland equivalents.
- First-class `Awaited` / tuple-carrier reductions; variadic tuple utilities via tuple-pattern `InferBind`.

Legacy deletions:
- Any per-utility resolver / hand-rolled utility walker (folded into shared-key reductions via `IntrinsicRegistry::lookup`).
- Any `starts_with("Pick<")`-style shape sniff or type-text splitter inside the utility path (Typed-IR-Only Resolver Rule).
- No projection-repair path remains for utilities.

SemanticQueryKey/facts touched: `Instantiate`, `IndexedAccess`, `KeyOf`, `MappedType`, `Conditional` (utility reductions); consumes `Relate` bindings (variadic concat / conditional utilities). Facts read: `Member` / `MemberPresence`, `LibIntrinsic` (intrinsic registry facts), `TypeEnvOptions`. Admission: `Singleflight` for these keys per the generated `SemanticQueryKeySpec` (mapped/template-driven utilities — `Required`/`Partial`/`Readonly` keyspaces — guarded by the planned `KeyspaceBudget` reducer; `ReturnOnly` on overflow).

Exact test rows lifted (capability `UtilityComposition`, `indexed_utilities.rs` / `utility_composition.rs` / `utility_edge.rs` / `utility_top_bottom.rs`; capability `TupleFeatures`, `tuple_labels.rs` / `variadic_tuples.rs`):
- indexed_utilities.rs::direct_parameters_payload_extracts_function_argument
- indexed_utilities.rs::direct_parameters_second_extracts_number_argument
- indexed_utilities.rs::nested_parameters_nonnullable_indexed_payload_resolves
- indexed_utilities.rs::nested_indexed_utility_surface_resolves_all_terminal_members
- indexed_utilities.rs::nested_nonnullable_array_indexed_access_resolves_element
- utility_composition.rs::utility_composition_resolves_required_pick_over_nested_nonnullable_payload
- utility_composition.rs::utility_composition_resolves_deep_intersection_config
- utility_edge.rs::utility_edge_pick_never_yields_empty_object
- utility_edge.rs::utility_edge_omit_all_keys_yields_empty_object
- utility_edge.rs::utility_edge_pick_all_keys_yields_input_shape
- utility_edge.rs::utility_edge_required_strips_optional_markers
- utility_edge.rs::utility_edge_readonly_required_composes_modifiers
- utility_edge.rs::utility_edge_non_nullable_strips_null_and_undefined
- utility_top_bottom.rs::utility_top_bottom_utb01_return_type_of_any_is_any
- utility_top_bottom.rs::utility_top_bottom_utb02_return_type_of_never_is_never
- utility_top_bottom.rs::utility_top_bottom_utb07_parameters_of_any_is_unknown_array
- utility_top_bottom.rs::utility_top_bottom_utb08_parameters_of_never_is_never
- utility_top_bottom.rs::utility_top_bottom_utb11_constructor_parameters_any_is_unknown_array
- utility_top_bottom.rs::utility_top_bottom_utb12_instance_type_any_is_any
- utility_top_bottom.rs::utility_top_bottom_utb13_constructor_parameters_any_ctor_is_any_array
- utility_top_bottom.rs::utility_top_bottom_utb14_awaited_any_is_any
- utility_top_bottom.rs::utility_top_bottom_utb15_awaited_unknown_is_unknown
- utility_top_bottom.rs::utility_top_bottom_utb16_awaited_never_is_never
- utility_top_bottom.rs::utility_top_bottom_utb17_awaited_null_is_null
- utility_top_bottom.rs::utility_top_bottom_utb18_awaited_undefined_is_undefined
- utility_top_bottom.rs::utility_top_bottom_utb19_awaited_nested_promise_is_inner_primitive
- utility_top_bottom.rs::utility_top_bottom_utb20_non_nullable_any_is_any
- utility_top_bottom.rs::utility_top_bottom_utb21_non_nullable_unknown_is_empty_object
- utility_top_bottom.rs::utility_top_bottom_utb22_non_nullable_never_is_never
- utility_top_bottom.rs::utility_top_bottom_utb23_non_nullable_null_undefined_is_never
- utility_top_bottom.rs::utility_top_bottom_utb25_exclude_any_against_string_is_any
- tuple_labels.rs::tuple_labels_parameters_preserves_named_labels_and_optional_marker
- tuple_labels.rs::tuple_labels_numeric_position_access_drops_label
- tuple_labels.rs::tuple_labels_numeric_position_access_on_optional_slot_carries_undefined
- tuple_labels.rs::tuple_labels_number_index_projects_all_elements_union
- variadic_tuples.rs::variadic_tuple_head_of_sample_resolves_to_first_literal
- variadic_tuples.rs::variadic_tuple_tail_of_sample_resolves_to_remaining_tuple
- variadic_tuples.rs::variadic_tuple_last_of_sample_resolves_to_terminal_literal
- variadic_tuples.rs::variadic_tuple_init_of_sample_resolves_to_prefix_tuple
- variadic_tuples.rs::variadic_tuple_concat_alias_produces_joined_literal_tuple
- variadic_tuples.rs::variadic_tuple_variadic_function_with_explicit_type_args_concatenates_tuples
- typescript_rules.rs::typescript_rules_awaited_recursively_unwraps_promises

(42 rows. The added `typescript_rules_awaited_recursively_unwraps_promises` row is the `Awaited<Promise<...>>` recursive-unwrap intrinsic reduction (mechanism `Instantiate` via `IntrinsicRegistry::lookup` — the same builtin-utility dispatch as the `utb14`–`utb19` `Awaited` top/bottom rows), so it is owned here. The OTHER `Awaited` / `ReturnType` / `ConstructorParameters` / `InstanceType` rows in `typescript_rules.rs` and `function_advanced.rs` that depend on FLOW return or the OVERLOAD shape lift in U6 / U2.CLASS_SURFACES respectively — they are NOT this block's rows; this block owns only the pure intrinsic-reduction rows above — §10.4.1.)

Required new guards (parent §6):
- `keyspace_budget_exceeded_admits_nothing`

Critical-rule guards: this block introduces no NEW `(CRITICAL)` engine rule (it implements the existing Macro-Type-Traversal / Shallow-By-Default / Typed-IR-Only rules via shared-key reductions). Stated explicitly per the template. The Shallow-By-Default and Typed-IR-Only rules are pinned by their existing guards in `meta_tests.rs` and the architecture-guard suite; this block must not regress them.

Proof requirement: per-row — the top/bottom rows (`utb01`–`utb25`) and the `Pick`/`Omit`/`Required`/`Readonly`/`NonNullable` edge rows are `Ts7Oracle` (exact TS intrinsic results); the variadic-tuple rows are `Ts7Oracle`; the path-precise edge rows (`pick_never_yields_empty_object`, etc.) pair the oracle with the structural `keyspace_budget_exceeded_admits_nothing` where they exercise a keyspace bound (`OracleAndGuard`). Consumed by each row's generated wrapper.

Exit acceptance: all 42 rows above lift and pass; built-in utilities and userland equivalents produce identical results; top/bottom propagation matches the oracle exactly; `Awaited<Promise<...>>` unwraps recursively to its inner type; no per-utility resolver remains; no type-text splitter exists in the utility path.

Verification commands:
- `cargo test --package verter_session` utility/indexed-utility/tuple tests; the intrinsic-registry coverage test (every `= intrinsic` decl has a registry entry).
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate).
- The block's lifted-row proofs via the generated wrapper.
- Full workspace gate (as U0).

Docs updated: keep the `/type-resolution` skill's utility-as-reduction + `IntrinsicRegistry::lookup` notes current (the intrinsic utilities reduce through the shared keys; top/bottom propagation; `Awaited` / tuple carriers); reaffirm the Component-Meta Shallow-By-Default behavior in `/component-meta`.

Re-entry notes: idempotent. If partial, the manifest shows which `utility_*` / `indexed_utilities` / `tuple_labels` / `variadic_tuples` rows remain `#[ignore]`. The `IntrinsicRegistry` is the single dispatch surface — do not add a parallel utility branch.

---

## U2.INDEXED_ACCESS

ID: U2.INDEXED_ACCESS
Parent U-block: U2
Subplan: docs/arch/native-typeinfo-parity-u2-reducers.md

Prerequisites: U2.QUERY_VALUE_DOMAIN, U2.RELATION_INFER.
Blocked until: both prerequisites done.

Context: Indexed access must distribute union keys, honor string / number / symbol index-signature precedence, and keep intermediate hops in `Navigate` while the terminal hop runs in the caller's mode (parent §4.3; Macro-Type-Traversal Rule path-precision). Index signatures publish per key-kind (string / number / symbol) and the indexed lookup returns the signature VALUE. Union-key access distributes into a member-type union; `keyof self` projects the full value union. Path projection is path-precise (`A['a']['b']` loads only the `a` and `b` hops — Component-Meta Shallow-By-Default Rule; PART 1 §10.4 `PathProjection`). This block exists now to land the indexed / union / index-signature / path-projection substrate the utility, mapped, and JSX reductions project through.

Changes (exact files / functions):
- `crates/verter_session/src/project_semantic_dispatch/build.rs` — the `IndexedAccess` reducer: union-key distribution (one branch per key), string/number/symbol index-signature precedence, intermediate hops `Navigate` + terminal hop caller-mode (the `ProjectPath` walk), `KeyOf` keyspace reduction (literal-anchor reification only under `Published + Expanded`). Index-signature publication per key-kind on the object surface.
- `crates/verter_session/src/semantic_query.rs` — ensure `IndexKey` covers string / number / symbol / union index keys and `SemanticNodeData` carries index-signature shells per key-kind (PART 1 §1.1).
- `crates/verter_session/src/semantic_query_memo/budgeted_caches.rs` — `KeyspaceBudget` on `IndexedAccess` / `KeyOf` (union-key distribution explosion); reverse-demand matching BEFORE enumeration (match demanded keys back into the pattern — parent §6); `ReturnOnly` on overflow.

Deliverables:
- Union-key distribution, string/number/symbol index-signature precedence, path-precise multi-hop projection (intermediate `Navigate`, terminal caller-mode), `keyof` value-union projection.
- `KeyspaceBudget` with reverse-demand matching before enumeration; `ReturnOnly` on overflow.

Legacy deletions:
- Any eager whole-keyspace enumeration on the hot path (replaced by reverse-demand matching).
- Any sibling-key materialization during path projection (path-precision: only the walked hops load).
- No projection-repair path remains for indexed access.

SemanticQueryKey/facts touched: `IndexedAccess`, `KeyOf`, `ProjectPath`, `ProjectMember` (canonicalized to length-1 `ProjectPath`), `NormalizeUnion` (union-key distribution). Facts read: `Member` / `MemberPresence`, `TypeEnvOptions` (unchecked-indexed-access option), project-generation facts. Admission: `Singleflight` for these keys per the generated `SemanticQueryKeySpec` (keyspace-explosion guarded by the planned `KeyspaceBudget` reducer; `ReturnOnly` on overflow).

Exact test rows lifted (capability `IndexSignatures`, `index_signatures.rs`; capability `UnionDistribution`, `union_key_access.rs`; capability `PathProjection`, `deep_path.rs` / `wide_deep.rs`):
- index_signatures.rs::index_signatures_numeric_index_publishes_signature
- index_signatures.rs::index_signatures_symbol_index_publishes_signature
- index_signatures.rs::index_signatures_numeric_lookup_returns_signature_value
- index_signatures.rs::index_signatures_symbol_lookup_returns_signature_value
- index_signatures.rs::index_signatures_dual_string_key_returns_string_signature_value
- index_signatures.rs::index_signatures_dual_numeric_key_returns_numeric_signature_value
- union_key_access.rs::union_key_access_two_key_union_projects_member_type_union
- union_key_access.rs::union_key_access_keyof_self_projects_full_value_union
- deep_path.rs::deep_path_projection_resolves_terminal_without_losing_shape
- wide_deep.rs::wide_deep_projected_target_resolves_terminal_pick_intersection
- wide_deep.rs::wide_deep_projected_token_resolves_literal_union
- wide_deep.rs::wide_deep_row_flags_resolve_partial_record_surface
- wide_deep.rs::wide_deep_flag_active_resolves_boolean_terminal
- typescript_rules.rs::typescript_rules_tuple_rest_element_resolves_array_element_type
- typescript_rules.rs::typescript_rules_keyof_materializes_literal_key_union
- typescript_rules.rs::typescript_rules_indexed_access_reduces_terminal_property

(16 rows. The three added `typescript_rules.rs` rows are oracle-pinned TS quirk rows whose mechanism is `KeyOf` / `IndexedAccess` (`keyof_materializes_literal_key_union` → `KeyOf`; `indexed_access_reduces_terminal_property` → `IndexedAccess`; `tuple_rest_element_resolves_array_element_type` → tuple-rest indexed reduction), so they lift here. The `typescript_rules_distributive_conditional_expands_each_union_arm` row whose mechanism is `Conditional` lifts in U2.RELATION_INFER — §10.4.1 assigns each `typescript_rules_*` row to exactly one block.)

Required new guards: this block's non-admission guard is the shared `keyspace_budget_exceeded_admits_nothing` (already landed in U2.UTILITIES if it precedes; otherwise landed here). No NEW `(CRITICAL)` engine rule is introduced.

Critical-rule guards: none new — implements the existing Macro-Type-Traversal path-precision rule (pinned by its existing traversal guards). Stated explicitly per the template.

Proof requirement: per-row — index-signature + union-key + path-projection rows are `Ts7Oracle` (exact TS index/projection results); rows that pin path-precision non-materialization (`wide_deep_*`) pair the oracle with a structural non-materialization assertion (`OracleAndGuard`). Consumed by each row's generated wrapper.

Exit acceptance: all 16 rows above lift and pass; union keys distribute; string/number/symbol precedence is correct; multi-hop projection is path-precise (no sibling load); `keyof` materializes the literal key union and indexed access reduces the terminal property; `KeyspaceBudget` reverse-matches before enumerating and `ReturnOnly`s on overflow.

Verification commands:
- `cargo test --package verter_session` indexed / union / path-projection tests.
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate).
- The block's lifted-row proofs via the generated wrapper.
- Full workspace gate (as U0).

Docs updated: keep the `/type-resolution` skill's path-projection / indexed-access notes current (union-key distribution, string/number/symbol index-signature precedence, intermediate `Navigate` + terminal caller-mode, `keyof` value-union); reaffirm the Component-Meta Shallow-By-Default path-precision in `/component-meta`.

Re-entry notes: idempotent. If partial, the manifest shows which `index_signatures` / `union_key_access` / `deep_path` / `wide_deep` rows remain `#[ignore]`.

---

## U2.MAPPED_TEMPLATE

ID: U2.MAPPED_TEMPLATE
Parent U-block: U2
Subplan: docs/arch/native-typeinfo-parity-u2-reducers.md

Prerequisites: U2.QUERY_VALUE_DOMAIN, U2.RELATION_INFER, U2.INDEXED_ACCESS.
Blocked until: all three prerequisites done (mapped key-remap runs through `TemplateLiteralReduce`; mapped/template reduction projects through indexed access; `infer` splitting consumes relation bindings).

Context: Mapped `-?` clears ONLY the optional-property-origin `undefined` (the member-presence / optional-origin component), NOT an explicitly declared `| undefined` on a required property (parent §4.3 — the two-component `MemberPresence` / `Member` model). `{ a?: string }` under `-?` becomes `{ a: string }`; `{ a: string | undefined }` REMAINS unchanged. `+?` / bare `?` sets the presence flag without altering the value. Key remap runs through `TemplateLiteralReduce`; `as never` drops keys. Template-literal reduction distributes patterns, applies the intrinsics (`Uppercase` / `Lowercase` / `Capitalize` / `Uncapitalize` keyed by `lib_env_hash`), and splits `infer` (parent §4.3, via the active `InferenceSession` of §4.2; PART 1 §2.6 `TemplateLiteralReduce`). This block exists now to land the mapped-modifier + template-reduce substrate the utility (`Required`/`Partial`/`Readonly`) and key-remap reductions depend on.

Changes (exact files / functions):
- `crates/verter_session/src/project_semantic_dispatch/build.rs` — the `MappedType` reducer modelling the optional-origin / member-presence component SEPARATELY from the value type: `-?` clears the presence flag (and the optional-origin `undefined` it implies) WITHOUT rewriting the value; `+?` / `?` sets it; `as never` drops keys; key remap dispatches `TemplateLiteralReduce`. The `TemplateLiteralReduce` reducer: pattern distribution, the four casing intrinsics via lib facts, `infer` splitting via relation bindings (`TemplatePart` `InferTargetPattern`), and TS LEXICAL numeric/bigint parsing for numeric `infer` segments (`infer N extends number` / `extends bigint`) — TS's numeric/bigint grammar + canonical literal-name normalization, never Rust `str::parse` or a hand-rolled splitter (PART 1 §4.3, Typed-IR-Only Resolver Rule). The homomorphic mapped surface (`{ [K in keyof T]: F<T[K]> }`) this reducer produces is the surface the relation-owned reverse-mapped inference pass (U2.RELATION_INFER, PART 1 §4.2) reverses through the `InferenceSession`; the mapped reducer owns the surface, the session owns the inference.
- `crates/verter_session/src/semantic_query.rs` — ensure the per-member shell carries an optional-origin/presence flag distinct from the value type (the same two-component model the `MemberPresence` / `Member` cache split uses — parent §4.3), and `SemanticNodeData::Mapped` / `TemplateLiteral` shells (PART 1 §1.1) are present.
- `crates/verter_session/src/semantic_query_memo/budgeted_caches.rs` — `KeyspaceBudget` on mapped/template explosion (cartesian products capped, reverse-demand matching before enumeration — parent §6); `ReturnOnly` on overflow.
- `crates/verter_session/tests/typeinfo_ignored_test_manifest.rs` (+ `manifest_data/`) — register the NEW `mapped_modifiers.rs::mapped_modifier_minus_optional_preserves_explicit_undefined_on_required_property` `AdditionalProofRow` (coverage-only, block_id `U2.MAPPED_TEMPLATE`, `ProofRequirement::StructuralGuard(mapped_minus_optional_preserves_explicit_undefined_on_required_property)`); it is NOT one of the 362 `IgnoredTestRow`s and is excluded from the count + bijection (PART 2 §10.1).

Deliverables:
- Mapped `-?` operating on the presence component only (preserving explicit `| undefined` on required properties); `+?` / `?` setting presence; `as never` key drop; key remap via `TemplateLiteralReduce`.
- The `mapped_modifier_minus_optional_preserves_explicit_undefined_on_required_property` `AdditionalProofRow` (coverage-only) pinning explicit-`| undefined` preservation on a REQUIRED property.
- Template-literal reduction: pattern distribution, the four casing intrinsics (lib-fact keyed), `infer` splitting via relation bindings.

Legacy deletions:
- Any `-?` implementation that strips arbitrary `undefined` from the value (replaced by presence-flag-only stripping).
- Any hand-rolled template-text splitter / `split_top_level_*` inside the template path (Typed-IR-Only Resolver Rule — walk the typed IR).
- No projection-repair path remains for mapped / template.

SemanticQueryKey/facts touched: `MappedType`, `TemplateLiteralReduce` (value domain `TypeNode`); consumes `Relate` bindings (`infer` splitting). Facts read: `Member` / `MemberPresence` (the two-component split), `LibIntrinsic` (the four casing intrinsics), `TypeEnvOptions`. Admission: `Singleflight` (keyspace-explosion guarded by the planned `KeyspaceBudget` reducer; `ReturnOnly` on overflow) — matches the generated `SemanticQueryKeySpec` row for both keys.

Exact test rows lifted (capability `MappedTypes`, `mapped_modifiers.rs` / `mapped_template.rs`; capability `TemplateLiteralInference`, `template_literal_inference.rs`):
- mapped_modifiers.rs::mapped_modifier_minus_optional_strips_optional_and_undefined
- mapped_modifiers.rs::mapped_modifier_as_never_filter_drops_matching_keys
- mapped_modifiers.rs::mapped_modifier_conditional_value_keeps_never_typed_members
- mapped_modifiers.rs::mapped_modifier_as_rename_capitalize_rewrites_keys
- mapped_template.rs::mapped_type_with_template_literal_key_remap_resolves_remapped_slot
- mapped_template.rs::mapped_type_with_template_literal_key_remap_resolves_item_slot
- mapped_template.rs::template_literal_key_alias_projects_static_template_slot
- mapped_template.rs::record_with_template_literal_key_union_projects_root_slot
- mapped_template.rs::template_literal_union_key_projects_static_slot_union
- template_literal_inference.rs::template_literal_split_on_dot_produces_segment_tuple
- template_literal_inference.rs::template_literal_strip_on_prefix_uncapitalises_remainder
- template_literal_inference.rs::template_literal_strip_returns_input_unchanged_when_prefix_missing
- template_literal_inference.rs::template_literal_key_remap_capitalises_each_event_key
- template_literal_inference.rs::template_literal_numeric_infer_extends_number_casts_to_literal
- typescript_rules.rs::typescript_rules_template_intrinsic_evaluates_union
- typescript_rules.rs::typescript_rules_key_remap_exclude_filters_and_renames_keys

(16 rows. The two added `typescript_rules.rs` rows are oracle-pinned TS quirk rows whose mechanism is template / mapped reduction: `template_intrinsic_evaluates_union` evaluates a template-literal casing intrinsic over a union (`TemplateLiteralReduce`), and `key_remap_exclude_filters_and_renames_keys` is mapped key-remap with `as never` filtering plus rename (`MappedType` + `TemplateLiteralReduce`), so both lift here — §10.4.1.)

Required new guards (parent §§4.2, 4.3):
- `mapped_minus_optional_strips_only_optional_origin_undefined`
- `mapped_minus_optional_preserves_explicit_undefined_on_required_property`
- `template_literal_reduce_models_ts_numeric_bigint_lexing` — template-literal numeric/bigint `infer` matching (`infer N extends number` / `extends bigint`, placeholder-vs-literal numeric matching) uses TS's LEXICAL numeric/bigint grammar (decimal/hex/octal/binary integer forms, exponent/fractional forms, numeric separators, leading-sign, the `n` bigint suffix, the no-fraction/exponent-bigint rule, and TS's canonical literal-name normalization) — NOT Rust `str::parse` or an ad-hoc splitter; a segment that does not lex as a valid number/bigint does not match the numeric `infer` (PART 1 §4.3). Oracle-pinned against `tsgo`.
- `reverse_mapped_inference_is_relation_owned_in_session` — the mapped-type reducer owns the homomorphic mapped surface that reverse-mapped inference reverses; the inference itself runs as a relation-owned `InferenceSession` pass (per-key recovery via binding-producing `Relate`, reassembled by final substitution), shared with U2.RELATION_INFER (PART 1 §4.2). No standalone reverse-mapping matcher in the mapped path.
- the shared `keyspace_budget_exceeded_admits_nothing` (if not already landed by a preceding block).

Critical-rule guards: implements the parent's `(CRITICAL)` mapped-`-?` two-component rule (the two `mapped_minus_optional_*` guards are its R6 guards) plus the parent's `(CRITICAL)` typed-IR-only template-reduction rule for numeric/bigint lexing (`template_literal_reduce_models_ts_numeric_bigint_lexing` — walk the typed IR / apply TS lexical semantics, never reparse-and-`parse`); the reverse-mapped inference shares the one relation/session engine (no second matcher). The stripping fixture MUST use OPTIONAL properties (`a?: string; b?: number`) so it actually exercises optional-origin removal (parent §4.3).

Proof requirement: per-row — all mapped / template rows are TS7-oracle-pinned (`Ts7Oracle`); the `-?` row (`mapped_modifier_minus_optional_strips_optional_and_undefined`) is `OracleAndGuard` pairing the oracle with `mapped_minus_optional_strips_only_optional_origin_undefined`. The companion required-`| undefined`-preservation contract is a NEW `AdditionalProofRow` — none of the 362 `IgnoredTestRow`s covers it — registered as `mapped_modifiers.rs::mapped_modifier_minus_optional_preserves_explicit_undefined_on_required_property` (block_id `U2.MAPPED_TEMPLATE`, `ProofRequirement::StructuralGuard(mapped_minus_optional_preserves_explicit_undefined_on_required_property)`), coverage-only and excluded from the 362 count + bijection (PART 2 §10.1). It uses a REQUIRED `{ a: string | undefined }` property so it exercises explicit-`| undefined` preservation (distinct from the optional-origin stripping the ignored `-?` row exercises). Each row's / additional-row's declared proof is consumed by its generated wrapper. (See the JSX submatrix in `U2.JSX_FOUNDATIONS` for the other six `AdditionalProofRow`s; these seven are the complete `AdditionalProofRow` set.)

Exit acceptance: all 16 rows above lift and pass plus the one `mapped_modifier_minus_optional_preserves_explicit_undefined_on_required_property` `AdditionalProofRow`; `{ a?: string }` under `-?` becomes `{ a: string }` while `{ a: string | undefined }` is preserved (both guards green); the template-intrinsic and key-remap-exclude TS rules match the oracle; key remap runs through `TemplateLiteralReduce`; the casing intrinsics match the oracle; `infer` splitting uses relation bindings; `KeyspaceBudget` reverse-matches and `ReturnOnly`s on overflow.

Verification commands:
- `cargo test --package verter_session` mapped / template tests.
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate).
- The block's lifted-row proofs via the generated wrapper.
- Full workspace gate (as U0).

Docs updated: update the `/type-resolution` skill's mapped/template notes (the `-?` optional-origin / member-presence two-component rule, key remap via `TemplateLiteralReduce`, the four casing intrinsics keyed by `lib_env_hash`, `infer` splitting via relation bindings); cross-reference the two-component `MemberPresence` / `Member` model in `/type-cache-architecture`.

Re-entry notes: idempotent. If partial, the manifest shows which `mapped_*` / `template_literal_inference` rows remain `#[ignore]`. The optional-origin / value separation is the load-bearing invariant — do not collapse the two components.

---

## U2.CLASS_SURFACES

ID: U2.CLASS_SURFACES
Parent U-block: U2
Subplan: docs/arch/native-typeinfo-parity-u2-reducers.md

Prerequisites: U2.QUERY_VALUE_DOMAIN, U2.RELATION_INFER.
Blocked until: both prerequisites done (class relation reads private/protected brands through `Relate`; identity-compatible decorator effects validate `Relate`-only against the decorator target / member contracts; apparent-member lookup feeds class relation). This block does NOT consume `ResolveCall` — that key lands in U6, and full decorator-call routing is a U6.CALL_RESOLVE backfill, not a prerequisite of this block.

Context: This block owns the class-surface substrate plus the apparent-type and overload-set keys. Class surfaces carry nominal private/protected brand identities separate from published members (`#private` absent from public projection but present in relation identity — parent §7); abstract metadata (`ClassSurface.is_abstract` + per-member abstract flags) with abstract construct signatures using `SignatureKind::AbstractConstruct` (PART 1 §1.6 — abstract-base inheritance, `InstanceType<abstract new ...>`, constructor-utility behavior on abstract, rejecting concrete `new Abstract`); and decorator / auto-accessor members per PART 1 §1.7 (an `accessor` is a declared property whose visibility follows its modifiers — only PUBLIC auto-accessors publish public properties; decorated method return types preserved; identity-compatible decorator effects validated `Relate`-only against the decorator target / member contracts WITHOUT rewriting the surface — replacing the prior `UnsupportedConstruct::Decorator` + diagnostic-projection ruling; full decorator-call routing additionally validated by U6.CALL_RESOLVE once `ResolveCall` lands, as a U6 backfill that does not re-own these rows). `ResolveClassSurface` carries heritage generic substitution + instance/static side + member demand (PART 1 §2.6); `ApparentType` resolves primitive / array / constrained-generic members through lib wrapper interfaces keyed by `lib_env_hash` (parent §4.5); `ResolveOverloadSet` returns ordered signatures — call expressions use the first applicable overload, `ReturnType<typeof overloaded>` / `ConstructorParameters` use the LAST visible overload, not the implementation body (parent §7). This block exists now to land the class / apparent / overload surfaces the JSX, call, and composite blocks consume.

Changes (exact files / functions):
- `crates/verter_session/src/project_semantic_dispatch/build.rs` (+ a class-surface reducer site) — the `ResolveClassSurface` reducer: instance/static heritage with generic substitution, member-demand aware (no whole-surface flatten), nominal private/protected/`#private` brand identities, abstract metadata + abstract-construct rejection, decorator/auto-accessor member surfaces (public-only auto-accessor publication, preserved decorated return, identity-compatible decorator validation `Relate`-only against the decorator target / member contracts — full decorator-call routing is a U6.CALL_RESOLVE backfill). The `ResolveOverloadSet` reducer: ordered signatures, first-applicable for calls, last-visible for `ReturnType`/`ConstructorParameters`. The `ApparentType` reducer: lib-wrapper member lookup keyed by `lib_env_hash`, member-demand REQUIRED on the hot path.
- `crates/verter_session/src/semantic_query.rs` — `SemanticNodeData` class surfaces carry abstract metadata + decorator/auto-accessor member surfaces (PART 1 §1.6–1.7); `SignatureKind::AbstractConstruct` is a first-class signature kind; overload sets are first-class (`SemanticNodeData` overload-set carrier — PART 1 §1.1).
- `crates/verter_semantic/src/analysis/` (class/member-surface lowering) — lower the `accessor` keyword as a declared property with accessibility + static/instance preserved; lower abstract class/member metadata; lower decorated-method declared/inferred return; lower overload signature order. The OXC lowering happens ONCE during shallow analysis (parent one-resolver rule) — no query-time class re-resolution.
- `crates/verter_type_expr_oxc/src/lib.rs` — confirm `lower_ts_type` lowers `abstract new (...)` construct signatures, decorator/auto-accessor syntax, and overload declarations once (front-end lowering only).
- `crates/verter_session/src/semantic_query_memo/budgeted_caches.rs` — `ApparentType` member-demand budget (whole-lib materialization is `BudgetExceeded` / `ReturnOnly` — parent §6); `ResolveClassSurface` / `ResolveOverloadSet` singleflight with `ReturnOnly` on budget.

Deliverables:
- Class surfaces with private/protected/`#private` brands, abstract metadata + abstract-construct rejection, and decorator/auto-accessor members (public-only publication, preserved decorated return, identity-compatible decorator validation).
- The `ResolveClassSurface` (heritage substitution + side + member demand), `ApparentType` (lib-wrapper member lookup, member-demand required), and `ResolveOverloadSet` (ordered; first-applicable for calls, last-visible for `ReturnType`/`ConstructorParameters`) reducers.

Legacy deletions:
- The `UnsupportedConstruct::Decorator` ruling + the decorator/auto-accessor diagnostic-projection path (PART 1 §1.7 — decorators/auto-accessors now participate in the class surface; the recovered-doc `decorators.rs — UnsupportedConstruct::Decorator + diagnostic projection` line is amended to the class-surface ruling).
- Any generic `ResolveClass` key folded into `ResolveClassSurface`; any `GetApparentType` name folded into `ApparentType` (PART 1 §2.3).
- Any whole-class-surface flatten on the hot path (replaced by member demand); any whole-lib apparent-member materialization (replaced by the member-demand budget).
- No projection-repair path remains for class / apparent / overload surfaces.

SemanticQueryKey/facts touched: `ResolveClassSurface`, `ApparentType`, `ResolveOverloadSet` (value domains `TypeNode` / `TypeNode` / `OverloadSet(Arc<[SignatureRef]>)`); consumes `Relate` (brand relation, identity-compatible decorator-effect validation). This block does NOT consume `ResolveCall` (that key lands in U6; full decorator-call routing is a U6.CALL_RESOLVE backfill). Facts read: `Member` / `MemberPresence`, `LibIntrinsic` (apparent surfaces), `TypeEnvOptions`, project-generation facts. Admission: `ApparentType` member-demand budget; `ResolveClassSurface` / `ResolveOverloadSet` singleflight; `ReturnOnly` on budget.

Exact test rows lifted (capability `ClassFeatures`, `class_features.rs` / `decorators.rs`; capability `ApparentTypes`, `apparent_types.rs` / `branded_types.rs`; capability `UniqueSymbol`, `unique_symbol.rs`; the OVERLOAD-shape subset of capability `CallResolution`, `call_resolution.rs` / `function_advanced.rs`; the const-type-param + pure-substitution + variance subset of capability `TypeParameterFeatures` / `ModernTsFeatures`, `const_type_param.rs` / `substitution_types.rs` / `modern_ts_features.rs`; the constructor-utility / instance-type / `typeof const` subset of capability `TypeScriptRules`, `typescript_rules.rs`):
- class_features.rs::class_features_abstract_subclass_instance_includes_inherited_and_own_members
- class_features.rs::class_features_dog_sound_return_type_is_literal_woof
- class_features.rs::class_features_static_inheritance_resolves_inherited_field_type
- class_features.rs::class_features_static_inheritance_resolves_inherited_method_return
- class_features.rs::class_features_extends_plus_implements_projects_union_of_members
- class_features.rs::class_features_generic_subclass_substitutes_type_parameter_on_inherited_field
- class_features.rs::class_features_protected_inherited_member_drives_subclass_method_inference
- class_features.rs::class_features_generic_subclass_with_own_type_param_substitutes_through_base
- class_features.rs::class_features_static_generic_method_instantiation_projects_return_with_substitution
- decorators.rs::decorators_identity_method_decorator_preserves_return_inference
- decorators.rs::decorators_identity_accessor_decorator_publishes_public_property
- decorators.rs::decorators_metadata_reader_describe_return_is_literal_union
- decorators.rs::decorators_accessor_decorator_returning_same_target_publishes_public_property
- apparent_types.rs::apparent_types_ap01_string_length
- apparent_types.rs::apparent_types_ap02_string_to_upper_case
- apparent_types.rs::apparent_types_ap03_string_char_at
- apparent_types.rs::apparent_types_ap04_string_slice
- apparent_types.rs::apparent_types_ap05_number_to_fixed
- apparent_types.rs::apparent_types_ap06_number_to_string
- apparent_types.rs::apparent_types_ap07_number_to_exponential
- apparent_types.rs::apparent_types_ap08_array_length
- apparent_types.rs::apparent_types_ap09_array_map
- apparent_types.rs::apparent_types_ap10_array_filter
- apparent_types.rs::apparent_types_ap11_boolean_to_string
- apparent_types.rs::apparent_types_ap12_boolean_value_of
- apparent_types.rs::apparent_types_ap13_bigint_to_string
- apparent_types.rs::apparent_types_ap14_symbol_description
- apparent_types.rs::apparent_types_ap15_generic_constraint_length
- branded_types.rs::branded_unique_symbol_wrapper_publishes_branded_surface
- branded_types.rs::branded_key_access_projects_literal_brand_tag
- branded_types.rs::branded_key_access_projects_boolean_literal_brand_tag
- branded_types.rs::branded_symbol_key_access_projects_wrapped_value_type
- branded_types.rs::branded_double_intersection_collapses_to_never
- unique_symbol.rs::unique_symbol_indexed_access_via_typeof_returns_literal_value
- unique_symbol.rs::unique_symbol_string_key_access_returns_sibling_value
- call_resolution.rs::call_resolution_abstract_constructor_instance_type_projects_class_shape
- function_advanced.rs::function_advanced_return_type_of_overloaded_function_uses_last_overload
- function_advanced.rs::function_advanced_constructor_parameters_publishes_constructor_arg_tuple
- function_advanced.rs::function_advanced_instance_type_publishes_constructor_return_shape
- function_advanced.rs::function_advanced_call_construct_hybrid_parameters_uses_call_signature
- function_advanced.rs::function_advanced_call_construct_hybrid_return_type_uses_call_signature
- function_advanced.rs::function_advanced_call_construct_hybrid_constructor_parameters_uses_construct_signature
- function_advanced.rs::function_advanced_call_construct_hybrid_instance_type_uses_construct_signature
- function_advanced.rs::function_advanced_class_method_prototype_extraction_projects_return
- function_advanced.rs::function_advanced_class_method_prototype_extraction_projects_parameters
- substitution_types.rs::substitution_types_sb14_default_type_arg_ignored_by_return_type
- substitution_types.rs::substitution_types_sb15_recursive_generic_substitution
- modern_ts_features.rs::variance_annotation_in_substitution_through_consumer_consume_parameters
- typescript_rules.rs::typescript_rules_constructor_parameters_resolve_tuple
- typescript_rules.rs::typescript_rules_instance_type_resolves_constructed_object
- typescript_rules.rs::typescript_rules_class_instance_type_includes_fields_and_methods
- typescript_rules.rs::typescript_rules_typeof_const_preserves_readonly_literals

(52 rows. The added rows ride this block's `ResolveClassSurface` / `Instantiate` substitution + apparent surface: `substitution_types_sb14` (default type arg ignored by return type) and `sb15` (recursive generic substitution) are PURE-substitution rows (not flow narrowing); `variance_annotation_in_substitution_through_consumer_consume_parameters` reduces `Parameters<NumberConsumer[...]>` under variance; and the four `typescript_rules.rs` rows are the constructor-utility / instance-type / class-instance / `typeof const` reductions whose mechanism is `ResolveClassSurface` / `ResolveOverloadSet` / `Instantiate` — per the "Row-level-split capabilities" note. The actual call-expression rows are NOT owned here: the two `const_type_param_*` rows (which apply the TS7 `<const T>` modifier when inferring `T` from a call-site array argument), the overload-selection call rows (`call_resolution_optional_overload_picks_*`, `call_resolution_specific_literal_argument_*`, `function_advanced_overload_call_picks_matching_signature_return`), and the flow / generic-inference rows (`call_resolution_generic_infers_*`, `function_advanced_higher_order_*`, `function_advanced_void_callback_*`, `function_advanced_overload_generic_*`, `function_advanced_constrained_generic_*`, the `this`-parameter rows) all dispatch `ResolveCall` and lift in **U6.CALL_RESOLVE**; the `call_resolution_contextual_callback_return_picks_first_overload` row lifts in **U6.CONTEXTUAL_CALLBACK**. This block owns the overload-SHAPE surface (`ResolveOverloadSet` ordered signatures, `ReturnType<typeof overloaded>` / `ConstructorParameters` last-visible selection) those call rows consume, but not the call dispatch itself. The flow-narrowing-of-generic `substitution_types_sb01`–`sb08`/`sb11`–`sb13` rows (incl. `sb07_constraint_flow_apparent_type`) lift in U6.NARROW_SUBSTITUTION, and the generic-predicate `sb09`/`sb10` rows in U6.PREDICATE_ASSERTION. This block owns only the overload-SHAPE + abstract-constructor + hybrid-signature + prototype-extraction + pure-substitution + variance + constructor-utility rows above — §10.4.1.)

Required new guards (PART 1 §§1.6–1.7, §6):
- `decorator_identity_method_preserves_declared_return`
- `accessor_decorator_publishes_public_property`
- `decorated_method_literal_union_return_projects`
- `accessor_decorator_identity_target_return_keeps_public_property`
- `apparent_type_budget_exceeded_admits_nothing`

Critical-rule guards: implements the parent's `(CRITICAL)` abstract-class / TS7-decorators-auto-accessors / Fallthrough-class-surface rules; the four decorator/accessor guards above plus the abstract-construct matrix are their R6 guards. The recovered-doc `UnsupportedConstruct::Decorator` amendment lands in this change.

Proof requirement: per-row — class / apparent / overload / abstract rows are TS7-oracle-pinned (`Ts7Oracle`); the four decorator/accessor rows are `OracleAndGuard` (oracle + the named decorator/accessor guard); the apparent rows that pin member-demand non-materialization pair with `apparent_type_budget_exceeded_admits_nothing` (`OracleAndGuard`). Consumed by each row's generated wrapper.

Exit acceptance: all rows above lift and pass; abstract-base inheritance / `InstanceType<abstract new ...>` / constructor-utility-on-abstract work and concrete `new Abstract` is rejected; public auto-accessors publish public properties while private/protected/`#private` ones participate only in brand identity; decorated method returns are preserved; identity-compatible decorator effects validate `Relate`-only against the decorator target / member contracts without surface rewrite (full decorator-call routing is a U6.CALL_RESOLVE backfill, not this block's exit-acceptance); the overload SHAPE is ordered so `ReturnType<typeof overloaded>` / `ConstructorParameters` use the last visible overload (the call-expression first-applicable selection is U6.CALL_RESOLVE's exit-acceptance, consuming this block's ordered overload set); `ApparentType` never forces whole-lib materialization (budget green).

Verification commands:
- `cargo test --package verter_session` class / decorator / apparent / overload tests.
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate).
- The block's lifted-row proofs via the generated wrapper.
- Full workspace gate (as U0).

Docs updated: update the `/component-meta` skill's class-surface + decorator/auto-accessor + abstract-construct notes (the TS7 decorator/auto-accessor class-surface ruling replacing `UnsupportedConstruct::Decorator`; abstract metadata + `SignatureKind::AbstractConstruct`); update the `/type-resolution` skill for the `ResolveClassSurface` / `ApparentType` / `ResolveOverloadSet` keys and apparent-type lib-wrapper lookup. Amend the recovered foundation doc's `decorators.rs — UnsupportedConstruct::Decorator + diagnostic projection` line to the class-surface ruling (owned by the recovered-doc integration step, not performed here).

Re-entry notes: idempotent. If partial, the manifest shows which `class_features` / `decorators` / `apparent_types` / `branded_types` / `unique_symbol` / overload-SHAPE `function_advanced` (`return_type_of_overloaded_function_uses_last_overload`, `constructor_parameters_*`, `instance_type_*`, the four hybrid `call_construct_*`, the prototype-extraction rows) / abstract-constructor `call_resolution` / pure-substitution `substitution_types` (`sb14`/`sb15`) / `variance_annotation` / constructor-utility `typescript_rules` rows remain `#[ignore]` (the call-expression `const_type_param` / overload-selection / generic-inference rows lift in U6.CALL_RESOLVE; the contextual-callback overload row in U6.CONTEXTUAL_CALLBACK). Decorators/auto-accessors are class-surface members, NOT diagnostics — do not reintroduce the unsupported-construct path.

---

## U2.ENUMS

ID: U2.ENUMS
Parent U-block: U2
Subplan: docs/arch/native-typeinfo-parity-u2-reducers.md

Prerequisites: U2.QUERY_VALUE_DOMAIN, U2.RELATION_INFER, U2.INDEXED_ACCESS, U2.MAPPED_TEMPLATE.
Blocked until: all four prerequisites done — enum identity reads through `Relate`; discriminant extraction consumes relation bindings; `enum_keyof_typeof_*` projects the member-name union through the `KeyOf` reducer (U2.INDEXED_ACCESS); and `enum_template_literal_over_string_enum_*` produces the value union through `TemplateLiteralReduce` (U2.MAPPED_TEMPLATE).

Context: Enums carry value / type duality (PART 1 §1.1): a numeric member resolves to a branded literal, a string member to a branded string literal, a const-enum member inlines its literal. `keyof typeof Enum` yields the member-name union; a template-literal over a string enum produces the value union; discriminant extraction projects the matching arm payload. `ResolveEnum` is the dedicated key (PART 1 §2.8 — `EnumContext` carries no substitution axis since an enum declaration is not generic). This block exists now to land the enum value/type-duality substrate.

Changes (exact files / functions):
- `crates/verter_session/src/project_semantic_dispatch/build.rs` (+ enum reducer site) — the `ResolveEnum` reducer: branded numeric/string/const-enum literal members, value/type duality, `keyof typeof` member-name union (via `KeyOf`), template-over-string-enum value union (via `TemplateLiteralReduce`), discriminant extraction (via `Relate` bindings).
- `crates/verter_session/src/semantic_query.rs` — `SemanticNodeData` enum carrier with value/type duality (PART 1 §1.1); the branded-literal identity used by relation.
- `crates/verter_semantic/src/analysis/` — lower enum member declarations (numeric / string / const) once during shallow analysis.

Deliverables:
- The `ResolveEnum` reducer with value/type duality, branded literal members, const-enum inlining, `keyof typeof` member-name union, template-over-string-enum value union, and discriminant extraction.

Legacy deletions:
- Any enum resolution outside the dedicated `ResolveEnum` key (folded in).
- No projection-repair path remains for enums.

SemanticQueryKey/facts touched: `ResolveEnum` (value domain `TypeNode`); consumes `KeyOf` (member-name union), `TemplateLiteralReduce` (template-over-enum), `Relate` (discriminant extraction). Facts read: `Member` / `MemberPresence`, `TypeEnvOptions`, project-generation facts. Admission: singleflight; `ReturnOnly` on overflow.

Exact test rows lifted (capability `EnumResolution`, `enums.rs`):
- enums.rs::enum_numeric_member_resolves_to_branded_literal_zero
- enums.rs::enum_string_member_resolves_to_branded_string_literal
- enums.rs::enum_template_literal_over_string_enum_produces_value_union
- enums.rs::enum_keyof_typeof_numeric_yields_member_name_union
- enums.rs::enum_keyof_typeof_string_yields_member_name_union
- enums.rs::enum_const_enum_member_resolves_to_inlined_string_literal
- enums.rs::enum_discriminant_extract_projects_matching_arm_payload

Required new guards: none beyond the shared per-key cross-context guard `resolve_enum_do_not_warm_hit` (landed in U2.QUERY_VALUE_DOMAIN). No NEW `(CRITICAL)` engine rule.

Critical-rule guards: none new — implements the existing value/type-duality coverage via `ResolveEnum`. Stated explicitly per the template.

Proof requirement: per-row — all enum rows are TS7-oracle-pinned (`Ts7Oracle`); the const-enum-inlining row pairs the oracle with a structural assertion that the const member inlines (`OracleAndGuard`). Consumed by each row's generated wrapper.

Exit acceptance: all 7 rows lift and pass; numeric/string members are branded literals; const-enum members inline; `keyof typeof` yields the member-name union; template-over-string-enum yields the value union; discriminant extraction projects the matching arm.

Verification commands:
- `cargo test --package verter_session` enum tests.
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate).
- The block's lifted-row proofs via the generated wrapper.
- Full workspace gate (as U0).

Docs updated: keep the `/type-resolution` skill's `ResolveEnum` value/type-duality notes current (branded numeric/string/const-enum literals, `keyof typeof` member-name union, template-over-string-enum value union, discriminant extraction).

Re-entry notes: idempotent. If partial, the manifest shows which `enums` rows remain `#[ignore]`.

---

## U2.MODULE_AUGMENTATION

ID: U2.MODULE_AUGMENTATION
Parent U-block: U2
Subplan: docs/arch/native-typeinfo-parity-u2-reducers.md

Prerequisites: U2.QUERY_VALUE_DOMAIN, U2.RELATION_INFER, U2.INDEXED_ACCESS.
Blocked until: all three prerequisites done — merged/ambient surfaces relate through `Relate`; augmentation facts resolve to the `DeclarationAnalysis` value domain landed in U2.QUERY_VALUE_DOMAIN; and `import_attribute_simulated_string_literal_indexed_member` reduces an indexed access over the `typeof import` shape through the `IndexedAccess` reducer (U2.INDEXED_ACCESS).

Context: This block owns merged declarations, ambient modules / namespaces, and the GENERALIZED `ResolveDeclarationAugmentation` (the seventh U2 key) covering module AND global declaration-environment-mutation facts under one identity (PART 1 §§2.1–2.2). Module facts lower to `module_augmentations`, global facts (`declare global` / `export as namespace` / UMD globals) to `global_augmentations`; both resolve to `SemanticQueryValue::DeclarationAnalysis`, NEVER `TypeNode` and NEVER a `GraphTypeNode` arm (PART 1 §3 — the value-domain counterpart of the wire-side `DeclarationAnalysisGraph` relocation). Merged declarations, ambient modules, and ambient namespaces REMAIN value-bearing object surfaces (queryable type values via `ResolveMergedDeclaration` / `ResolveAmbientNamespace` — PART 1 §1.3), distinct from the augmentation FACTS that mutate the declaration environment. `typeof import(...)` (default / named value / named shape), namespace + interface merge, and CJS `export =` resolve through these merged/ambient surfaces. This block exists now to land the module / ambient / merged + augmentation substrate the JSX foundations (`JSX` namespace via ambient + augmentation + merged) depend on.

Changes (exact files / functions):
- `crates/verter_session/src/project_semantic_dispatch/build.rs` (+ merged/ambient/augmentation reducer sites) — the `ResolveMergedDeclaration` reducer (merged object surface with contributor provenance), the `ResolveAmbientNamespace` reducer (ambient module / namespace object surface), and the `ResolveDeclarationAugmentation` reducer (env-free `Module(ModuleSpecifier)` / `Global(GlobalEnvScope)` target; `AugmentationTargetKey { project_identity, resolve_env_hash, lib_env_hash, target }` DERIVED from `DeclarationAnalysisContext` at execution time — PART 1 §2.2). `typeof import` / namespace-interface-merge / CJS `export =` resolution through these surfaces.
- `crates/verter_semantic/src/analysis/` — produce global augmentations (`declare global` / `export as namespace` / UMD globals) in declaration analysis ALONGSIDE module-augmentation analysis, with the same contributor-provenance discipline as merged declarations (PART 1 §3). Lower merged-declaration / ambient-module / ambient-namespace / module-augmentation surfaces once during shallow analysis. Populate the `augmentation_index` inverse-lookup skeleton under `AugmentationTargetKey { project_identity, resolve_env_hash, lib_env_hash, target }` (project + env isolation — Cache Architecture). DECLARATION-MERGE ORDER (PART 1 §1.8): assemble the merged surface in TS binder order (source-order same-name merge) with overload-group precedence (declared overloads in order; implementation signature internal-only), order augmentation contributors deterministically by the declaration-analysis contributor sequence, and RECORD that contributor sequence as facts so the merged result is deterministic and `ReadSetSignature` validates against the exact contributor set + order (a new/removed/reordered contributor invalidates the cached merge through the recorded facts — R6 version rooting on the value, not a query-identity key).
- `crates/verter_session/src/semantic_query.rs` — `enum DeclarationAugmentationTarget { Module(ModuleSpecifier), Global(GlobalEnvScope) }` (env-free); `DeclarationAnalysisValue` (the value-domain arm; module facts → `module_augmentations`, global facts → `global_augmentations`); `SemanticNodeData` merged-declaration / ambient-module / ambient-namespace value-bearing surfaces (PART 1 §1.1, §1.3).
- `crates/verter_protocol/proto/verter/v1/typeinfo.proto` — the `DeclarationAnalysisGraph { module_augmentations, global_augmentations }` side surface on `TypeInfoGraphPayload.declaration_surfaces` (additive; the `GraphTypeNode` arms `module_augmentation` (23) / `global_augmentation` (25) are retired + `reserved` and relocated here — PART 1 §§1.3–1.5). Schema-version gated.

Deliverables:
- The `ResolveMergedDeclaration`, `ResolveAmbientNamespace`, and generalized `ResolveDeclarationAugmentation` (Module + Global) reducers, with module/global augmentation facts resolving to `DeclarationAnalysis`.
- The `DeclarationAnalysisGraph` declaration side surface on `TypeInfoGraphPayload.declaration_surfaces` (additive), with the augmentation `GraphTypeNode` arms retired/`reserved` and relocated.
- `typeof import` / namespace-interface-merge / CJS `export =` resolution through merged/ambient surfaces.

Legacy deletions:
- The former `ResolveModuleAugmentation`-era module-only handling (generalized into `ResolveDeclarationAugmentation { Module | Global }` in U2.QUERY_VALUE_DOMAIN; this block removes any residual module-only augmentation path).
- The `GraphTypeNode` `module_augmentation` (23) / `global_augmentation` (25) type-value arms (retired + `reserved`, relocated to `DeclarationAnalysisGraph` — PART 1 §1.3). The recovered-doc `module_augmentation = 23` / `global_augmentation = 25` type-value arms, the §8 exporter `TypeNode::ModuleAugmentation` DTO, and the `module_augmentation_is_public_graph_state` guard are amended/RETIRED and replaced by the declaration-surface guards (Cross-reference / doc-update obligations).
- No projection-repair path remains for module / ambient / merged surfaces.

SemanticQueryKey/facts touched: `ResolveMergedDeclaration` (`TypeNode`), `ResolveAmbientNamespace` (`TypeNode`), `ResolveDeclarationAugmentation` (`DeclarationAnalysis(DeclarationAnalysisValue)`). Facts read: `ModuleAugmentation`, `AmbientGlobal`, `ExportSurface`, `Member` / `MemberPresence`, `RouteGeneration`, `LibIntrinsic` (lib-declared global/ambient surfaces a global augmentation mutates — `lib_env_hash` IS part of `DeclarationAnalysisContext`), `TypeEnvOptions`, project-generation facts. Admission: singleflight; `ReturnOnly` on overflow/cancel.

Exact test rows lifted (capability `ModuleFeatures`, `module_features.rs`; the import-attribute `typeof import` subset of capability `ModernTsFeatures`, `modern_ts_features.rs`):
- module_features.rs::module_features_namespace_geometry_vector_aliases_point
- module_features.rs::module_features_declare_global_merges_two_blocks
- module_features.rs::module_features_typeof_import_default_resolves_value_shape
- module_features.rs::module_features_typeof_import_named_shape_resolves_to_interface
- module_features.rs::module_features_typeof_import_named_value_resolves_to_literal
- module_features.rs::module_features_module_augmentation_merges_plugin_surface
- module_features.rs::module_features_cjs_export_equals_resolves_to_carrier
- module_features.rs::module_features_namespace_interface_merge_namespace_value_resolves_to_literal
- module_features.rs::module_features_external_module_augmentation_merges_config
- modern_ts_features.rs::import_attribute_simulated_resolves_imported_json_shape
- modern_ts_features.rs::import_attribute_simulated_string_literal_indexed_member

(11 rows. The two added `import_attribute_*` rows resolve a simulated `import ... with { type: "json" }` module shape — surfacing the `readonly` flag on the projected members and reducing the `as const` `typeof` indexed access — through the merged / ambient / `typeof import` surfaces this block owns, so their owning `block_id` is U2.MODULE_AUGMENTATION (§10.4.1). The remaining `modern_ts_features.rs` rows lift elsewhere by mechanism: `satisfies_*` in U2.RELATION_INFER / U6.VALUE_INFERENCE, `variance_annotation_*` in U2.CLASS_SURFACES, `await_using_*` in U6.ASYNC_GENERATOR.)

Required new guards (PART 1 §§1.8, 2.2, 3):
- `global_augmentation_query_has_declaration_analysis_identity` (if not already landed in U2.QUERY_VALUE_DOMAIN; this block exercises it)
- `declaration_augmentation_target_is_env_free_env_comes_from_context`
- `declaration_augmentation_facts_not_type_nodes`, `augmentation_keys_return_declaration_analysis_value`
- `declaration_augmentation_doc_wire_query_placement_match`
- `declaration_merge_records_binder_overload_augmentation_order_as_facts` — the merged-declaration / augmentation reducers order contributors by TS binder order (source-order same-name merge), overload-group precedence (declared overloads in order; implementation signature internal-only), and augmentation-contributor sequence (the declaration-analysis contributor provenance), and RECORD that contributor sequence as facts so `ReadSetSignature` validates against the exact contributor set + order — a new/removed/reordered contributor invalidates the cached merge through the recorded facts (R6 version rooting on the value; PART 1 §1.8). Discriminating fixture: an order-sensitive merged surface pinned against the oracle + a contributor-addition invalidation assertion.
- the wire-side relocation guards `graph_type_node_oneof_contains_only_type_value_arms` / `graph_type_node_allowlist_arms_have_type_value_classification` (the augmentation arms must FAIL these until retired/relocated — PART 1 §1.3).
- `session_overlay_augmentation_fails_closed_until_implemented` (unified-plan §0.5.1 fail-closed gate) — a base-only `FileArtifactStore::augmentation_index` is acceptable ONLY as an intermediate; a session/overlay-aware augmentation query (an unsaved-buffer / overlay edit that adds or removes a `declare module` / `declare global` contributor) must EITHER be implemented OR EXPLICITLY FAIL CLOSED — returning a typed degraded / `ReturnOnly` result, NEVER a silently-stale base-only answer presented as session-authoritative (a silent base-only answer is a correctness compromise, not a degradation), and never publishing a base-only result as a session-authoritative warm entry (composes with the overlay-results-do-not-populate-base-caches Cache-Architecture rule + the §0.5.3 broken-code recovery contract). Discriminating fixture: an overlay that adds an augmentation contributor over a base-only index asserts a typed degraded / `ReturnOnly` result, NOT the stale base-only shape.

Critical-rule guards: implements the parent's `(CRITICAL)` declaration-augmentation / value-domain / `GraphTypeNode`-purity rules plus the declaration-merge-order rule (PART 1 §1.8); the augmentation-identity + value-domain + wire-relocation guards above, `declaration_merge_records_binder_overload_augmentation_order_as_facts`, and `session_overlay_augmentation_fails_closed_until_implemented` (the unified-plan §0.5.1 fail-closed gate) are their R6 guards. The recovered-doc augmentation-placement amendment and the `module_augmentation_is_public_graph_state` retirement land in this change.

Proof requirement: per-row — the merged/ambient/`typeof import`/CJS rows are TS7-oracle-pinned (`Ts7Oracle`); the augmentation rows (`declare global`, `module_augmentation_merges_plugin_surface`, `external_module_augmentation_merges_config`) are `OracleAndGuard` pairing the oracle with `declaration_augmentation_facts_not_type_nodes` (the fact resolves to `DeclarationAnalysis`, never a `GraphTypeNode` arm). Consumed by each row's generated wrapper.

Exit acceptance: all 11 rows lift and pass; module + global augmentation facts resolve to `DeclarationAnalysis` (never `TypeNode` / `GraphTypeNode`); merged declarations / ambient modules / ambient namespaces remain value-bearing object surfaces; the two `import_attribute_*` simulated-JSON `typeof import` shapes resolve through the merged/ambient surfaces; the `AugmentationTargetKey` env is derived solely from context (no public constructor can create a target/context env mismatch); the augmentation `GraphTypeNode` arms are retired/`reserved` and relocated to `DeclarationAnalysisGraph`; a session/overlay augmentation query over a base-only `augmentation_index` returns a typed degraded / `ReturnOnly` result and never a silently-stale base-only session-authoritative answer (`session_overlay_augmentation_fails_closed_until_implemented`).

Verification commands:
- `cargo test --package verter_session` module-features / augmentation tests.
- `cargo test --package verter_protocol` typeinfo proto/TS freshness + taxonomy guards (the wire relocation); `cargo test --package verter_session --test g_block typeinfo_graph_taxonomy` and the validation guards.
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate).
- The block's lifted-row proofs via the generated wrapper.
- Full workspace gate (as U0); `node scripts/gen-corpus-audit-tests.mjs` if audit fixtures change.

Docs updated: amend the recovered foundation doc's stale augmentation placements (the §8 exporter `TypeNode::ModuleAugmentation` DTO, the §3 `module_augmentation = 23` / `global_augmentation = 25` type-value arms, the `module_augmentation_is_public_graph_state` guard) → `DeclarationAnalysisGraph` on `TypeInfoGraphPayload.declaration_surfaces` + `SemanticQueryValue::DeclarationAnalysis` (owned by the recovered-doc integration step, not performed here); update the `/type-cache-architecture` skill's `augmentation_index` / `AugmentationTargetKey` notes and the `/type-resolution` merged/ambient-surface notes for the generalized `ResolveDeclarationAugmentation`.

Re-entry notes: idempotent. The wire relocation is a closed-contract change (Typeinfo Wire Contract — schema-version bump, `reserved` tags never reused); regenerate the TS bindings via the workspace `buf`/`oxfmt` and re-run the byte-equal freshness test. If partial, the manifest shows which `module_features` rows remain `#[ignore]`.

---

## U2.JSX_FOUNDATIONS

ID: U2.JSX_FOUNDATIONS
Parent U-block: U2
Subplan: docs/arch/native-typeinfo-parity-u2-reducers.md

Prerequisites: U2.QUERY_VALUE_DOMAIN, U2.RELATION_INFER, U2.INDEXED_ACCESS, U2.UTILITIES, U2.CLASS_SURFACES, U2.MODULE_AUGMENTATION.
Blocked until: all six prerequisites done. JSX resolution reuses ambient-namespace (`JSX.*`), indexed-access (intrinsic element attributes), class-surface (class components), and the type-level factory surfaces (`Parameters<typeof createElement<…>>` / `Parameters<FC<P>>` projections) — the `Parameters` utility intrinsic those factory rows project through dispatches via `IntrinsicRegistry::lookup` in U2.UTILITIES, so this block depends on the utility-reduction substrate being live — it adds NO new keys (parent §8), so it depends on those reducers being live. (U2.UTILITIES does NOT depend on JSX, so this edge introduces no cycle.) This block does NOT consume `ResolveCall` or `CallResolutionBudget`: the two JSX-factory rows are `Parameters<…>` type-surface projections, not real call dispatch. Actual `jsx` / `jsxs` / `createElement` call dispatch is a U6.CALL_RESOLVE backfill guard, not a prerequisite of this block.

Context: JSX parity resolves through the EXISTING query surface — no dedicated JSX query keys and no dedicated JSX `GraphTypeNode` value, keeping the "exactly five added query keys" rule intact (parent §8). `JSX.IntrinsicElements` / `JSX.Element` / `JSX.ElementClass` / the `JSX` namespace resolve through `ResolveAmbientNamespace` + module augmentation + merged declarations; intrinsic element attribute types project through `IndexedAccess` / `KeyOf` over the resolved `JSX.IntrinsicElements` surface; component element types resolve through the normal class (`ResolveClassSurface`) / function surfaces. The two JSX-factory rows are TYPE-SURFACE projections — `Parameters<typeof createElement<…>>[1]` / `Parameters<FC<P>>[0]` over the factory's declared signature via `Parameters` / `Instantiate` — NOT real call dispatch, so this block resolves them WITHOUT `ResolveCall`. Actual `jsx` / `jsxs` / `createElement` call dispatch (and `jsxImportSource` factory invocation) is a U6.CALL_RESOLVE backfill guard, not this block's exit-acceptance. The no-new-key completeness submatrix (`LibraryManagedAttributes`, `ElementAttributesProperty`, `ElementChildrenAttribute`, `IntrinsicAttributes` / `IntrinsicClassAttributes`, the class-component `ElementClass` check, `jsxImportSource` module-namespace resolution) also resolves through the existing surface and lands as `AdditionalProofRow`s (coverage-only; excluded from the 362 count + bijection — parent §8). This block exists now to land JSX parity over the U2 reducer substrate.

Changes (exact files / functions):
- `crates/verter_session/src/project_semantic_dispatch/build.rs` — JSX resolution wiring that dispatches ONLY existing keys: `ResolveAmbientNamespace` for `JSX.*`, `IndexedAccess` / `KeyOf` for intrinsic element attributes (`JSX.IntrinsicElements["div"]`), `ResolveClassSurface` for class components, `Parameters` / `Instantiate` for the type-level factory surfaces (`Parameters<typeof createElement<…>>` / `Parameters<FC<P>>`), `NormalizeIntersection` for `IntrinsicAttributes` / `IntrinsicClassAttributes`. No JSX-specific reducer, no `ResolveJsxIntrinsicElement` / `ResolveJsxAttribute` key, no `TypeNode::JsxIntrinsicElement` value (the recovered-doc stale JSX wording is amended — Cross-reference / doc-update obligations). This block does NOT dispatch `ResolveCall`; `jsx` / `jsxs` / `createElement` call dispatch is a U6.CALL_RESOLVE backfill.
- `crates/verter_session/tests/typeinfo_ignored_test_manifest.rs` (+ `manifest_data/`) — register the six no-new-key submatrix rows as `AdditionalProofRow`s (coverage-only) with their `ProofRequirement`s; a submatrix fixture corresponding to an existing ignored `JsxResolution` row STAYS in that `IgnoredTestRow` (not duplicated — parent §8, PART 2 §10.5).

Deliverables:
- JSX parity resolving entirely through the existing query surface (ambient-namespace / indexed-access / keyof / class-surface / call / normalize-intersection), with no new keys and no dedicated JSX `GraphTypeNode`.
- The six no-new-key submatrix rows as `AdditionalProofRow`s (coverage-only).

Legacy deletions:
- Any JSX-specific resolution engine / `ResolveJsxIntrinsicElement` / `ResolveJsxAttribute` key / `TypeNode::JsxIntrinsicElement` value (none is introduced; the recovered-doc stale JSX wording is amended to the existing-query mechanism).
- No projection-repair path remains for JSX.

SemanticQueryKey/facts touched: `ResolveAmbientNamespace`, `IndexedAccess`, `KeyOf`, `ResolveClassSurface`, `NormalizeIntersection`, `Instantiate` (the `LibraryManagedAttributes<C,P>` application + the `Parameters<typeof createElement<…>>` / `Parameters<FC<P>>` type-level factory projections) — all EXISTING keys (no JSX key); this block does NOT consume `ResolveCall` (the type-level factory rows are `Parameters<…>` projections, not call dispatch; `jsx`/`jsxs`/`createElement` call dispatch is a U6.CALL_RESOLVE backfill). Facts read: `Member` / `MemberPresence`, `ModuleAugmentation` / `AmbientGlobal` (JSX namespace augmentation), `LibIntrinsic`, project-generation facts. Admission: inherits the budgets of the dispatched keys (`KeyspaceBudget` for attribute projection); no `CallResolutionBudget` (no call dispatch in this block).

Exact test rows lifted (capability `JsxResolution`, `jsx.rs`):
- jsx.rs::jsx_intrinsic_div_resolves_to_declared_shape
- jsx.rs::jsx_intrinsic_span_resolves_to_declared_shape
- jsx.rs::jsx_factory_inferred_props_for_component_resolves
- jsx.rs::jsx_fc_props_includes_children_optional
- jsx.rs::jsx_intrinsic_via_generic_lookup_div_resolves_to_div_shape
- jsx.rs::jsx_intrinsic_keys_resolves_to_string_literal_union
- jsx.rs::jsx_intrinsic_via_generic_lookup_span_resolves_to_span_shape
- jsx.rs::jsx_intrinsic_augmented_custom_card_resolves_to_declared_shape
- jsx.rs::jsx_element_resolves_to_declared_interface_shape

Additional coverage rows (`AdditionalProofRow`s — coverage-only, NOT in the 362 count/bijection — parent §8): the six no-new-key submatrix rows (`JSX.LibraryManagedAttributes<C,P>`, `JSX.ElementAttributesProperty`, `JSX.ElementChildrenAttribute`, `JSX.IntrinsicAttributes` / `IntrinsicClassAttributes<C>`, the class-component `JSX.ElementClass` check, `jsxImportSource` module-namespace resolution), each with its named guard and `ProofRequirement`.

Required new guards (parent §8):
- `jsx_resolution_uses_existing_semantic_queries`
- `jsx_intrinsic_elements_project_via_indexed_access`
- `jsx_no_dedicated_graph_type_node`
- `jsx_library_managed_attributes_via_ambient_namespace_and_indexed_access`
- `jsx_element_attributes_property_via_ambient_namespace_keyof`
- `jsx_element_children_attribute_via_ambient_namespace_keyof`
- `jsx_intrinsic_attributes_via_ambient_namespace_intersection`
- `jsx_element_class_check_via_resolve_class_surface_and_relate`
- `jsx_import_source_module_namespace_via_existing_resolution`

Critical-rule guards: implements the parent's `(CRITICAL)` "JSX resolution — no new query keys" rule; the nine JSX guards above are its R6 guards (`jsx_resolution_uses_existing_semantic_queries` / `jsx_no_dedicated_graph_type_node` are the load-bearing no-new-key guards).

Proof requirement: per-row — the nine `IgnoredTestRow`s are TS7-oracle-pinned (`Ts7Oracle`) for the resolved JSX shapes; the six `AdditionalProofRow`s are `OracleAndGuard` (oracle + the named existing-query JSX guard). Consumed by each row's / additional-row's generated wrapper.

Exit acceptance: all 9 `jsx.rs` rows lift and pass; the six submatrix `AdditionalProofRow`s pass; JSX resolves entirely through existing keys (no JSX key, no JSX `GraphTypeNode`) — all nine JSX guards green; the binding 362 `IgnoredTestRow` total is unchanged (the submatrix rows are coverage-only).

Verification commands:
- `cargo test --package verter_session` jsx tests.
- `cargo test --package verter_session --test typeinfo_ignored_test_manifest` (coverage gate + the `AdditionalProofRow` non-participation in the count/bijection).
- The block's lifted-row + additional-row proofs via the generated wrapper.
- Full workspace gate (as U0).

Docs updated: update the `/type-resolution` skill's JSX-resolution notes (JSX resolves through the existing query surface — `ResolveAmbientNamespace` / `IndexedAccess` / `KeyOf` / `ResolveClassSurface` / `Parameters` / `Instantiate` / `NormalizeIntersection`; the JSX-factory rows are `Parameters<…>` type-surface, not `ResolveCall` dispatch — `jsx`/`jsxs`/`createElement` call dispatch is a U6.CALL_RESOLVE backfill; no new JSX key, no JSX `GraphTypeNode`); amend the recovered foundation doc's stale `ResolveJsxIntrinsicElement` / `ResolveJsxAttribute` / `TypeNode::JsxIntrinsicElement` wording → the existing-query mechanism (owned by the recovered-doc integration step, not performed here).

Re-entry notes: idempotent. JSX adds NO keys — if a JSX key or JSX `GraphTypeNode` appears, `jsx_resolution_uses_existing_semantic_queries` / `jsx_no_dedicated_graph_type_node` fail. If partial, the manifest shows which `jsx` rows remain `#[ignore]`.

---

## Row-level-split capabilities (ownership note)

Several capability classes are row-level split across U2 and U6/U10 (Capability
Map — "Owning U-block" reads `row-level U2/U6` or `U2/U10`). This subplan's U2
blocks own the rows whose lifting MECHANISM is a U2 reducer; the authoritative
`row → block_id` partition over all 362 rows is §10.4.1 of the parent (the per-block
`Exact test rows lifted` lists are its projection). The remaining rows lift in U6
(flow / call / inference) or U10 (mode / demand / expansion exactness). The split,
stated explicitly so no row is double-counted:

- **`TypeParameterFeatures` (17)** — the two const-type-param rows
  (`const_type_param_route_call_preserves_readonly_tuple_with_literal_paths`,
  `const_type_param_string_call_preserves_readonly_literal_string_tuple`) lift in
  **U6.CALL_RESOLVE**: they are genuine call expressions that apply the TS7
  `<const T>` modifier when inferring `T` from a call-site array argument, so their
  dominant mechanism is `ResolveCall` (call dispatch + argument inference), not a U2
  reducer; they are LISTED in `U6.CALL_RESOLVE`. The two pure-substitution /
  recursive-generic substitution rows
  (`substitution_types_sb14_default_type_arg_ignored_by_return_type`,
  `substitution_types_sb15_recursive_generic_substitution`) lift in
  **U2.CLASS_SURFACES** (substitution is a reducer concern carried on the keys —
  PART 1 Shallow-File invariant: "generic substitutions are part of semantic
  meaning"; they ride the `ResolveClassSurface` / `Instantiate` substitution
  identity; both are LISTED in `U2.CLASS_SURFACES`). The two generic-predicate
  rows (`substitution_types_sb09_asserts_x_is_string_on_generic`,
  `substitution_types_sb10_x_is_t_predicate_on_generic`) lift in
  **U6.PREDICATE_ASSERTION**. The eleven flow-narrowing-of-generic rows
  (`substitution_types_sb01`–`sb08`/`sb11`–`sb13` — bare / constrained-generic
  narrowing, compound `typeof`+`instanceof`, un-narrowing on reassignment,
  `in`-operator, truthiness, destructure correlation, constraint-flow apparent
  access) lift in **U6.NARROW_SUBSTITUTION**. §10.4.1 assigns each `substitution_types_*` /
  `const_type_param_*` row to exactly one block by mechanism.
- **`TypeScriptRules` (11)** — the rows whose mechanism is `KeyOf` /
  `IndexedAccess` (`keyof_materializes_literal_key_union`,
  `indexed_access_reduces_terminal_property`, `tuple_rest_element_resolves_array_element_type`)
  lift in **U2.INDEXED_ACCESS**; the distributive-conditional /
  template-intrinsic / key-remap rows
  (`distributive_conditional_expands_each_union_arm`,
  `template_intrinsic_evaluates_union`, `key_remap_exclude_filters_and_renames_keys`)
  lift in **U2.RELATION_INFER** / **U2.MAPPED_TEMPLATE**; the
  constructor-utility / instance-type / class-instance / `typeof const` /
  `Awaited` rows lift in **U2.CLASS_SURFACES** / **U2.UTILITIES** by mechanism.
  Every `typescript_rules_*` row maps to exactly one U2 block in the coverage
  table.
- **`ApparentTypes` (20)** — the `apparent_types_ap01`–`ap15` (15) and `branded_*`
  (5) rows ALL lift in **U2.CLASS_SURFACES** (`ApparentType` + brand identity, listed
  there). (The constraint-flow apparent row
  `substitution_types_sb07_constraint_flow_apparent_type` is classified under
  `TypeParameterFeatures`, not `ApparentTypes`; it depends on flow and lifts in
  **U6.NARROW_SUBSTITUTION** — see the `TypeParameterFeatures` bullet.)
- **`ClassFeatures` (13)** — all 13 (the 9 `class_features_*` + 4 `decorators.rs`
  rows) lift in **U2.CLASS_SURFACES** (listed there).
- **`ModernTsFeatures` (6)** — split by mechanism:
  `satisfies_array_literal_widens_to_primitive_array` lifts in **U2.RELATION_INFER**
  (`satisfies` target-contextual validation + array-literal widening — parent §4.4);
  `satisfies_widens_inner_value_to_primitive_without_as_const` lifts in
  **U6.VALUE_INFERENCE** (the `satisfies` widening on the flow return path);
  `variance_annotation_in_substitution_through_consumer_consume_parameters` lifts in
  **U2.CLASS_SURFACES** (`Parameters<…>` under variance); the two `import_attribute_*`
  rows lift in **U2.MODULE_AUGMENTATION** (simulated-JSON `typeof import` shapes); and
  `await_using_simulated_return_type_resolves_to_primitive` lifts in
  **U6.ASYNC_GENERATOR** (the `await` / `Awaited` carrier). §10.4.1 assigns each
  `modern_ts_features_*` row.
- **`ExpansionBoundaries` (6)** / **`DemandBoundary` (3)** / **`ModeBoundary`
  (5)** — these are mode / demand / expansion-EXACTNESS rows (PART 1 §10.4 maps
  them to `Instantiate` / `IndexedAccess` / `KeyOf` / `MappedType` / `Conditional`
  mode-boundary mechanisms). The pure-reducer mode/demand rows (e.g.
  `expansion_pick_does_not_load_unpicked_imports`,
  `mode_boundary_identity_does_not_materialize_alias_body`,
  `demand_boundary_terminal_projection_resolves_value_without_unused_branch`) are
  exercised by the U2 reducers but their EXACTNESS gating is owned by **U10**
  (mode/demand/budget exactness — Capability Map "U2/U10"); they lift with U10
  against the U2 reducer substrate. This subplan's U2 reducers must satisfy them
  (the U2 reducers are path-precise and member-demand aware), but the rows'
  coverage `block_id` is the U10 block, not a U2 block.

In every case the parent's §10.4.1 partition (the generated coverage table, PART 2
§10.4) is the authority: each row maps to exactly one `block_id` via its
`mechanism_id`, and `capability_rows_map_to_expected_query_fact_mechanisms` asserts
the mapping is consistent with the capability. This subplan's U2 blocks own only the
rows whose coverage `block_id` is a U2 block listed above; no row is owned twice, and
the binding 362 total stays exact.

---

## Verification (whole-subplan)

Every block runs the full workspace gate as its CI gate (PART 2 §§11.2, 14) — the
complete Rust **AND** JavaScript gate, green only when BOTH pass:
`cargo test --workspace --tests`, `cargo clippy --workspace -- -D warnings`,
`cargo fmt --all --check`, `pnpm test`, and `pnpm install --frozen-lockfile`. A block
reaches `Lifted` + a merged `Typeinfo-Block:` trailer only after green CI over the
branch content AND the three-reviewer LAND verdict (1 Claude Code + 2 codex; PART 2
§11.12), via the git/CI landing protocol — branch per block → green CI → three-reviewer
LAND → squash-merge with the `Typeinfo-Block:` trailer (PART 2 §§11.2–11.4); the block's
WIP series squash-merges to ONE target-branch commit (PART 2 §11.11). The parent U2
token is the aggregate over every block above and is done only when every row in the
union of all U2-block row-sets is `Lifted` (PART 2 §11.9) — never vacuously. Downstream
U3 / U8 / U10 / U11 / U13 stay blocked until the whole U2 parent is done.

The whole-subplan parity guarantee is the parent's composition (Capability Map →
"The guarantee over the 362 rows"): the two-table ledger with the exact-362 count
+ bijection (PART 2 §§10.1, 10.5); the U0 row-exact coverage table that DEFINES
completeness (PART 2 §10.4); the per-row executable `ProofRequirement` with the
generated proof registry + row-test wrapper (PART 2 §§10.2–10.3); the git/CI landing
protocol (PART 2 §11); the no-skip guarantee (PART 2 §12); and the git/manifest-driven,
parallel-safe resume protocol (PART 2 §14). U0 builds the ledger/coverage substrate; the
U2 blocks lift their exact manifest rows through it, landing each via its own branch.
