> **SUPERSEDED — HISTORICAL EVIDENCE, NO NORMATIVE FORCE.**
> The single normative artifact for Stage 2 is
> `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-25-C1-STAGE2-CUTOVER.md`.
> This document and its annex are retained only as the record of how that ruling was reached.
> Do not execute, cite as authority, or repair anything below; in particular the two-document
> override structure this file participates in was deleted rather than repaired.

# Stage 2 — atomic module-resolver cutover: execution plan

**Status: PROPOSED — NOT AUTHORITY YET.** This document is the separately ratifiable,
digest-bindable execution plan the sequencing record
(`docs/arch/refactor/rev11/evidence/C1/sequencing.md`, closing paragraph) requires
before the irreversible atomic cutover may execute. It is promoted from that record's item 5
migration/deletion table and the ratified answers in the F23/F24/F25/F26 consults; it does not
re-open any of them.

**READ WITH THE ANNEX — this document is incomplete alone.**
`stage2-preflight-annex.md` REPLACES §1's re-pin mechanism (which compared against the SHA it was
checking and could not detect drift) and REPLACES §2's inventory with a tree-derived one that is
explicitly NON-EXHAUSTIVE: a static name sweep cannot see a type reached by inference or forwarded
through a glob re-export, so it can size the work but never bound it. §4's abort conditions stand
unamended; AB-1 fires off the annex's live-ref comparison. §5 step 0 is bound by the annex's
re-ratification rule — an inventory difference stops execution rather than being amended in place.
**Completeness is not a precondition of this plan.** It is proven at the end by the annex's §A5
build gate; a missed reference is a compile error in a scratch tree, recoverable by §4's reset, and
costs rework rather than damage.

**How this becomes authority is UNRESOLVED, and this plan no longer claims otherwise.** An earlier
revision stated that architecture-seat ratification plus a `[[document]]` row in
`authority-registry.toml` would activate it. That registration is not available to this file:
`scripts/validate-program-state.mjs:2234` admits only `CHARTER`, `AMENDMENT` and `RULING` kinds and
`:2236` confines each to a subdirectory of `docs/arch/refactor/rev11/`, while this plan lives under
`docs/arch/refactor/rev11/evidence/C1/`. A document row would also be insufficient on its own — a
block that has left `LOCKED` requires exactly one `[[authorization]]` record as well.

That is governance infrastructure owned outside C1 and is escalated there. Until it resolves, this
plan is a design pending ratification with **no recorded activation path**, and no Stage-2 work may
be dispatched against it. This branch carries no authority-registry delta at all.

**The seven `wip(core)` commits already on this branch are unapproved scratch, not dispatch
authority.** They are additive semantic-owned value modules (step 1 only: no deletion, no caller
repointing, no Cargo-edge change, no guard flip). They confer no permission to continue, are
superseded by this plan's work order, and are squashed out at landing.

**Predecessor gate unchanged.** C1 remains gated on `CM1` (`REVIEW`, all three review mandates
`BLOCKING`). Ratifying this plan does not release that gate and nothing here lands while it holds.

---

## 1. Exact rebased baseline (BINDING)

| Field | Value |
|---|---|
| Baseline commit (trunk) | `f593b24c8a2a53b5496d85ee4de1bab0dafe61d1` |
| Baseline subject | `docs(*): exempt one legacy dispatch-record gap and bind its charter digest` |
| Branch | `block/module-resolver-core` |
| Rebase performed | branch replayed onto that exact commit; **0 commits behind**, zero conflicts |
| Authority registry on branch | blob `04da165b78d4905b6256952a72665d055b2fd383` — byte-identical to the baseline's, inherited, not edited |

"Current trunk" is **not** an acceptable baseline: trunk moves. Execution binds the literal SHA
above.

**Re-pin rule.** Immediately before Stage-2 execution begins, re-verify
`git rev-list --count <branch>..<baseline>` is `0`. If it is not, rebase onto the then-current
trunk commit, record that new SHA here, and re-ratify this section before starting. A Stage-2
cutover started against a stale baseline is aborted under §4.

