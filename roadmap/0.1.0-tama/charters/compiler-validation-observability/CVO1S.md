<!-- unified-charter-v2
id=CVO1S
name=Pinned svelte-benchmarks external workload probe lane
phase=compiler
train=compiler.validation-observability
product=validation_observability
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CVO1
owner=compiler.validation-observability:test-only validation and observability lane
conflict_domains=validation_observability,release_orchestration,github_projection_state
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
external_requirements=pinned_svelte_benchmarks_checkout
charter=charters/compiler-validation-observability/CVO1S.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CVO1S — Pinned svelte-benchmarks external workload probe lane

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

The Svelte half of the real-workload probe lane: one pinned `pikax/svelte-benchmarks` revision and one deterministic representative slice run through the CVO1 runner, over normal/public compiler request routes, classified with the CVO0 taxonomy, reported in the same summary artifact with `framework = svelte` as a case attribute. Svelte is a first-class Verter target; the lane must give it the same continuous evidence Vue gets from CVO1. The current owner is **Vue-only external workload evidence**. The final and sole owner is **the single framework-neutral workload probe lane covering both pinned corpora**. This charter accepts one boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: none; the corpus adapter, slice, manifest entries, and lane wiring are test/CI tooling.
- Named API/data boundaries: the CVO1 workload runner (unchanged shape; `framework` is a case attribute, never a runner variant) over the same public NAPI batch route `compileRequests` and the same `{ canonicalId, source, request }` carrier; the corpus adapter `crates/verter_validation_probe/src/corpus/svelte_benchmarks.rs`; `ProbeStateManifest` entries with stable case ids `svelte/<relative-path-in-corpus>`; the shared summary artifact with per-framework counters.
- Comparator and the meaning of `pass`: no existing Svelte structural comparator is available (`crates/verter_svelte_conformance` ships manifest and generation tooling, no `compare`), so Svelte cases are compile-only observations. Each case issues one `HostCompileRequest` on the `svelte` arm with `products = [runtimeClient]`; product validity is decided exactly as `verter_vue_conformance::canon::canonicalize_module` decides it: the emitted module is extracted exactly as CVO1 extracts it (exactly one `runtimeClient` product, exactly one `main` node; absent → `product_not_produced`, duplicate or foreign → `product_malformed`) and its `main.code` is parsed with `oxc_parser::Parser` and `SourceType::mjs()` and then built with `oxc_semantic::SemanticBuilder`; a parse panic, any parse error, or any semantic-builder error classifies the case `product_malformed` with the messages retained; `pass` means the route produced a product that both parses and builds. Two planted fixtures must classify `product_malformed`, never `pass`: a truncated module (syntax error) and a syntactically valid module with a semantic error (a duplicate `const` binding). A `gate` entry may cite only `compiler.public-request-route`, only for `dimension = Route`, and only for `pass` (`route-callable`) or `request_refused` (`typed-refusal`); `unsupported` and `verter_diagnostic` are `Compile` outcomes, so `Compile` cells are `canary` or `known-fail` citing `svelte.runtime-client-product`; `Structural` cells are `skip` citing the same authority with reason `no Svelte structural comparator` (the driver emits `reference: { inapplicable: "svelte" }` and the runner records `Terminal::NotRun` for `Structural`), and CVO4 may move them `skip -> canary` only once the `compiler.svelte-compiler` train supplies a comparator surface that the authority catalog names. Svelte cases go through the same `probe-driver.mjs` boundary as Vue; the manifest is `crates/verter_validation_probe/manifest/svelte.toml`. The manifest records `comparison = none` and `semantic_mismatch`, `runtime_mismatch`, and `source_map_mismatch` are declared inapplicable for Svelte cases until the `compiler.svelte-compiler` train supplies a comparator, at which point CVO4 may bind it. Writing a Svelte comparator inside this node is forbidden.
- Hermeticity: the corpus checkout lives at `.integration-tests/repos/svelte-benchmarks` behind the same `external-corpus` feature; the default canonical run neither reads nor requires it; only `.github/workflows/validation-probe.yml` provisions it.
- Test/CI homes: `crates/verter_validation_probe` (the Svelte adapter, cases under `tests/cases/` through the single `tests/main.rs`) and `.github/workflows/validation-probe.yml` (a `release_orchestration` root this node also leases); no file under `scripts` is touched. No fallback home; an unavailable path requires an authority amendment before mutation.
- Mutation boundary: test/CI bytes only; production LOC is zero.

