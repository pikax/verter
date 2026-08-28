<!-- unified-charter-v2
id=FMT4
name=Formatter LSP/public parity, conformance, and promotion
phase=expansion
train=expansion.formatter
product=formatter
kind=terminal
semantic_role=delivery
class=successor
predecessors=FMT3,PUB0,PER0
conditional_predecessors=
owner=expansion.formatter:native document algebra and carrier-composed formatter service
conflict_domains=lsp_publication,formatter_service,public_protocol
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=S
dispatchable=true
optional=false
release_gating=product
source_refs=source:successor-expansion.md:L1390
external_requirements=
activation_gate=ORC0
charter=charters/expansion-formatter/FMT4.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# FMT4 — Formatter LSP/public parity, conformance, and promotion

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Formatter LSP/public parity, conformance, and promotion. The current owner is **fragmented formatting adapters**. The final and sole owner is **native document algebra and carrier-composed formatter service**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `packages/language-shared/src`.
- Named API/data boundaries: `Doc`, `FormatRequest`, `FormatEdit`, `CursorMap`, `FormatterConfig`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **FMT3:** exact current receipt ID and digest for “Formatter service composition cutover”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PUB0:** exact current receipt ID and digest for “Versioned public request/result and capability truth”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PER0:** exact current receipt ID and digest for “Cache/backend identity, cancellation, budgets, and zero work”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** expose and independently promote the formatter across all applicable surfaces.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **format-after-build string surgery**, **second semantic parser for formatting** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

- **FMT4-AC1 — sole-owner proof:** add `fmt4_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **FMT4-AC2 — positive contract:** add `fmt4_publishes_exact_doc`; assert exact identities, provenance, completeness, and deterministic ordering.
- **FMT4-AC3 — incremental equivalence:** add `fmt4_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **FMT4-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_language/tests`, `packages/language-shared`.

## Deletions and forbidden designs

- Delete or structurally reject: **format-after-build string surgery**.
- Delete or structurally reject: **second semantic parser for formatting**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 300 production LOC, 3 production files, 1 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance is 0.0% unless an owning-authority amendment supplies exact replacement thresholds; after warmup, retained bytes may not increase across 100 identical requests.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_language -p verter_session`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1390`

## Reconciled source-plan contract

**Intent:** expose and independently promote the formatter across all applicable surfaces.
**Predecessors:** `FMT3`, `PUB0`, `PER0`.
**Subblocks:** (1) Rust/NAPI/WASM request/result; (2) LSP document/range/on-type cells where applicable; (3) MCP formatting service cells; (4) config/ignore/override provenance; (5) cold/warm/large-file/RSS/cancellation/zero-work tests; (6) dogfood and exact-candidate reviews.
**Acceptance:** Rust/NAPI/WASM/LSP/MCP surfaces agree on output/edits/maps; LSP capability is registered only under its ownership mask; repository dogfood produces a reviewed finite diff; CLI remains explicitly unavailable until `CLIF0`; formatter maturity promotes independently.
**Forbidden:** waiting for future verticals, hiding unsupported custom blocks, or using lint fixes to make formatter conformance pass.
**Deletion/abort:** delete only named obsolete public formatter façade APIs/packages assigned to `FMT4` by the `UAK0` ledger after zero-consumer/generated-reference proof; printer and routing deletions remain with their earlier sole owners. Any failing cell returns to its printer/composition owner.

## 13. Native lint product train

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1390-877B3B6566DF

- Kind: `context`
- Source: `successor-expansion.md:1390-1390`
- Applicability: `FMT4`
- Exact text SHA-256: `877b3b6566df09580c01f634b62cf680e5eb121554dc7c57b2789f0d1e18a434`

~~~~markdown
### `FMT4.md` — Formatter LSP/public parity, conformance, and promotion
~~~~

### SRC-EXP-L1392-250320FF6ACD

- Kind: `forbidden`
- Source: `successor-expansion.md:1392-1397`
- Applicability: `FMT4`
- Exact text SHA-256: `250320ff6acd9f3e2d51b825ee42df59b454ce35b4be16e9ea3b357afcd8cfcf`

~~~~markdown
**Intent:** expose and independently promote the formatter across all applicable surfaces.
**Predecessors:** `FMT3`, `PUB0`, `PER0`.
**Subblocks:** (1) Rust/NAPI/WASM request/result; (2) LSP document/range/on-type cells where applicable; (3) MCP formatting service cells; (4) config/ignore/override provenance; (5) cold/warm/large-file/RSS/cancellation/zero-work tests; (6) dogfood and exact-candidate reviews.
**Acceptance:** Rust/NAPI/WASM/LSP/MCP surfaces agree on output/edits/maps; LSP capability is registered only under its ownership mask; repository dogfood produces a reviewed finite diff; CLI remains explicitly unavailable until `CLIF0`; formatter maturity promotes independently.
**Forbidden:** waiting for future verticals, hiding unsupported custom blocks, or using lint fixes to make formatter conformance pass.
**Deletion/abort:** delete only named obsolete public formatter façade APIs/packages assigned to `FMT4` by the `UAK0` ledger after zero-consumer/generated-reference proof; printer and routing deletions remain with their earlier sole owners. Any failing cell returns to its printer/composition owner.
~~~~
