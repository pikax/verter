<!-- unified-charter-v2
id=NCKF0
name=Required native checker diagnostic-family convergence
predecessors=NCF-BD-SCOPE,NCF-BD-DUP,NCF-BD-INIT,NCF-RO-ASSIGN,NCF-RO-OPER,NCF-RO-EXCESS,NCF-CO-CALL,NCF-CO-OVER,NCF-CO-INFER,NCF-CF-CONTEXT,NCF-CF-VAR,NCF-CF-THIS,NCF-FD-NARROW,NCF-FD-DEF,NCF-FD-CFLOW,NCF-OC-MEM,NCF-OC-HERIT,NCF-OC-MERGE,NCF-MP-MODULE,NCF-MP-AUG,NCF-MP-PROJECT,NCF-AT-QUERY,NCF-AT-REDUCE,NCF-AT-CYCLE,NCF-JF-JSX,NCF-JF-VUE,NCF-JF-SVELTE,NCF-JD-JS,NCF-JD-JSDOC,NCF-JD-DEC
phase=expansion
train=expansion.native-checker
product=native_checker
kind=convergence
semantic_role=convergence
class=successor-generated-convergence
owner=expansion.native-checker:machine-generated required-family receipt convergence
conflict_domains=vertical_manifest,program_authority,performance_evidence
resource_class=rust-mixed
gate_profile=targeted-domain
review_profile=architecture-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
dispatchable=true
optional=false
release_gating=contract
external_requirements=
charter=charters/expansion-native-checker/generated-families/NCKF0.md
size=S
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# NCKF0 — Required native checker diagnostic-family convergence

Readiness is derived only from implemented-ledger rows for the node ancestors. Commit message, approximate timezone-bearing date, and optional PR are loose locator hints; the CLI performs no Git, GitHub, SHA, tree, ancestry, receipt, lease, or digest validation.

## Independently acceptable outcome

Prove that every manifest row marked `required = true` has an accepted implementation receipt, current oracle/correction-overlay certification receipt, exact NCK6 promotion receipt, provider-zero-work evidence, and current charter/source digest. NCKF0 is generated from the manifest and adds no semantic rule or diagnostic algorithm.

## Concrete surfaces and APIs

- Production surfaces: `.claude/skills`, `AGENTS.md`, `crates`, `roadmap/0.1.0-tama`, `packages`, `crates/verter_identity`, `crates/verter_identity/src`, `crates/verter_language/src`, `crates/verter_protocol/src`, `crates/verter_session/src`, `crates/verter_audit/src`, `crates/verter_bench`, `crates/verter_compiler/src`, `crates/verter_lsp`, `crates/verter_lsp/src`, `crates/verter_napi/src`, `crates/verter_semantic/src`, `crates/verter_wasm/src`, `roadmap/0.1.0-tama/evidence`, `packages/benchmark`, `scripts`.
- Pack production inventory:
  - no production mutation; this is a constitution/convergence authority block.
- Named API/data boundaries:
  - exact schema and receipt boundaries declared by this charter.

## Exact predecessor contracts

- **NCF-BD-SCOPE:** implemented ledger row for “Lexical scope, shadowing, and name binding diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-BD-DUP:** implemented ledger row for “Duplicate and conflicting declaration diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-BD-INIT:** implemented ledger row for “Use-before-declaration and initialization diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-RO-ASSIGN:** implemented ledger row for “Assignment and return assignability diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-RO-OPER:** implemented ledger row for “Operator and property/index access diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-RO-EXCESS:** implemented ledger row for “Freshness, excess-property, and object conformance diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-CO-CALL:** implemented ledger row for “Call, construct, tag, and invocation applicability diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-CO-OVER:** implemented ledger row for “Overload resolution and implementation conformance diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-CO-INFER:** implemented ledger row for “Generic inference, constraints, defaults, and instantiation diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-CF-CONTEXT:** implemented ledger row for “Contextual typing and expression conformance diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-CF-VAR:** implemented ledger row for “Function variance, predicate, assertion, and effect diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-CF-THIS:** implemented ledger row for “This, super, private environment, and call-context diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-FD-NARROW:** implemented ledger row for “Control-flow narrowing and impossible-condition diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-FD-DEF:** implemented ledger row for “Definite assignment and initialization coverage diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-FD-CFLOW:** implemented ledger row for “Reachability, return coverage, and control-flow legality diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-OC-MEM:** implemented ledger row for “Object, class, interface, and member declaration diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-OC-HERIT:** implemented ledger row for “Heritage, override, abstract, and implementation diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-OC-MERGE:** implemented ledger row for “Enums, namespaces, ambient declarations, and declaration merging diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-MP-MODULE:** implemented ledger row for “Module resolution, import/export, and package-boundary diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-MP-AUG:** implemented ledger row for “Module/global augmentation and cross-file declaration diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-MP-PROJECT:** implemented ledger row for “Project references, configuration, library, and program diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-AT-QUERY:** implemented ledger row for “Keyof, indexed access, type query, alias, and reference diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-AT-REDUCE:** implemented ledger row for “Mapped, conditional, infer, template-literal, and utility-type diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-AT-CYCLE:** implemented ledger row for “Recursive type, instantiation cycle, and complexity diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-JF-JSX:** implemented ledger row for “JSX intrinsic/component, props, children, and attribute diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-JF-VUE:** implemented ledger row for “Vue template and component-contract diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-JF-SVELTE:** implemented ledger row for “Svelte template, rune, event, slot/snippet, and component-contract diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-JD-JS:** implemented ledger row for “JavaScript and CommonJS semantic diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-JD-JSDOC:** implemented ledger row for “JSDoc type, template, import, and tag diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **NCF-JD-DEC:** implemented ledger row for “Decorator, metadata, and auto-accessor diagnostics”; ledger presence alone satisfies the predecessor. Its commit message, approximate timezone-bearing date, and optional PR are locator hints only.
- **External requirements:** agents check any listed requirement; tooling does not validate external state.

