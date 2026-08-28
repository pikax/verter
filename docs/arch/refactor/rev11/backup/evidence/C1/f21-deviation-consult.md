# C1 twenty-first deviation — F21: real atomic-cutover scoping (items 1/2/5), first next step redirected to C1-AC-8

Found while scoping the real `ModuleResolverCore` production port: item
1's inventory is stale AGAIN (this session's own `ResolutionBasis` work
added `ResolutionPopulation`/`ResolutionWorldId`/`WorkspaceAuthorityId`
references not in F18's snapshot), the Cargo-edge-deletion ordering
relative to the resolver relocation was never settled, and item 2's
3-field sketch was never checked against the FULL algorithm's owned
surface. Given the scale and irreversibility of the coming atomic
cutover (CLAUDE.md: "delete the superseded code in the same change...
no shims, no dual paths"; "execute approved plans fully in one pass"),
this needed ratifying before dispatching any implementation. Full
consult prompt/output: `/tmp/c1-item2-5-atomic-cutover-scoping-prompt.md`
/ `/tmp/c1-item2-5-atomic-cutover-scoping-output.md` (not committed —
ephemeral scratch; this file plus the rewritten sequencing record are
the durable record).

## Bottom line

"The Cargo-edge deletion is part of the final atomic production cutover.
A transitional `verter_workspace::ProjectResolver` forwarding wrapper is
neither compliant nor technically useful. Preparatory changes may land
first, but every landed state must have exactly one production
module-resolution authority."

## 1. Current crate-wide inventory — item 1 is stale again

40 production lines, **8** distinct types/traits (not F18's 5): `FactVersionRef`,
`ProjectStableKey`, `AmbientSymbolHit`, `PathProbe`, `WorkspaceAuthorityId`,
`ResolutionPopulation`, `ResolutionWorldId`, `WorkspaceRead` — plus the
resolver shim (23 re-exported symbols) and the fact-registry wildcard (16
public value types). `analysis/routes.rs`'s six `&dyn WorkspaceRead`
parameters (C1-AC-8) are STILL outstanding, unrelated to this session's
resolver-core work. Two Cargo entries (production + test-support dev
edge). `SessionFingerprint` is transitive through `ResolutionPopulation`,
not a direct reference. Workspace `PackageManifest` is NOT a current
production reference (the observation API already uses semantic-owned
`ResolutionPackageManifest`; only docs mention it). `CapturedResolutionWorld`
need not relocate — it can stay sealed and workspace-owned while
projecting exact semantic-owned basis/replay DTOs.

## 2. Cargo-edge ordering — MUST be the same final cutover, not later

Explicit authority: C1-AC-2 (production closure + exception row must
disappear), C1-AC-8 (all six `WorkspaceRead` params must disappear), the
charter's convergence map ("delete the Cargo.toml edge outright"), the
required-exit paragraph's zero-upward-closure requirement, and the
authorization record itself (names both the resolver relocation AND edge
deletion together).

**Mechanical forcing function, not just policy**: once `WorkspaceSnapshot`
stores `ModuleResolverCore` (semantic-owned) instead of `ProjectResolver`,
`verter_workspace` must depend DOWNWARD on `verter_semantic` — the OLD
`verter_semantic -> verter_workspace` edge cannot coexist with that
(a literal Cargo cycle). The edge deletion is therefore forced by the
relocation itself, not a separable follow-up.

"If closing the edge proves impossible without invading F1, the
charter's response is abort/rescope, not a tracked post-C1
dependency-debt follow-up."

Remove BOTH the normal and dev-dependency `verter_workspace` edges (the
dev edge existed only for the `test-support` feature backing this
session's `ResolutionBasis`/dual-runner test infrastructure and the
now-planned-for-deletion `test_support` bridge/dual-runner module).

**Governance wrinkle, not an architecture fork**: `authority-registry.toml`
pins C1-CHARTER at `ff25fdce...`, but `docs/arch/refactor/rev11/charters/
C1.md`'s CURRENT hash is `0367b175...` (independently verified via
`sha256sum`). Traced to trunk commit `4107210c7` ("fix(gate): grandfather
three legacy digest gaps and pin the rehearsal shortcut") — confirmed via
`git show` that its ONLY edit to C1.md updates stale predecessor/
sequencing status prose (A6/B1/B2/CM1 acceptance state at that point in
time), no scope-clause change. Must be reconciled before the final
cutover per the authority model's "registered hashes bind exact current
bytes," but is a mechanical re-pin, not a re-ratification — the
underlying scope ruling is unaffected. NOT fixed this round: recomputing/
rewriting a MAINTAINER-ratified digest pin sits at the implementer/
program-orchestrator authority boundary this whole engagement has
consistently respected (I resolved a REBASE CONFLICT in this same file
this round by preserving both sides' content, which is different from
re-attesting a changed hash) — flagged here for the orchestrator/
maintainer rather than silently self-authorized. Also flagged: the live
governance validator's TOML parser trips on escaped quotes at
authority-registry.toml's own C1 authorization row (line 584) — a
validator defect, not an architecture question.

## 3. Atomic versus staged — stage PREPARATION, never the algorithm itself

Two-stage model:
- **Pre-cutover preparation** (may land now, in this and future rounds):
  refresh item 1/2/5's inventory; fix the authority digest; convert
  `RouteAnalysisInputs` (C1-AC-8); add black-box characterization and
  migration guards. Invariant: `verter_workspace::ProjectResolver`
  remains the ONLY production module resolver throughout.
- **Final atomic cutover** (one landed state, not staged): move the
  complete algorithm and value closure; wire retry/replay; repoint EVERY
  caller; reverse the Cargo edge; delete the old resolver, shims, test
  bridge, and dual-runner harness; remove the A5-DD1 exception row.
  Invariant: `verter_semantic::ModuleResolverCore` becomes the ONLY
  production module resolver, in one transition.

**Explicitly REJECTED**: a transitional form where `verter_workspace`
delegates through a thin forwarding `ProjectResolver` wrapper around the
new `ModuleResolverCore`. It is a compatibility shim forbidden by
CLAUDE.md; it leaves two public authorities/names; it produces a Cargo
cycle before the edge is removed; and it serves no purpose after the edge
is removed (callers can and must be repointed directly).

Scale confirmed real: the full resolver covers three entry shapes,
project selection, aliases, tsconfig paths/`baseUrl`, ordered
project-reference recursion, `#imports`, node_modules (package
exports/conditions), provider projection, and preferred-specifier
generation. Item 5's "~14" undercounts — physical call-site occurrences
are already higher than 14 migration surfaces.

"Atomic" means no landed production state ever has two engines or a
superseded wrapper — not that all documentation, characterization, and
unrelated edge cleanup must land in one working commit.

## 4. Item 2's 3-field sketch — storage concept fine, write-up incomplete

The `{ configs, by_tsconfig, reference_edges }` sketch is plausibly
sufficient PERSISTENT STATE (aliases/`base_url`/paths/compiler-options
live in each config; project references use the compiled edges; package
imports/exports and node_modules walks are request-local algorithms;
manifests/probes/realpaths come through `ResolverObservation`; provider
projection computes from the target config) — do NOT add a workspace
reader, manifest cache, filesystem handle, transaction, or package index
to the core.

What item 2's write-up currently lacks (a documentation-completeness gap,
not a structural redesign): the full owned API surface — `resolve_attempt`,
`resolve_for_project_attempt`, importer/target owner selection, the FULL
relative/absolute + alias + paths/`baseUrl` + project-reference +
imports/exports + node_modules + legacy-package fallthrough,
`preferred_specifier_candidates`, exact-result and provider-graph
projection, carrier/provider path helpers, request/result and
project-selection DTOs, `KernelAttempt`/`AttemptOutput` witness behavior
for EVERY branch (not just the dual-runner's narrow slice), precise
semantic ownership for `IdeProjectConfig`/membership/glob values/env-hash
methods and their mode/condition-set inputs, explicit dispositions for
`FactVersionRef`/resolution-identity DTOs/ambient DTOs/`PathProbe`, and
canonical public module paths plus an explicit decision to DELETE
`NativeProjectResolver` (not preserve it as a compatibility alias).

## 5. First next implementation step — redirected to C1-AC-8, NOT the resolver

**The next production code unit is the complete C1-AC-8
`RouteAnalysisInputs` conversion** — NOT the resolver algorithm at all.
Scoped as one closed unit: introduce a semantic-owned immutable
`RouteAnalysisInputs` covering the exact file contents/existence/kind
answers/directory entries route analysis consumes; convert all six
`WorkspaceRead` parameters in `analysis/routes.rs`; move snapshot
construction to the higher-level LSP/MCP orchestration callers; retarget
the existing filesystem-backed route tests to the immutable input
snapshot, preserving every assertion; add a structural assertion that
production `verter_semantic` contains no `WorkspaceRead` reference; re-run
the crate-wide inventory (this unit should remove all six route
references without touching the resolver algorithm at all).

This is a genuine, independently reviewable, completed production
conversion that leaves the sole resolver authority unchanged and reduces
the final dependency-reversal cutover's remaining surface. Before
dispatching that code unit: repair the C1 authority digest/parser issue
(flagged for the orchestrator, not self-authorized this round) and
update item 1/2/5 with this refreshed inventory (this document +
sequencing-record rewrite). After the route unit lands, the next work is
final-cutover PREFLIGHT — not another inert kernel slice, not a
forwarding resolver.

## Explicit instruction, followed

"No files changed" (an investigation-only consult). This round's next
action: rewrite the sequencing record's items 1/2/5 with this refreshed
inventory and staging model, flag the digest issue for the orchestrator,
then — capacity permitting — start the C1-AC-8 `RouteAnalysisInputs`
conversion as the next production code unit.
