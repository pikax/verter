<!-- unified-charter-v2
id=CVO3
name=Critical false-green controls for validation machinery
phase=compiler
train=compiler.validation-observability
product=validation_observability
kind=verification
semantic_role=delivery
class=compiler
predecessors=CVO0,CVO1
owner=compiler.validation-observability:test-only validation and observability lane
conflict_domains=validation_observability
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
implementation_effort_min=medium
implementation_effort_default=high
review_effort_min=medium
review_effort_default=high
verification_effort_min=medium
verification_effort_default=high
confirmation_effort_min=medium
confirmation_effort_default=high
size=S
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-validation-observability/CVO3.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CVO3 — Critical false-green controls for validation machinery

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

A small set of required negative/mutation controls proving the validation infrastructure detects the failure classes it claims to detect. The current owner is **unproven validation machinery**. The final and sole owner is **the false-green control set and its admission rule**. This charter accepts one boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: none; controls are test-only.
- Named API/data boundaries: control harness entries that inject or construct each protected failure and assert the machinery reports the correct specific outcome.
- Test/CI homes: `crates/verter_validation_probe/tests/cases/controls.rs`, wired through the crate's single `tests/main.rs`; the controls are hermetic (they construct their inputs, never read a corpus checkout) and therefore run in the default canonical gate.
- Mutation boundary: test/CI bytes only; production LOC is zero.

## Exact predecessor contracts

- **CVO0:** implemented ledger row for "Probe outcome taxonomy and probe-state manifest contract"; the controls assert against that taxonomy, its ordering rules, and its expected-state semantics. Ledger presence alone satisfies the predecessor. Its locator metadata remains non-authoritative.
- **CVO1:** implemented ledger row for "Pinned vue-benchmarks external workload probe lane"; supplies the runner (process execution, deadline handling, classification through `ProbeOutcomeClass::terminal`) and the bound comparator that controls 1–3 exercise. Ledger presence alone satisfies the predecessor. Its locator metadata remains non-authoritative.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Intent:** prove the validator cannot silently report success incorrectly.
- **Problem:** a plausible defect in classification, timeout handling, comparison, or canary handling could make CI report success while the underlying contract is broken; positive coverage cannot detect its own blind spots.
- **Solution and architecture decisions — seed exactly four control classes:**
  1. **Diagnostic propagation:** construct a case where compilation fails after initial ingestion, exercised at the runner's ingestion boundary rather than at the classifier alone: a constructed driver response line in the driver's own envelope, using the real failure arm of `HostCompileRequestsEntry` — `{ probe_id, elapsed_ns, entries: [{ canonicalId, response: null, failure: { kind: "productNotProduced", productKind: "runtimeClient", canonicalId, message, diagnostics: <one error-severity entry> } }], reference: [{ code }] }` — is fed to `runner::ingest_response_line` in-process (no test-only envelope shape exists), and the fold must rank the error diagnostic above the product absence, and the terminal must be `verter_diagnostic`, never `product_not_produced` or generic empty output.
  2. **Timeout/refusal distinction:** a timed-out or signaled process cannot satisfy an expected-refusal/negative-control assertion; injected as raw `ExecutionEvent::TimedOut` and `ExecutionEvent::Signaled` through the runner's in-process `runner::classify_execution`, whose classes then flow through the per-dimension fold into `ProbeEntry::evaluate`; the control asserts the classes `timeout` and `crash`, the evaluation `UnrelatedRegression` against a `canary` entry expecting `verter_diagnostic` or `unsupported`, and `GateRegression` against a `gate` entry with the same expectation (both expected states are explicit in the table); the same table carries real-union rows that keep the three boundary classes apart — a driver line whose entry is `{ response: null, failure: { kind: "runtimeSurfaceRefused", ... } }` folds to `unsupported` and evaluates a `Compile` canary expecting `unsupported` to `CanaryExpected`, while a manifest declaring a `Route` gate expecting `unsupported` is rejected by the validator (no atom authorizes it); a `runtimeClient` product with zero `main` nodes folds to `product_not_produced` and one with two `main` nodes to `product_malformed`, for a Vue and for a Svelte entry alike; `Exited { phase: load, code: 1 }` folds to `harness_failure` and `Exited { phase: reference, code: 1 }` to `reference_failure` (the non-zero-exit-by-phase rows); `{ kind: "frameworkMismatch" }` against a gate expecting `request_refused` evaluates to `GatePass` and against one expecting `unsupported` to `GateRegression`; `{ kind: "refused", diagnostics: <one error> }` folds to `verter_diagnostic` (never `request_refused`) and `{ kind: "refused" }` with no diagnostic folds to `harness_failure`; `{ kind: "host", diagnostics: <one error> }` folds to `host_failure` with the diagnostic secondary and evaluates a gate expecting `verter_diagnostic` to `GateRegression`; a driver-level `{ error }` line folds to `harness_failure`; `TimedOut { phase: reference }` folds to `reference_failure` while `TimedOut { phase: compile }` folds to `timeout` and `Signaled { phase: load }` folds to `harness_failure` (the reference-hang and addon-load-failure rows); and `TimedOut` in phase `compile` against any refusal gate evaluates to `GateRegression` — and a mutation of the event-to-class mapping (for example `TimedOut` → `verter_diagnostic`) must turn it red.
  3. **Comparator sensitivity:** deliberately perturb a known-valid comparison product; the comparator must report the appropriate mismatch class; the table includes a row whose only difference is an alpha-equivalent rename of a source-authored identifier (a name in `authored_identifiers(source)`), which must still report `semantic_mismatch`, proving source-authored exactness is enforced; and a row whose reference module is malformed, which must classify `reference_failure` and emit no comparison.
  4. **Canary state transition:** an expected known failure stays non-blocking; an unexpected pass surfaces as XPASS; an unrelated crash/timeout/harness failure is surfaced as a new regression rather than silently accepted as the known failure.