## Source-specific scope

### Binding architecture

- The predecessor set is generated from the exact required slice rows; hand-maintained lists and external “all complete” assertions are forbidden.
- A slice is complete only when implementation, certification, authority promotion, incremental/admission, and performance evidence all bind the same candidate and manifest row identity.
- Optional/residual rows remain explicit and do not block NCKF0 unless promoted to required by an amendment.
- Any changed manifest, charter, source atom, implementation, oracle, overlay, toolchain, authority, or evidence digest invalidates convergence.
- NCKF0 emits one immutable `NativeCheckerFamilyConvergenceReceipt` consumed by NCK8.

### Internal subblocks

#### NCKF0-SB1 - Manifest/predecessor bijection

Generate the predecessor set from required rows and prove exact set equality, stable ordering, no duplicate/unknown IDs, and no required row without a DAG node/charter.

#### NCKF0-SB2 - Receipt chain validation

For every slice, validate exact implementation, oracle, correction-overlay, certification, promotion, provider-zero-work, gate, and review receipts against the current tree.

#### NCKF0-SB3 - Cross-slice authority consistency

Prove no overlapping family/profile/feature-slice publishing authority, no gaps for required applicability, stable diagnostic identity namespaces, and no conflicting correction overlays.

#### NCKF0-SB4 - Global incremental/admission invariants

Run generated class-wide mutations proving no required slice caches cancelled/stale/partial/NeedInputs/budget outcomes as complete and that combined incremental results equal fresh results.

#### NCKF0-SB5 - Equivalent-work and retained-state convergence

Aggregate PER0 counters without hiding per-slice regressions; require provider diagnostic work zero for every certified slice and bounded memory across combined workloads.

#### NCKF0-SB6 - Immutable convergence receipt

Emit a receipt binding manifest, generated DAG/charters, all predecessor receipts, source atoms, authority snapshot, toolchains, evidence, and reviews. Any input change invalidates the receipt.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **NCKF0-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or reproduced defect that existing evidence does not discriminate.
- **NCKF0-AC2 — positive contract:** the named API/data boundary preserves exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **NCKF0-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm; otherwise bind a terse not-applicable rationale.
- **NCKF0-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, provider, filesystem, network, allocation, copy, or retained-candidate work; otherwise bind a terse not-applicable rationale.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; do not add implementation mirrors, duplicate permutations, or universal test quotas.
- Test homes: `crates/verter_session/tests/cases`, `crates/verter_protocol/tests`, `packages/typescript-plugin/src`, and the exact generated vertical fixture selected by this node.


### Pack-specific proof obligations

- **NCKF0-AC-BIJECTION:** required manifest rows equal generated predecessors/charters exactly.
- **NCKF0-AC-RECEIPTS:** every slice has one current complete implementation/certification/promotion/evidence chain.
- **NCKF0-AC-AUTHORITY:** required applicability has no duplicate or missing publishing authority.
- **NCKF0-AC-ADMISSION:** class-wide mutations preserve complete-only admission and incremental/fresh equality.
- **NCKF0-AC-ZERO-PROVIDER:** every certified required slice performs zero external diagnostic work at runtime.
- **NCKF0-AC-PERF:** aggregate and per-slice equivalent-work/allocation/latency/RSS thresholds pass.

## Deletions and forbidden designs

- Delete or structurally reject the displaced authority named by the source charter after same-candidate replacement proof.

- Manual/external attestation that “all slices are complete”.
- Patching a semantic mismatch, rule, or feature in this convergence block.
- Aggregate pass percentages that hide one stale/missing slice receipt.
- Promoting optional/residual rows without a manifest amendment.

- Delete or structurally reject every compatibility path that would preserve a second owner after cutover.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 300 production LOC, 3 production files, 1 related crates/packages.
- Mandatory rescope above 1500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, identity aliasing, or unauthorized executable work.
- Performance budget: when preflight identifies touched authority or a hot path, use the ratified replacement SLO and equivalent-work counters below; otherwise performance evidence is not applicable and no soak is invented solely to populate evidence.



## Abort conditions

- Stop before mutation if the exact sole owner, predecessor contract, or evidence boundary is false.

## Targeted verification

1. `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

### Pack-specific verification inventory

Run the manifest generator/bijection validator, every generated receipt validator, class-wide authority/admission/provider-zero-work mutations, combined incremental/fresh and churn/performance suites, canonical gate, and independent architecture review on the squashed review candidate.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.