**Pre-start tag (mandatory).** Before step 1 of §5, tag the branch tip
`c1-stage2-prestart`. That tag is the known-good state §4's abort procedure returns to.

## 2. Caller and deletion inventory (BINDING)

Consolidated from item 5's table plus the F22/F23/F25/F26 re-greps. Nine crates are touched.
The inventory is **re-run immediately before the move** (§5 step 0) — it is a starting point that
must be re-greped, never treated as permanently current.

### 2a. Callers to repoint

| Crate | Sites |
|---|---|
| `verter_workspace` | `engine.rs:3298,3330,3633,3790,3857,3921` (six `WorkspaceSnapshot.resolver` uses); `resolution_currency.rs:1500`; `resolution_currency::evaluate_selected_context` → `nearest_config_for_path`; `snapshot_builder.rs` ×3; `ProjectGraph::to_project_resolver`; `Engine::rebuild_and_publish` |
| `verter_lsp` | ten production files borrowing/cloning `WorkspaceSnapshot.resolver` or the shim: `server/mod.rs` (`ServerState.resolver`), `server_utils.rs`, `background_drain.rs`, `workspace_scanner.rs`, `sync_coordinator.rs`, `background_drain_decl_closure.rs`, `server/provider_state.rs`, `background_drain_owner_loss.rs`, `server/sync_orchestration.rs`, `external_ts/carrier_sync.rs` (`CarrierSyncRequest.resolver`); plus `config.rs`, `provider_sync.rs`, `carrier_provider_projection.rs`; constructor `ProjectRegistry::to_native_project_resolver` (`config.rs:832-838`) |
| `verter_napi` | `lib.rs:2102,2124` (real analysis fns, path unchanged); DTO consumers `meta.rs`, `lib.rs` ×3 |
| `verter_wasm` | `lib.rs:640,667` (real analysis fns, path unchanged) |
| `verter_session` | DTO consumers `component_meta_host.rs`, `host_lifecycle.rs`, `meta.rs`; path/carrier-helper consumers |
| `verter_tsc` | `checker.rs:1861,1930` (`is_relative_specifier`) |
| `verter_mcp` | path/carrier-helper consumers |
| `verter_semantic` | inert kernel repointed so production semantic source names **zero** workspace types |
| `verter_identity` | guard cluster, §3 |

Path/carrier helpers with their own multi-file consumer lists (full detail in the F22 evidence
file): `is_relative_specifier`, `collapse_path`, `normalize_canonical_id`, `path_is_carrier`,
`carrier_ide_provider_path`, `carrier_api_provider_path`, `carrier_source_extensions`,
`strip_carrier_extension`.

`server_utils`'s intentionally-unused `_resolver` parameters are **deleted with their call-site
arguments**, not mechanically retyped.

### 2b. Value closure to relocate

15 core DTOs (`ProjectOwnership`, `ResolveRequestKind`, `ResolvePhase`, `ResolutionContext`,
`ProviderTarget`, `ResolutionKind`, `ResolveRequest`, `ResolveResult`, and the project/config
closure `WorkspaceAlias`, `IdeProjectCompilerOptions`, `IdeProjectConfig`, `ConfiguredMembership`,
`StaticMembershipSpec`, `CompiledGlob`, `NormalizedGlob`); the env-hash closure
(`IdeProjectConfig`'s env-hash methods + `project_identity`, `EnvHashInputs`,
`ModuleResolutionMode`, `ConditionSet`, `SpecifierKind`) using semantic's dependency-neutral
`Hash16`; F25's five items (`FactVersionRef` + F26's corrected full immutable value graph,
`ProjectStableKey`/`AmbientSymbolHit`, `PathProbe`, `WorkspaceAuthorityId`/`ResolutionPopulation`/
`ResolutionWorldId`, and the `DirEntry` → `RouteDirEntry` **mirror**, the single exception to MOVE).
`ProjectMembership` stays workspace-owned with no semantic re-export. Cache **authority** —
validators, read sets, admission, mutation propagation, counters, compaction, replay ledgers,
publication, invalidation, `CANDIDATE_CAP` — stays workspace/session-owned; only vocabulary moves.
`ProjectStableKey::from_project` becomes a workspace free function (same-crate inherent-impl
constraint).