- Admission rule for any future control: it must state (1) the invariant protected, (2) the plausible false-green regression, (3) why existing positive coverage would not detect it. One discriminating control per invariant family; no permutation farming.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **CVO3-AC1 — sole-owner outcome:** exactly the four seeded control classes exist, each with its stated invariant, plausible false-green, and positive-coverage gap; no repository-wide mutation program and no retrofitted mutation tests on unrelated families.
- **CVO3-AC2 — positive contract:** each control demonstrably fails closed — removing or weakening the guarded machinery makes the control red, and the machinery intact makes it green with the specific expected outcome class.
- **CVO3-AC3 — incremental equivalence:** not applicable; the controls own no incremental, cache, cancellation, or publication authority.
- **CVO3-AC4 — bounded work:** not applicable; controls must not add production counters or instrumentation.
- Every control is itself evidence; do not add controls for invariants without a plausible false-green path. Do not add mutation cases merely because a node contains an invariant.
- Test homes: `crates/verter_validation_probe/tests/cases/controls.rs` via `tests/main.rs`; runs in the default canonical gate.

## Deletions and forbidden designs

- Delete or structurally reject: **mutation permutations beyond one discriminating control per invariant family**.
- Delete or structurally reject: **controls that assert implementation details rather than the protected outcome**.
- Never add a repository-wide mutation-testing program, per-fixture mutation jobs, or production hooks used only to seed failures into live paths (injection stays inside test harness constructs/fixtures).
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero control that can pass while its guarded machinery is broken.
- Performance budget: exactly four hermetic controls, each one table-driven in-process invocation of the ingestion parser, classifier, evaluator, or comparator whose rows cover that control's required cases (control 2: `TimedOut` and `Signaled`; control 4: expected known failure, unexpected pass, and unrelated crash/timeout/harness failure); at most one invocation per control, no subprocess, no corpus access, no network.

## Abort conditions

- Stop before mutation if a seeded control cannot be constructed without production hooks, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate if proving a control would require weakening the CVO0 taxonomy or the CVO1 runner contract; record the conflict and stop.

## Targeted verification

1. Run the control set and confirm each reports the specific expected outcome class; then verify fail-closed behavior by temporarily disabling one guarded mechanism locally during review.
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Controls are the TDD artifact themselves: each names the false-green it discriminates before it is written.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** prove the critical validation machinery cannot silently false-green.

**Problem:** classification, timeout, comparison, and canary logic can each fail in a way that reports success; positive tests cannot detect their own blind spots.

**Solution and architecture decisions:**

- four seeded control classes: diagnostic propagation, timeout/refusal distinction, comparator sensitivity, canary state transition;
- each control injects or constructs the failure inside test harness constructs and asserts the specific expected outcome class;
- admission rule for new controls: invariant + plausible false-green + why positive coverage misses it;
- one discriminating control per invariant family.

**Suggested predecessors:** `CVO0` for the taxonomy and manifest, `CVO1` for the runner and comparator the controls drive; may run in parallel with CVO1S/CVO2.

**Normative source decomposition:**

1. **CVO3-A — Control harness.** Injection/construct patterns shared by the four controls.
2. **CVO3-B — Four controls.** One per class above, each with its stated admission record.
3. **CVO3-C — Fail-closed proof.** Local demonstration during review that disabling the machinery turns each control red.
4. **CVO3-D — Admission documentation.** The rule recorded where future controls would be proposed.

**Acceptance:** all four controls discriminate their false-green paths; the set stays small and required; no unrelated test family gains mutation tests.

**Forbidden:** mutation farms, per-fixture mutation jobs, production failure hooks, controls asserting implementation detail.

**Deletion/abort:** delete redundant permutations; abort if a control requires weakening a guarded contract to pass.
