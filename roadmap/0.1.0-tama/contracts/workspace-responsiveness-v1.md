# Workspace responsiveness v1: issue-93 investigation and interactive SLO constitution

Status: RATIFIED by WSP0 (charter `charters/expansion-workspace-responsiveness/WSP0.md`), consuming the accepted ORC0 trusted implementation-ledger cutover and the accepted MEM0 aggregate semantic memory budget and workload contract (`catalogs/semantic-memory-budget.toml`, `catalogs/semantic-memory-workload/`). This contract binds `contracts/product-experience.md` unchanged. Machine products: `tests/workspace-responsiveness/WSP0/products/`. Train plan: `plans/expansion-workspace-responsiveness.md`.

This contract is the shared measurement and budget constitution for large-workspace editor responsiveness. It ratifies the workload population, the measurement definitions, the reference-machine manifest requirements, the issue-93 evidence plan and the interactive SLO catalog before any optimization lands. It changes contract bytes only: WSP0 produces 0 production LOC and no runtime; instrumentation belongs to WSP1, backend identity and cancellation to PER0, application-performance attribution to WPF0 (owner text already assigns Verter responsiveness here). It reopens no producing owner: semantic algorithms, project resolution, format/lint logic, TypeScript projection and edit transactions stay with their existing owners, and every shipped valid feature remains RequiredCurrent.

## 1. Ownership and received outcomes

Final owner for this outcome: `expansion.workspace-responsiveness` — large-workspace responsiveness and Lapce hang elimination. Conflict domains `lsp_publication`, `performance_evidence`, `scheduler_admission` (all preregistered in `catalogs/conflict-domains.toml`; no new domain, no new orchestration system). Retirement obligation: this constitution is retired only when the interactive SLO catalog it ratifies is either measured-and-rebaselined by its owning successors through a recorded amendment or superseded by a successor constitution — never by an unrecorded deletion.

Received outcomes (contract dependencies, not resource locks):

- **ORC0** — implementation state resolves solely through the implementation ledger (`authority/state/implemented.toml`); this contract adds no second status store and no SHA/receipt/lease policy.
- **MEM0** — the aggregate memory budget, allocation-ownership inventory and fixed churn workload are the memory-side equal-work basis; the workload tiers below reuse MEM0's truthfulness discipline (measured figures marked measured, provisional figures marked with their named blocker, no guessed values).

## 2. Measurement definitions (machine product `workspace-responsiveness-contract.v1.json`)

Closed population, each row carrying unit, clock domain and instrumentation owner:

- `client_input_to_paint` — client-side UI responsiveness from input event to applied paint. Independent of server response time by definition; a server-side number can never substitute for it (WSP0-AC1).
- `server_request_to_response` — server handling time per request, with internal phase spans. Evidence for server work only; never an interactivity certificate.
- `process_tree_rss` — total retained RSS summed across the whole process tree (client, server, providers, workers).
- `language_service_incremental_rss` — incremental/retained RSS attributable to the language service, reported separately from the process-tree total (the class split MEM0's whole-process ceiling subdivides).
- `protocol_bytes_and_messages` — outbound bytes and message counts per scenario window, by notification family.
- `decode_apply_time` — client time decoding and applying server payloads.
- `worker_queue_depth_and_wait` — CPU/IO pool queue depth and wait under the existing scheduler admission (`scheduler_admission` domain).
- `provider_work` — work performed by provider engines (tsgo/tsserver modes), including their process cost.
- `cancellation_waste` — work started then discarded by cancellation, count and duration.

Every row is `not-instrumented` at ratification with owner WSP1; an unavailable metric is labelled, never guessed as zero (WSP0-AC-RESOURCE). Measurement binds the canonical immutable observation basis and the ProductReceiptBasis completeness states (`contracts/product-experience.md` §4): old-source, old-project, old-engine, old-worker and wrong-handle outcomes are rejected; partial, pending, unsupported, ambiguous, failed and cancelled are distinct from complete-empty.

## 3. Equal-work workload population (WSP0.2)

Closed matrix, each tier binding the correctness denominator: all RequiredCurrent features enabled, full diagnostic publication, full project membership, no truncation. A smaller feature set, hidden error truncation or reduced project membership is not an improvement (WSP0-AC2).

- **Scale tiers:** 1k / 10k / 50k authored-source projects.
- **Structural adversaries:** monorepo cross-references; a single 1 MiB source file; high diagnostic density; Unicode + CRLF content; ignored dependency trees present on disk.
- **Dynamic adversaries:** edit storms (rapid sequential authored edits); a slow-reading client (delayed reads applying backpressure).
- **Corpus law:** real pinned projects plus adversarial synthetic cases. The memory-side equal-work corpus is MEM0's frozen churn workload (`catalogs/semantic-memory-workload/`, 9 fixture sources at ratification); real project pins are recorded by WSP1 against a reference machine, never asserted here unmeasured.

## 4. Reference machines and version capture (machine product `reference-machine-manifests.v1.json`)

Every measured claim binds the manifest of the machine that produced it: CPU class, physical cores, memory, storage class, OS, client identity and version, server binary and version, provider engines with exact versions and modes, and the enabled-feature set (all RequiredCurrent on). Two manifest classes exist: `reference-client-machine` (real editor client) and `headless-ci-machine` (replay harness). The manifest population is empty at ratification — recording one is a WSP1 measured act; results from unmanifested or cross-manifest bases never compare (WSP0-AC-BASIS: incremental and fresh execution compare only on the same basis).

## 5. Issue-93 evidence plan (machine product `issue93-evidence-plan.v1.json`)

The original report (issue 93, opened 2026-07-25: large codebase, suspected excess LSP information) is an **unverified hypothesis**: no root cause is proved, and it stays labelled unverified — never fixed — until reproduced under the section-3 workloads (WSP0-AC3). Before any reproduction attempt the actual Lapce client, server, plugin and provider versions and enabled features are recorded; the plan's version-capture checklist is wholly unrecorded at ratification. Three clock domains — client, server, provider — carry the timeline law: each domain records its own monotonic spans, spans correlate only through shared trace identity, and a server timeline alone can never certify interactivity. All raw failed measurements are retained; anecdotal responsiveness claims are replaced by this constitution, not joined by it.

## 6. Interactive SLO catalog (machine product `interactive-slo-catalog.v1.json`)

Budgets are pinned **before** optimization, per operation and per scale tier, each row carrying a client-domain budget, a server-domain budget, and the correctness and equivalent-work denominators. At ratification every numeric budget is a `pinned-candidate-target`: authored by this constitution, validated by no measurement, promotable to a measured SLO only by WSP1 against a recorded reference machine. Downstream runtime-test ownership is bound to WSP1 (the registered in-train successor) for instrumentation and replay; PER0 and WPF0 consume this contract through their own charters as cross-train successors; JBT1 owns the real JetBrains harness; client-terminal claims require the actual clients (WSP0-AC-BASIS).

## 7. Exposure and forbidden routes

WSP0 introduces no new supported operation; the SLO rows measure existing product-surface operations only. Any operation a later node in this train exposes registers through DX1's descriptor contract with executable route, replay case and completeness states, obeying `HostExecutionClass`; promotion stays blocked until executable exposure exists (WSP0-AC-EXPOSURE). A protocol smoke or screenshot is not real-editor responsiveness proof.

Forbidden: certifying interactivity from server timings alone; improvements obtained by reduced features, hidden truncation or reduced membership; guessed zeros for missing telemetry; sleeps as readiness; stale publication; dropped diagnostics or weakened typing to meet timing; unexplained UI feature removal; a second semantic/type/project engine inside a client; regex/name-based semantic truth; hidden source uploads; arbitrary companion shell execution; general performance claims from compiler microbenchmarks; and publishing an unreproduced issue as fixed.

## 8. Scope and deletion population

WSP0 is contract-only: 0 production LOC, 0 production files, 0 related packages. The boundary is additive — the deletion population is empty; no route is displaced or retired by this node. The reference production budget is not permission: crossing it requires a scope amendment, not the rescope figure as allowance.