## Exact predecessor contracts

- **CVO1:** implemented ledger row for "Pinned vue-benchmarks external workload probe lane"; supplies the framework-neutral runner, manifest format, summary artifact, PR smoke lane, and main lane this node plugs the Svelte corpus into. Ledger presence alone satisfies the predecessor. Its locator metadata remains non-authoritative.
- **External requirements:** agents check `pinned_svelte_benchmarks_checkout`; tooling does not validate external state. The pinned revision is recorded in the manifest and every summary artifact.

## Source-specific scope

- **Intent:** give Svelte the same continuous real-workload evidence as Vue without a second runner, a second summary format, or a second manifest schema.
- **Problem:** a Vue-only lane would let Svelte regressions surface late and would silently bias the train's evidence, promotion decisions, and benchmark observations toward one framework.
- **Solution and architecture decisions:**
  - one pinned `pikax/svelte-benchmarks` revision, the full 40-hex commit SHA recorded in `manifest/svelte.toml` as `external_revision` (initial pin: `e19c48b81ad24b75a6d4b81377b4a7ebc39a1900`), checked out exactly by the workflow; that repository ships no committed `.svelte` files — its corpus is produced by its own deterministic generator (`pnpm generate`, fixtures under `fixtures/`) at the pinned SHA, so the workflow runs the generator once and the manifest lists the generated files case by case with a content digest per case; `ProbeStateManifest::validate` proves every listed case exists with its digest after generation, so a generator drift is a manifest failure, not silent inventory change; the representative slice is stratified by the generator's own fixture families (recorded verbatim as `strata` in the manifest at ratification, each with `min_cases >= 2`) and the main lane is the complete generated set; stable `svelte/...` case ids; aggregate reporting in the shared artifact with per-framework and total counters;
  - the corpus is an external workload probe, not an oracle, semantic authority, expected-output authority, golden source, normalizer authority, or reason to change compiler behavior, and not a replacement for `crates/verter_svelte_conformance` coverage;
  - requests go through normal/public compiler routes for Svelte inputs; no test-only semantic shortcuts;
  - the PR smoke slice gains at most 24 Svelte cases in the same single job (combined smoke inventory at most 48 cases, one `compileRequests` call per case); the main lane runs the complete generated Svelte set beside the complete Vue inventory in the same job, its size fixed by the manifests (the summary validator refuses an inventory that differs from the manifests); elapsed time is an observation only;
  - a Svelte case whose failure belongs to a future DAG node (for example the `compiler.svelte-compiler` or `compiler.svelte-style` trains) is recorded with that owner and kept canary/known-fail or skip; a runner defect exposed by Svelte inputs (framework coupling, Vue-shaped assumptions in classification or reporting) is a probe-infrastructure defect and is repaired in this train. A test failure alone does not override DAG ownership.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **CVO1S-AC1 — sole-owner outcome:** exactly one runner, one manifest schema, and one summary format serve both corpora; no Svelte-specific runner, summary, or per-fixture CI job exists. Prefer static/structural enforcement over new test scaffolding.
