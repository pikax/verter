<!-- unified-charter-v2
id=CVO0
name=Probe outcome taxonomy and probe-state manifest contract
phase=compiler
train=compiler.validation-observability
product=validation_observability
kind=contract
semantic_role=delivery
class=compiler
predecessors=CMP0
owner=compiler.validation-observability:test-only validation and observability lane
conflict_domains=validation_observability
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
implementation_effort_min=medium
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=medium
verification_effort_default=high
confirmation_effort_min=medium
confirmation_effort_default=high
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-validation-observability/CVO0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CVO0 — Probe outcome taxonomy and probe-state manifest contract

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

One shared typed contract for validation and probe results. The current owner is **generic pass/fail collapse in comparison and workload tooling** ("empty output", "null response", "failed"). The final and sole owner is **typed probe outcome taxonomy with causal ordering plus the machine-readable probe-state manifest**. This charter accepts one contract boundary; it contains no independently dispatchable subblocks and no compiler semantic changes.

## Concrete surfaces and APIs

- Production surfaces: none; this is a test/CI/tooling-only train.
- Named API/data boundaries: the test-only workspace crate `crates/verter_validation_probe` (not a member of any production dependency graph) with modules `outcome` (`ProbeOutcomeClass` and the precedence fold `ProbeOutcomeClass::terminal(observed: &[ProbeOutcomeClass]) -> (ProbeOutcomeClass, Vec<ProbeOutcomeClass>)` returning the single terminal class plus retained secondary evidence), `manifest` (`ProbeStateManifest`, `ProbeEntry { probe_id, framework, case, expected_state, expected_class, owner, reason, external_revision }`, `ProbeStateManifest::validate`), and the rejection fixtures under `crates/verter_validation_probe/tests`. Successor nodes plug the runner, corpus adapters, controls, and join validator into this crate; none of them re-declares these types.
- Mutation boundary: the contract text in this charter, the new test-only crate, and its fixtures; production LOC is zero.

## Exact predecessor contracts

- **CMP0:** implemented ledger row for "Compiler request, policy, compatibility, and identity contract"; it fixes the public compiler request/refusal vocabulary the probe taxonomy classifies against. Ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

- **Intent:** every comparison and workload tool reports one of a closed set of actionable outcome classes instead of a generic failure state, and every externally exercised probe/cell is described by one machine-readable manifest with an explicit expected state.
- **Problem:** generic failure states hide whether the compiler, the harness/transport, the product shape, the comparator, or the runtime/map comparison failed, and they let known future gaps either block CI or silently pass.
- **Solution and architecture decisions:**
  - minimum outcome classes: `pass`, `verter_diagnostic`, `product_not_produced`, `product_malformed`, `semantic_mismatch`, `runtime_mismatch`, `source_map_mismatch`, `reference_failure`, `harness_failure`, `unsupported`, `timeout`, `crash`; the set is extensible only for a genuinely distinct actionable class, never by alias or tool-specific variant;
  - causal ordering when multiple states are present is total and deterministic: exactly one class is the outcome, the highest-precedence class present, and every lower class observed in the same run is recorded as secondary evidence, never as the outcome. Precedence, highest first: `harness_failure` (the harness/transport could not execute or observe the case), `crash` (the compiler process died by signal or abnormal exit), `timeout` (the compiler process was stopped by the deadline), `verter_diagnostic` (the compiler reported an error diagnostic), `unsupported` (the compiler explicitly declared the input outside its contract), `reference_failure` (the reference side of a comparison could not be produced or executed), `product_not_produced` (compilation ended without a diagnostic and without a product), `product_malformed` (a product exists but fails its own validity check), `semantic_mismatch` (structural/semantic comparison differs), `runtime_mismatch` (runtime comparison differs), `source_map_mismatch` (map comparison differs; a runtime mismatch outranks a map mismatch because a wrong program makes its map moot), `pass`. Impossible combinations are rejected rather than resolved: a compile diagnostic is never downgraded to `product_not_produced` because no product followed it; a `crash`, `timeout`, or `harness_failure` never satisfies an expected `verter_diagnostic` or `unsupported` (an expected refusal is met only by the refusal class itself); a comparison class is never reported for a case whose product or reference is absent;
  - expected states: `gate` (failure blocks), `canary` (known failure class is non-blocking; an unrelated class such as crash/timeout/harness failure is a new regression), `known-fail` (canary with an owning future DAG node; an unexpected pass is XPASS/promotion candidate, not an automatic gate), `skip` (only when execution is meaningless/impossible/unstable; requires owning future DAG node and reason; prefer canary over skip);
  - observation does not imply acceptance: a manifest entry records evidence about Verter behavior and never derives semantics from external fixture output;
  - the contract is framework-neutral: every manifest entry and every summary row carries `framework` (`vue` or `svelte`; both are first-class Verter targets) as a case attribute, so one taxonomy, one manifest schema, and one summary format serve every corpus the train's nodes pin.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **CVO0-AC1 — sole-owner outcome:** the contract must structurally reject generic terminal failure states and alias proliferation; no tool-specific outcome variant may bypass the taxonomy (`ProbeOutcomeClass` is a closed enum; there is no string-typed outcome). Planted-invalid proof is required, not optional: `ProbeStateManifest::validate` must reject at least (a) a generic `failed`/`error` alias offered as an outcome class, (b) a `known-fail` or `skip` entry with no owner, (c) an invalid state/class combination (`gate` with an expected failure class; `canary` with `expected_class = pass`), and `ProbeOutcomeClass::terminal` must reject the impossible combinations named above (a diagnostic plus `product_not_produced`; a `crash`/`timeout`/`harness_failure` plus an expected refusal). One fixture per rejection; no permutations.