### 2c. Deletions (all in the same transition)

`verter_workspace::resolver` module and its re-exports (`lib.rs:103,183-188`) — **no forwarding
`ProjectResolver`/`NativeProjectResolver` alias**; `ProjectResolver` itself; the private
`preferred_specifier`; `resolver.rs::test_support::legacy_resolve_with_reader` and the
`legacy_resolve_for_project_with_reader` / `legacy_preferred_specifier_candidates` /
`legacy_project_exact_result` bridges; `resolution_dual_runner_tests.rs`; `verter_lsp`'s
`project_resolver.rs` shim; `verter_semantic`'s `verter_workspace` normal dep **and** its
`test-support` dev-dep; the A5-DD1 exception row.

Retained/retargeted: `resolution_witness_contract_tests.rs` **preserved** as public-boundary
characterization; `resolver_tests.rs` (3,929 lines) moved/parameterized around attempt views; the
`raw_resolver_entry_points_are_private` compile-fail fixture **retargeted** to the new private
attempt boundary; `verter_semantic::analysis::project_resolver`'s two real functions stay at their
current path (napi/wasm callers unchanged), only the `:1-30` re-export half is deleted.

A transitional forwarding wrapper is **explicitly rejected** — it is a compatibility shim CLAUDE.md
forbids, leaves two public authorities, and produces a Cargo cycle. Encountering a caller that
cannot be repointed without one is an abort (§4).

## 3. Cargo-edge reversal and `verter_identity` guard transition (BINDING)

Both happen in the **same** commit as the code. Landing them separately is unsafe in either order:
code-first leaves the guard too permissive; guard-first false-fails.

**Cargo edges.** Add `verter_workspace --production--> verter_semantic`. Remove
`verter_semantic --> verter_workspace` (normal dep at `crates/verter_semantic/Cargo.toml:24-27`
and the `test-support` dev-dep, the latter once the dual runner is deleted). Final direction:
one edge, workspace → semantic, zero edge back. The reversal cannot be attempted while any
production `verter_semantic` reference to a workspace name survives — that is a Cargo SCC cycle.

**Guard cluster** — `crates/verter_identity/tests/cases/workspace_dependency_layers.rs`, all three
edits together:

| Site | Edit |
|---|---|
| `ratified_upward_exceptions()` (~:145) | remove the `verter_semantic` entry; **keep** `verter_diagnostics` (independent, unrelated reason) |
| `RATIFIED_ROOT_CRATES` (~:335) | shrink to `&["verter_diagnostics"]` |
| semantic→workspace canary (~:424-430) | replace with a BOTH-DIRECTIONS assertion: semantic's production closure must NOT reach workspace; workspace's production closure MUST reach semantic |

The Cargo-metadata production-closure gate is the final verification (C1-AC-2). No source scanner
substitutes for it.

## 4. Atomic abort conditions and return-to-known-good (BINDING)

The cutover is one unlanded transition. Every intermediate state is working-tree or squashed-out
scratch; **nothing intermediate ever lands as its own commit**. Therefore abort is always
recoverable and always the same procedure.

**Abort procedure.** Stop editing; do not commit; do not push; `git reset --hard c1-stage2-prestart`
and discard the working tree. Nothing landed, so nothing is reverted and no trunk state is touched.
Record the trigger and the evidence in this ledger directory before any retry. A retry re-runs §5
step 0 from a freshly re-pinned baseline (§1).

**States that force abort:**