- **CVO1S-AC2 — positive contract:** a pinned-revision Svelte run produces a deterministic summary (same slice, same classification, stable case ids) whose per-framework counters sum to the totals; the Svelte smoke inventory is non-empty and the combined inventory has at most 48 cases in one job; every selected Svelte case id exists at the pinned corpus revision; the committed Svelte manifest contains zero classless canaries, the summary reports `attempted == selected > 0` for each of `vue` and `svelte`, and a planted summary that omits one framework must fail the lane through the workflow's own entry point; the default canonical gate passes with both corpus checkouts absent.
- **CVO1S-AC3 — incremental equivalence:** not applicable; the lane owns no incremental, cache, cancellation, or publication authority.
- **CVO1S-AC4 — bounded work:** not applicable as a hot path; the adapter must not add production counters or instrumentation.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; CVO3 owns the false-green controls, so this node adds no mutation cases.
- Test homes: `crates/verter_validation_probe` (test-only), `.github/workflows`.

## Deletions and forbidden designs

- Delete or structurally reject: **a Svelte-specific runner, manifest schema, or summary format**.
- Delete or structurally reject: **one-test-per-fixture Svelte workload jobs**.
- Never treat corpus output as expected-output authority, repair Svelte compiler behavior from this train to make CI green, broaden this node into a general ecosystem-corpus project, or pull implementation work forward from the Svelte compiler trains.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero unclassified Svelte failure in the summary; zero silent canary-class change; zero gate regression left unreported; zero divergence between per-framework counters and totals.
- Performance budget: not applicable; the smoke lane is bounded by case count and job count, and no wall-clock figure is a gate.

## Abort conditions

- Stop before mutation if the pinned corpus cannot be checked out deterministically, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate if adding Svelte cases requires a second runner or summary format rather than a corpus adapter; that is a CVO1 framework-coupling defect to repair first, in this train, not a reason to fork the lane.
- Abort the candidate if a Svelte workload failure tempts a compiler change; record the owner and keep the case canary/known-fail or skip instead.

## Targeted verification

1. Run the lane against both pinned slices locally and confirm one summary artifact is produced, deterministic, with per-framework counters that sum to the totals.
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral tooling changes require TDD with a failing discriminating regression before the change; do not invent a test solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** exercise representative real Svelte inputs continuously through the same lane as Vue, classify every outcome, and keep future-owned Svelte gaps visible without blocking required CI.

**Problem:** Svelte is a first-class target; a Vue-only lane leaves half the compiler without continuous real-workload evidence and biases every downstream observation and promotion.

**Solution and architecture decisions:**

- pinned `pikax/svelte-benchmarks` revision, deterministic representative slice, stable `svelte/...` case ids;
- corpus adapter over the CVO1 runner; `framework` is a case attribute;
- Svelte share of the PR smoke slice plus Svelte cases in the broader main lane;
- shared summary artifact with per-framework counters;
- failure scope rule: classify → associate owner (Svelte compiler/style trains where applicable) → canary/known-fail/skip → continue.

**Suggested predecessors:** `CVO1` only; no unfinished compiler work.

**Normative source decomposition:**

1. **CVO1S-A — Corpus checkout and slice definition.** Pinned revision, deterministic slice, stable case ids.
2. **CVO1S-B — Corpus adapter.** Svelte inputs through the public routes via the existing runner; any framework coupling found in the runner is repaired here.
3. **CVO1S-C — Manifest entries.** Expected states per Svelte case with owners, reasons, pinned revision.
4. **CVO1S-D — Lane wiring.** Svelte share of the PR smoke slice, main-lane inclusion, per-framework counters in the summary.
5. **CVO1S-E — Failure triage pass.** Classify current Svelte failures, associate owners, no compiler repairs.

**Acceptance:** both corpora run through one lane deterministically; per-framework counters sum to totals; known future Svelte failures execute without blocking required CI; gated regressions and XPASS candidates are surfaced for Svelte exactly as for Vue.

**Forbidden:** a Svelte-specific runner or summary, per-fixture CI jobs, oracle or expected-output claims over corpus output, compiler repairs from this train, scope expansion to further corpora.

**Deletion/abort:** delete any Svelte-only scaffolding in favor of the shared runner; abort on inability to pin or classify, or on a runner that cannot take a second framework without forking, recording the blocking evidence.
