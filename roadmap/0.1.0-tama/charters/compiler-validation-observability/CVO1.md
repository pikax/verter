<!-- unified-charter-v2
id=CVO1
name=Pinned vue-benchmarks external workload probe lane
phase=compiler
train=compiler.validation-observability
product=validation_observability
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CVO0,CCA1O2J
owner=compiler.validation-observability:test-only validation and observability lane
conflict_domains=validation_observability,release_orchestration
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
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=pinned_vue_benchmarks_checkout
charter=charters/compiler-validation-observability/CVO1.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CVO1 — Pinned vue-benchmarks external workload probe lane

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

A bounded real-workload CI lane that exercises Verter against one pinned `pikax/vue-benchmarks` revision through normal/public compiler request routes, classifies every case with the CVO0 taxonomy, and emits one compact machine-readable summary. The current owner is **no continuous external workload evidence**. The final and sole owner is **the table-driven workload probe lane and its probe-state manifest**. This charter accepts one boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: none; the runner and manifest are test/CI tooling.
- Named API/data boundaries: the workload runner (`crates/verter_validation_probe/src/runner.rs`, table/data driven, no one-test-per-fixture jobs) bound to the public NAPI routes `compileRequest` / `compileRequests` on the exported host object (`crates/verter_napi/src/lib.rs`, owned by CCA1O2J) and to no other entry point; the corpus adapter `crates/verter_validation_probe/src/corpus/vue_benchmarks.rs`; `ProbeStateManifest` entries with stable case ids `vue/<relative-path-in-corpus>`; the summary artifact `target/validation-probe/summary.json` written by `scripts/validation-probe.mjs`.
- Comparator and the meaning of `pass`: the lane compares Verter's product against the output of the pinned upstream reference compiler (`@vue/compiler-sfc` at the version pinned in the root `package.json`) using the existing conformance comparator `verter_vue_conformance::compare::compare_modules` and its canonicalizer (`verter_vue_conformance::canon`); nothing new is written to decide equality. `pass` means the public route produced a well-formed product and the comparator reported no diff; `semantic_mismatch` carries the comparator's `DiffReason`. The reference output is comparator input, not expected-output authority: a mismatch is evidence for the owning semantic node (the `compiler.vue-compiler` train), never a verdict on Verter. `runtime_mismatch` and `source_map_mismatch` are declared inapplicable by this lane (never emitted; the manifest records `comparison = structural`) until a later owning node binds an existing runtime executor or map validator.
- Hermeticity (repository policy “Testing-Hermeticity”): the corpus checkout lives at `.integration-tests/repos/vue-benchmarks` and is read only from tests behind `#[cfg(feature = "external-corpus")]`; the default canonical run (`node scripts/gate.mjs`) neither reads nor requires it, and the guard `external_corpus_paths_not_present_outside_gated_tests` must stay green. Only the dedicated workflow `.github/workflows/validation-probe.yml` provisions the pinned checkout and enables the feature.
- Test/CI homes: `crates/verter_validation_probe` (modules `corpus`, `runner`, `manifest`, `summary`), `.github/workflows/validation-probe.yml`, `scripts/validation-probe.mjs`. The workflow and script live under `release_orchestration` roots, which this node therefore also leases. No fallback home: if any of these paths cannot be used, stop before mutation and obtain an authority amendment; never place the runner in another train's test home such as `crates/verter_compiler/tests`.
- Mutation boundary: test/CI bytes only; production LOC is zero.

## Exact predecessor contracts

- **CVO0:** implemented ledger row for "Probe outcome taxonomy and probe-state manifest contract"; supplies the `verter_validation_probe` crate, `ProbeOutcomeClass::terminal`, and `ProbeStateManifest::validate`, which the lane uses verbatim. Ledger presence alone satisfies the predecessor. Its locator metadata remains non-authoritative.
- **CCA1O2J:** implemented ledger row for "NAPI typed host-request callable route"; supplies the bound addon methods `compileRequest` and `compileRequests` on the exported host object, the only routes the runner may call. Ledger presence alone satisfies the predecessor. Its locator metadata remains non-authoritative.
- **External requirements:** agents check `pinned_vue_benchmarks_checkout`; tooling does not validate external state. The pinned revision is recorded in the manifest and every summary artifact.

## Source-specific scope