| # | Trigger |
|---|---|
| AB-1 | The baseline re-pin (§1) shows the branch is behind trunk when execution starts. |
| AB-2 | A both-edges Cargo state is committed, or Stage 2 proceeds to the final squashed candidate while any production `verter_semantic` reference to a `verter_workspace` name remains. An uncommitted red edge-reversal/build iteration is §A5 discovery and does not trigger AB-2. |
| AB-3 | The dual-runner harness shows ANY legacy-vs-kernel divergence — final `source_id`, ordered consumed selectors, `NeedInputs` wave shape, recovery scopes, replayed `ResolutionFactKey` set/signature, or the complete `ResolveResult` DTO. The harness is deleted only after it is green in the same working tree, never before. |
| AB-4 | F24's five replay/failure contracts (manifest fingerprint `name` preservation, `DirectoryMembers` consumed-vs-prefetched, complete fact replay/signature, basis-restart on the real driver, no-progress/terminal/transient-load-failure behavior) are not all satisfied by the real driver before callers are repointed. |
| AB-5 | A caller cannot be repointed without a forwarding wrapper, alias, feature flag, or dual path. |
| AB-6 | Code and guard flip would have to land in separate commits, or any candidate landed state would contain two production module resolvers or a superseded wrapper. |
| AB-7 | The charter's own Abort/rescope triggers fire: a fourth production lifecycle; `ProjectResolver` proving not cleanly separable from scheduler-integrated loading; a second query-time resolution path; an unexplained `A6_META_COMPILE_40_COLD_RUST` regression. These reopen the ruling — a second architecture challenge, never a quiet local substitution. |
| AB-8 | No gate slot is available. Steps 3-7 of §5 are not verifiable without a full workspace build; running one opportunistically on a loaded host is itself an abort trigger, not a shortcut. |

## 5. Work order (from item 5 / F25, with a re-grep step prepended)

0. **Re-run the §2 inventory against the live tree** and re-pin §1's baseline. This is the last
   preparation step; its output amends §2 in place before any edit.
1. Establish all semantic-owned values (§2b), workspace projections, and workspace value
   re-exports.
2. Repoint the inert kernel so production semantic code contains zero workspace names.
3. Reverse both Cargo edges and flip the complete `verter_identity` guard cluster together (§3).
4. Build the real workspace retry/replay driver and satisfy F24's five contracts (AB-4).
5. Repoint every production caller to `ModuleResolverCore` (§2a).
6. In that same unlanded transition, delete `ProjectResolver`, aliases/wrappers, bridges, the
   dual-runner harness, and obsolete tests (§2c).
7. Verify zero production/dev `semantic → workspace` edge, the positive `workspace → semantic`
   edge, authority uniqueness, and the full gate; then create the single final squashed commit.

WIP commits during execution are sanctioned provided they are squashed before landing. The landing
squash also resolves this branch's accumulated commit-message hygiene (program vocabulary across
its subjects and bodies; the non-approved `wip` type) — audit bodies, not only subjects, and scope
any vocabulary grep to this block's own commits and files.

## 6. Acceptance

Stage 2 discharges charter acceptance IDs C1-AC-2 (closure guard, exception row deleted not
widened), C1-AC-9 (no direct scheduler/tsgo I/O left unconverted in the relocated resolver), and
contributes to C1-AC-3 (authority uniqueness after the move). It does not by itself discharge
C1-AC-1/4/5/6/7; those remain the surrounding block's. Per the charter's controlling sentence,
`ARCH-ADDENDUM-C1-THREE-GAPS.md` governs where it conflicts — in particular the GAP 2 per-module
move/stay/split dispositions, which this plan implements rather than a literal directory
relocation.

**Cited authority documents, and where they live.** Every ruling this plan or its annex cites is
**trunk-resident** under `docs/arch/refactor/rev11/rulings/`, not authored on this branch, and all
are present and git-tracked here — verified: `ARCH-ADDENDUM-C1-THREE-GAPS.md`,
`ARCH-RULING-C1-FOUR-FORKS.md`, `ARCH-RULING-C1-D1-FLOW-FILE-RECONCILIATION.md` and
`ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`. The last of those entered via trunk commit
`961c16271a`, inherited through the merge-base, so a reviewer reading only the two ledger documents
in isolation may not see it while a reader on this branch will. Citations here are to trunk-resident
paths; none is a branch-local artifact.
