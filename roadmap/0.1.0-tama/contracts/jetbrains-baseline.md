# JetBrains baseline and superiority: official comparator, workflow matrix and measurement constitution

Status: RATIFIED by JBT0 (charter `charters/expansion-jetbrains-product/JBT0.md`), consuming the accepted DX0 cross-surface feature preservation and exposure constitution (`contracts/product-experience.md`). Machine products: `tests/jetbrains-baseline/JBT0/products/`. Train plan: `plans/expansion-jetbrains-product.md`.

This contract turns "better than official JetBrains tooling" into a falsifiable product gate. It binds every claim that Verter's JetBrains product (WebStorm-family adapter over `verter-lsp` and the Verter toolchain) is superior to the official tooling in a real IDE. It adds comparator, workflow-parity and measurement obligations on top of DX0; it reopens no producing owner — source identity, semantic authority (TypeScript stays the semantic-rail authority), mapping, lifetime and authored-edit contracts remain with their existing owners. Every DX0 execution-class, receipt-basis, rails and required-exposure rule binds this train unchanged: the JetBrains adapter is a `NativeOnly` consumer surface, its engine identities are pinned (never implied), and its answers ride the same semantic/inspection/comparison rails.

## 1. Comparator law (JBT0.1)

The official comparator is official WebStorm as shipped, in its recommended/default Vue-support mode, with the official Vue plugin and language services at their shipped versions.

- The comparator is benchmarked in its **recommended/default mode**. Any **alternate service-powered mode** (an opt-in richer official service) is recorded as a separate comparator row; it is never silently substituted for the default, and the default is never weakened to flatter Verter.
- **Diagnostics stay enabled** in every comparator run (JBT0-AC1). A comparator configuration with diagnostics, type checking, or any required feature disabled — to make the baseline faster or less correct — is invalid, and every measurement produced from it is void.
- The comparator is identified by captured pins (section 2), never by name alone, and never by a guessed value. A pin that has not been captured from the installed official product is `pending-capture`; a superiority claim may not rest on a `pending-capture` pin, and filling a pin by guesswork instead of capture is a rejected measurement (the DX0 "guessed zeros" prohibition extended to comparator identity).
- Comparator and candidate run on the **same ProductReceiptBasis** (DX0 §4): same source revisions, same project configuration, same corpus, explicitly pinned engine and host identities, and distinct completeness states. Old-source, old-project, old-engine, old-worker and wrong-handle results are rejected on both sides.
- The measured comparator product is the real IDE doing its required work. Installation presence, protocol smoke, or a screenshot is not comparator evidence (DX0 §9).

## 2. JetBrainsBaselineManifest v1

Vocabulary (machine product `jetbrains-baseline-manifest.v1.json`):

- `comparatorId` — the pinned official comparator identity (product family and support mode).
- `ide` — official IDE identity: product name, build number range, and distribution channel. Captured from the installed product, not from marketing pages.
- `plugins` — the official plugin set participating in Vue/web support: plugin id, version, bundled/disabled state in the measured configuration.
- `engines` — the engine identities the official product executes with for web languages (its TypeScript/language-service versions), each captured from the running product.
- `settings` — the measured configuration profile: the recommended/default settings baseline, every deviation from defaults required to run the corpus, and the diagnostics/features-enabled attestation. The attestation is mandatory; a profile without it is invalid (JBT0-AC1).
- `corpus` — the pinned comparison corpus (section 5): representative real projects, libraries, and adversarial cases with their pinned revisions.
- `alternateModes` — recorded alternate service-powered modes, each a full comparator row (own settings and engine pins), marked `alternate`; never the default row.
- Per-row `captureState` — `captured` (value observed from the installed official product, with capture owner and route) or `pending-capture` (slot reserved, value absent, named owner responsible). There is no third state: a value is either captured or absent, never guessed.
- Per-row `owner` — JBT1H owns executing the capture with the real-IDE harness; JBT1 owns the pinned JVM runner that executes it.

Rules:

- The manifest is the only admissible comparator identity for superiority claims in this train. Comparisons run against an unpinned or differently-pinned comparator are not evidence.
- Every pin addition or change is a new capture with a new capture route; deleting a captured pin to dodge a non-regression condition is forbidden.
- The manifest binds DX0 vocabulary (`ProductReceiptBasis`, `HostExecutionClass`) and introduces no second status store, no new receipt machinery.

## 3. RequiredWorkflowMatrix v1 (JBT0.2)

The required workflow population (machine product `required-workflow-matrix.v1.json`), fixed by this constitution — a workflow absent from the matrix is not required, and a required workflow absent from a candidate is a parity gap:

completion, diagnostics, source navigation (definitions/declarations), find usages, generics support, public types, component extraction, rename/move/import updates, formatting, inlays, styles (style support and its navigation), and preserved run/debug workflows.

Per-workflow obligations:

