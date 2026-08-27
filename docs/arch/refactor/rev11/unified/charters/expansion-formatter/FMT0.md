<!-- unified-charter-v2
id=FMT0
name=Full formatter implementation lock
phase=expansion
train=expansion.formatter
product=formatter
kind=lock
semantic_role=delivery
class=successor
predecessors=FMK0
conditional_predecessors=
owner=expansion.formatter:native document algebra and carrier-composed formatter service
conflict_domains=formatter_service
resource_class=docs-light
review_profile=architecture-3
gate_profile=docs-domain
size=M
dispatchable=true
optional=false
release_gating=none
source_refs=source:successor-expansion.md:L1318
external_requirements=
activation_gate=ORC0
charter=charters/expansion-formatter/FMT0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# FMT0 — Full formatter implementation lock

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Full formatter implementation lock. The current owner is **fragmented formatting adapters**. The final and sole owner is **native document algebra and carrier-composed formatter service**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_language/src`, `crates/verter_session/src`, `packages/language-shared/src`.
- Named API/data boundaries: `Doc`, `FormatRequest`, `FormatEdit`, `CursorMap`, `FormatterConfig`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **FMK0:** exact current receipt ID and digest for “Formatter ownership, composition, and compatibility contract”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** freeze exact Prettier compatibility, native behavior, corpora, performance, and current formatter deletion before printer work.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **format-after-build string surgery**, **second semantic parser for formatting** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

- **FMT0-AC1 — sole-owner proof:** add `fmt0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **FMT0-AC2 — positive contract:** add `fmt0_publishes_exact_doc`; assert exact identities, provenance, completeness, and deterministic ordering.
- **FMT0-AC3 — incremental equivalence:** add `fmt0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **FMT0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_language/tests`, `packages/language-shared`.

## Deletions and forbidden designs

- Delete or structurally reject: **format-after-build string surgery**.
- Delete or structurally reject: **second semantic parser for formatting**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance is 0.0% unless an owning-authority amendment supplies exact replacement thresholds; after warmup, retained bytes may not increase across 100 identical requests.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_language -p verter_session`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1318`

## Reconciled source-plan contract

**Intent:** freeze exact Prettier compatibility, native behavior, corpora, performance, and current formatter deletion before printer work.
**Predecessors:** `FMK0`.
**Subblocks:** (1) pin Prettier version/options and Vue/Svelte/HTML/JS/TS/JSX/CSS corpora; (2) enumerate exact/verter-default/unsupported cells; (3) pin recovery/range/cursor/edit/map behavior; (4) record every current whitespace-only formatter route and consumer; (5) lock latency/allocation/idempotence/stability gates; (6) assign one later cutover/deletion owner per carrier.
**Acceptance:** criteria are immutable and cover full SFC blocks plus embedded contents; every intentional divergence has a preexisting regression and rationale.
**Forbidden:** oxfmt options, post-implementation compatibility choices, or delegating production output.
**Deletion/abort:** no implementation; rescope if a syntax view is too lossy for exact authored trivia.

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1318-48D3315EDB8D

- Kind: `context`
- Source: `successor-expansion.md:1318-1318`
- Applicability: `FMT0`
- Exact text SHA-256: `48d3315edb8d3069151615e3749df4267cbbab5f92c0061997d08bf0a21c5da8`

~~~~markdown
### `FMT0.md` — Full formatter implementation lock
~~~~

### SRC-EXP-L1320-B113BD081C41

- Kind: `forbidden`
- Source: `successor-expansion.md:1320-1325`
- Applicability: `FMT0`
- Exact text SHA-256: `b113bd081c417fa00998bdcb3832b9b287514599cbc20be73e0eaf7ea35106dd`

~~~~markdown
**Intent:** freeze exact Prettier compatibility, native behavior, corpora, performance, and current formatter deletion before printer work.
**Predecessors:** `FMK0`.
**Subblocks:** (1) pin Prettier version/options and Vue/Svelte/HTML/JS/TS/JSX/CSS corpora; (2) enumerate exact/verter-default/unsupported cells; (3) pin recovery/range/cursor/edit/map behavior; (4) record every current whitespace-only formatter route and consumer; (5) lock latency/allocation/idempotence/stability gates; (6) assign one later cutover/deletion owner per carrier.
**Acceptance:** criteria are immutable and cover full SFC blocks plus embedded contents; every intentional divergence has a preexisting regression and rationale.
**Forbidden:** oxfmt options, post-implementation compatibility choices, or delegating production output.
**Deletion/abort:** no implementation; rescope if a syntax view is too lossy for exact authored trivia.
~~~~