- **Intent:** prove the probe infrastructure and establish a useful signal-to-cost ratio against representative real Vue inputs.
- **Problem:** without continuous external workload execution, real-workload regressions surface late, failures stay unclassified, and deferred gaps cannot be distinguished from new regressions.
- **Solution and architecture decisions:**
  - one pinned `pikax/vue-benchmarks` revision and one deterministic representative slice; stable case ids; aggregate reporting;
  - the corpus is an external workload probe, not an oracle, semantic authority, expected-output authority, golden source, normalizer authority, or reason to change compiler behavior, and not a replacement for official conformance coverage;
  - requests go through normal/public compiler routes; no test-only semantic shortcuts;
  - the runner, manifest, and summary are framework-neutral: `framework` is a case attribute, never a runner variant, so CVO1S plugs the pinned `pikax/svelte-benchmarks` corpus into this same lane without a second runner or summary format;
  - pull requests run a small deterministic smoke slice bounded structurally, not by wall clock: at most 24 Vue cases, one CI job, one `compileRequests` call per case (no warm repeats in the smoke lane), listed by case id in the manifest; elapsed time is reported as an observation only. A broader main-lane set (at most 200 cases, one job) may contain canary/known-fail cases and runs separately from the required fast gate;
  - the summary artifact reports cases attempted/passed, gated regressions, canary/known failures by outcome class, skips, XPASS/promotion candidates, crashes, timeouts, and harness failures;
  - scope rule for failures: classify first; a cause owned by a future DAG node is recorded with its owner and kept canary/known-fail or skip; a defect in the probe infrastructure itself is repaired here; a regression of an already-gated behavior follows normal regression handling. A test failure alone does not override DAG ownership.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **CVO1-AC1 — sole-owner outcome:** no per-fixture test definitions or per-case CI jobs; the lane is table/data driven and every probe/cell is a manifest entry. Prefer static/structural enforcement over new test scaffolding.
- **CVO1-AC2 — positive contract:** a pinned-revision run produces a deterministic summary (same slice, same classification, stable case ids) containing all required counters; the smoke slice inventory in the manifest has at most 24 cases and the workflow has exactly one probe job; the default canonical gate passes with the corpus checkout absent.
- **CVO1-AC3 — incremental equivalence:** not applicable; the lane owns no incremental, cache, cancellation, or publication authority.
- **CVO1-AC4 — bounded work:** not applicable as a hot path; the runner must not add production counters or instrumentation.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; CVO3 owns the false-green controls, so this node adds no mutation cases.
- Test homes: `crates/verter_validation_probe` (test-only), `.github/workflows`.

## Deletions and forbidden designs

- Delete or structurally reject: **one-test-per-fixture workload jobs**.
- Delete or structurally reject: **test-only compiler request routes used to dodge unsupported behavior**.
- Never treat corpus output as expected-output authority, repair compiler behavior from this train to make CI green, broaden this node into a general ecosystem-corpus project, or pull implementation work forward from existing DAG nodes.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero unclassified failure in the summary; zero silent canary-class change; zero gate regression left unreported.
- Performance budget: not applicable; the smoke lane is bounded by case count and job count above, and no wall-clock figure is a gate.

## Abort conditions

- Stop before mutation if the pinned corpus cannot be checked out deterministically, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate if a workload failure tempts a compiler change; record the owner and keep the case canary/known-fail or skip instead.

## Targeted verification

1. Run the lane against the pinned slice locally and confirm the summary artifact is produced and deterministic.
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral tooling changes require TDD with a failing discriminating regression before the change; do not invent a test solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** exercise representative real Vue inputs continuously, classify every outcome, and keep future-owned gaps visible without blocking required CI.

**Problem:** real-workload regressions are found late when no bounded external lane exists, and generic pass/fail cannot say whether the compiler, harness, product, comparator, or runtime failed.

**Solution and architecture decisions:**

- pinned `pikax/vue-benchmarks` revision, deterministic representative slice, stable case ids;
- table/data-driven runner over the public compiler request routes;
- PR smoke slice plus a broader non-fast-gate main lane;
- compact machine-readable summary with all required counters;
- failure scope rule: classify → associate owner → canary/known-fail/skip → continue.

**Suggested predecessors:** `CVO0` only; no unfinished compiler work.

**Normative source decomposition:**

1. **CVO1-A — Corpus checkout and slice definition.** Pinned revision, deterministic slice, stable case ids.
2. **CVO1-B — Runner.** Table-driven execution through public routes, timeout handling, outcome classification via the CVO0 taxonomy.
3. **CVO1-C — Manifest materialization.** Expected states per case with owners, reasons, pinned revision.
4. **CVO1-D — Summary artifact and CI wiring.** Required counters, PR smoke lane, broader main lane.
5. **CVO1-E — Failure triage pass.** Classify current failures, associate owners, no compiler repairs.

**Acceptance:** the lane runs the pinned slice deterministically; the summary reports every required counter; known future failures execute without blocking required CI; gated regressions and XPASS candidates are surfaced; the probe infrastructure defects found are repaired here.

**Forbidden:** per-fixture CI jobs, oracle or expected-output claims over corpus output, compiler repairs from this train, scope expansion to corpora other than the ones this train's own nodes pin (CVO1S adds `pikax/svelte-benchmarks`), Vue-shaped assumptions baked into the runner or summary.

**Deletion/abort:** delete any ad-hoc per-case test scaffolding in favor of the table-driven runner; abort on inability to pin or classify, recording the blocking evidence.