- **CVO0-AC2 — positive contract:** ordering rules are total and deterministic for every combination, and every manifest entry validates (stable id, expected state, expected outcome class where applicable, owning DAG node for every deferred/skip entry, pinned external revision where applicable).
- **CVO0-AC3 — incremental equivalence:** not applicable; the contract owns no incremental, cache, cancellation, or publication authority.
- **CVO0-AC4 — bounded work:** not applicable; the contract owns no hot path.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_validation_probe/tests` (rejection fixtures and the precedence-fold cases), this charter for the contract text.

## Deletions and forbidden designs

- Delete or structurally reject: **generic failure collapse** ("empty output", "null response", undifferentiated "failed") as a terminal outcome.
- Delete or structurally reject: **outcome aliases and tool-specific variants**.
- Never add a second semantic authority, oracle status for external fixtures, expected-output authority derived from third-party output, test-only production bypass, or an automatic promotion rule that turns a single pass into a gate.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero outcome class that hides a more specific compiler/harness result; zero deferred/skip entry without an explicit future owner.
- Performance budget: not applicable; do not create counters or soaks solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named contract boundary, an ancestor lacks an implemented ledger row, or the complete diff will not fit one review context.
- Abort the candidate if the contract would require a compiler semantic change to satisfy; that belongs to the owning DAG node, not this train.

## Targeted verification

1. `cargo nextest run -p verter_validation_probe` and `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. The rejection fixtures are the TDD artifact: each is written red against the schema before the validation that turns it green.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** one shared, typed, causally ordered outcome contract plus a probe-state manifest so external execution can be observed without implying acceptance.

**Problem:** generic failure states cannot distinguish compiler, harness, product, comparator, runtime, or source-map failures, and CI cannot execute known future gaps without either blocking or silently green-ing.

**Solution and architecture decisions:**

- closed minimum taxonomy with an explicit extension rule (genuinely distinct actionable class only);
- causal ordering rules that preserve diagnosis precedence;
- machine-readable manifest with stable probe ids, expected states (`gate`, `canary`, `known-fail`, `skip`), expected outcome classes, owning DAG nodes, terse reasons, and pinned external revisions;
- XPASS surfacing and unrelated-class regression detection for canary/known-fail entries;
- every skip carries an owning future DAG node and reason; canary preferred over skip whenever partial execution yields evidence.

**Suggested predecessors:** `CMP0` public compiler request contract; no unfinished compiler work.

**Normative source decomposition:**

1. **CVO0-A — Outcome taxonomy.** The class set, meanings, and the extension rule.
2. **CVO0-B — Ordering rules.** Causal precedence, diagnostic-vs-product distinction, timeout/refusal rule.
3. **CVO0-C — Manifest schema and crate.** The `verter_validation_probe` crate skeleton, entry fields, stable ids, `validate` constraints, and the three rejection fixtures.
4. **CVO0-D — Expected-state semantics.** gate/canary/known-fail/skip rules, XPASS, unrelated-class regression, owner requirement.
5. **CVO0-E — Independent review.** Challenge class sufficiency, ordering totality, and manifest completeness before any lane is built on it.

**Acceptance:** no generic failure state can hide a more specific outcome; known future failures can execute without blocking required CI; unexpected passes and unexpected new failure classes are surfaced; every deferred/skip state has an explicit future owner; no compiler semantic change is required.

**Forbidden:** deriving semantics or expected output from external fixtures, tool-specific outcome variants, automatic promotion, skip entries without owners.

**Deletion/abort:** no production deletion; abort if satisfying the contract would require repairing compiler behavior — record the failure against its owning future node instead.