1. **Parity definition.** Each row pins what the official tool does in its default mode for that workflow (its shipped behaviour, scoped by the manifest pins) and the pass predicate the Verter product must meet on the same corpus — including correctness of results, not only availability.
2. **Non-regression conditions.** Each row carries explicit non-regression conditions: no dropped diagnostics, no weakened typing, no reduced correctness, and no silently narrowed scope relative to the official baseline. A candidate that is faster by doing less required work on any row is void (the charter's abort condition).
3. **Completeness states.** Each row's state for a candidate is one of `parity`, `gap`, `partial`, `unsupported`, `pending` — distinct, never collapsed; `complete-empty` style success theater is rejected as in DX0 §4.
4. **Promotion blocking.** Product promotion requires `parity` (or better, per section 4) on **every** required row. A missing required workflow blocks promotion even when Verter is faster on every measured row (JBT0-AC3). Speed never buys out a parity gap.
5. **Preserved workflows.** Run/debug and other IDE workflows the official product already provides must be preserved through the adapter (JBT8's scope); removing or degrading an official workflow the user has is an unexplained UI feature removal and forbidden.

The matrix is consumed by JBT9 (official-tool workflow parity and correctness qualification), which is the only node that may record measured parity outcomes against the manifest.

## 4. SuperiorityPredicate v1 (JBT0.3, JBT0.4)

Superiority is a conjunction, evaluated only on the full gate (machine product `superiority-predicate.v1.json`). **All** clauses must hold:

1. **Workflow parity.** `parity`-or-better on every RequiredWorkflowMatrix row (section 3), qualified by JBT9 on the pinned corpus.
2. **Measured advantage.** Material performance margins met on the predeclared metric set (section 5), measured by JBT10 on the same basis as the comparator, exceeding the predeclared thresholds recorded in the shared contract. A single selected microbenchmark cannot certify overall superiority (JBT0-AC2): the predicate is evaluated over the full metric set, and no subset of metrics may be selected post-hoc to declare victory.
3. **Verter-specific semantic tooling.** At least one differentiating Verter capability (semantic tooling the official product does not provide) demonstrated executable and exposed per DX0 §6 — not merely registered.

A thin LSP plugin is only an implementation starting point (JBT0.4): reaching the IDE through `verter-lsp` satisfies no clause of this predicate by itself.

## 5. Predeclared measurement methodology

- **Equal-work corpus.** Measurements use WSP's equal-work corpus and the limits in the shared performance contract, with **all required features enabled** on both sides (diagnostics, completion, navigation — per the manifest attestation). WSP1 owns new performance instrumentation; this train consumes it.
- **Metric set (predeclared).** Real-client responsiveness (actual client application/paint, not server-side latency alone), provider process costs (CPU and wall time of the semantic provider processes on both sides), outbound bytes on the wire, and retained memory. Unavailable metrics are explicitly labelled `unavailable` with the reason; they are never guessed as zeros and never silently omitted.
- **Same basis.** Comparator and candidate are measured on identical source revisions, corpus, and host hardware in one campaign; every run binds its receipt basis; stale or wrong-handle runs are rejected (DX0 §4).
- **Required work.** A performance result obtained by doing less required work (fewer diagnostics, narrower analysis, dropped features) is void, and is an abort/replan condition for the claiming node, not a tuning finding.
- **Claim scope.** No general performance claim follows from a compiler microbenchmark. Claims are scoped to the measured matrix rows and corpus; extrapolation to unmeasured workflows or projects is forbidden.
- **Incremental vs fresh.** Where the adapter changes caching, mapping, snapshots, result handles or incremental publication, incremental and fresh execution are compared on the same basis (JBT0-AC-BASIS). JBT0 itself changes none of these: it binds this obligation to JBT1H (harness) and JBT10 (qualification) instead of fabricating a runtime test here.

## 6. Adapter boundary and forbidden routes

- The JetBrains adapter is one consumer surface over the shared producer services (DX0 §8): `extensions/jetbrains` (the IntelliJ-platform adapter), `packages/dx-harness/jetbrains` (the comparison harness driver), consuming `crates/verter_protocol/src` contracts. A second semantic/type/project engine inside the client is forbidden; `verter-lsp` and the Verter toolchain remain the sole semantic authority.
- The adapter may be inert/dormant before its activation boundary (DX0 §10), but a cutover cannot retain two active semantic authorities — the official plugin and the Verter adapter cannot both authoritatively answer semantics in one open project (JBT3 owns coexistence and feature ownership).
- Reduced client capabilities preserve semantic meaning (DX0 §9): rich information fetched on demand; closed/disabled views create no background semantic work; cross-file edits stay authored, version-checked and atomic through the existing transaction authority.
- Forbidden (charter, binding here): regex/name-based semantic truth; generated coordinates passed off as authored; hidden source uploads; arbitrary companion shell execution; dropped diagnostics or weakened typing to meet timing; guessed zeros/pins for missing telemetry or captures; sleeps as readiness; stale publication; unexplained UI feature removal; declaring product superiority from installation or protocol smoke alone.

## 7. Scope and ownership

JBT0 is contract-only: 0 production LOC, 0 production files. The adapter (JBT1+), the real-IDE comparison harness (JBT1H), parity qualification (JBT9), comparative performance qualification (JBT10), and the superiority terminal (JBT11) belong to their own nodes.

Final owner for this product outcome: `expansion.jetbrains-product`. Semantic algorithms, project resolution, format/lint logic, TypeScript projection and edit transactions stay with their existing producing owners. Conflict domains: `jetbrains_product`, `semantic_presentation`, `performance_evidence`.

Receiving amendments (machine product `baseline-ownership-map.json`): JBT1, JBT1S, JBT1H, JBT2, JBT3, JBT4, JBT4N, JBT5, JBT6, JBT7, JBT8, JBT9, JBT10, JBT11 (train); WSP1 (performance instrumentation); DX1 (exposure registration); cross-train adapters TST10J and DBG9J bind through their own charters. This contract creates no reverse edges into predecessor trains. JBT0's deletion population is empty; no route is retired by this node.
