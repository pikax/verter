<!-- unified-charter-v2
id=SST0
name=Svelte framework style semantics and source-stage integration
phase=compiler
train=compiler.svelte-style
product=svelte_style
kind=implementation
semantic_role=delivery
class=compiler
predecessors=SCP1,J4
conditional_predecessors=
owner=compiler.svelte-style:Svelte-owned adaptive matcher over canonical CSS/template facts
conflict_domains=source_lineage,style_semantics,semantic_authority
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
implementation_effort_min=medium
implementation_effort_default=medium
review_effort_min=medium
review_effort_default=medium
verification_effort_min=medium
verification_effort_default=medium
confirmation_effort_min=medium
confirmation_effort_default=medium
size=M
dispatchable=true
optional=false
release_gating=none
source_refs=source:compiler-proposal.md:L1520
external_requirements=
activation_gate=ORC0
charter=charters/compiler-svelte-style/SST0.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# SST0 — Svelte framework style semantics and source-stage integration

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Svelte framework style semantics and source-stage integration. The current owner is **Svelte style matching and source-stage glue**. The final and sole owner is **Svelte-owned adaptive matcher over canonical CSS/template facts**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/svelte`, `crates/verter_css_syntax/src`.
- Named API/data boundaries: `SvelteStylePlan`, `CandidateIndex`, `StyleMatchFact`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **SCP1:** exact current receipt ID and digest for “Canonical Svelte semantic authority convergence”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **J4:** exact current receipt ID and digest for “Dialect preprocessor formatter recovery contract”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** consume J-owned CSS products and establish one Svelte style-semantic authority before matching/planning.
- **Problem:** a compiler-local CSS grammar/matcher or ambiguous preprocessing stage can create duplicate syntax and incorrect map/scoping behavior.
- **Solution and architecture decisions:**
- consume J StyleSyntaxIr and neutral facts;

## Acceptance IDs and discriminating proof

- **SST0-AC1 — sole-owner proof:** add `sst0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **SST0-AC2 — positive contract:** add `sst0_publishes_exact_sveltestyleplan`; assert exact identities, provenance, completeness, and deterministic ordering.
- **SST0-AC3 — incremental equivalence:** add `sst0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **SST0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_compiler/tests`, `crates/verter_css_syntax/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **Svelte-local CSS parser**.
- Delete or structurally reject: **unbounded selector scan**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance is 0.0% unless an owning-authority amendment supplies exact replacement thresholds; after warmup, retained bytes may not increase across 100 identical requests.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_css_syntax`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `semantic-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1520`

## Reconciled source-plan contract

**Intent:** consume J-owned CSS products and establish one Svelte style-semantic authority before matching/planning.

**Problem:** a compiler-local CSS grammar/matcher or ambiguous preprocessing stage can create duplicate syntax and incorrect map/scoping behavior.

**Solution and architecture decisions:**

- consume J `StyleSyntaxIr` and neutral facts;
- own Svelte-specific global/local semantics, keyframe meaning, scope-hash inputs, style injection/extraction facts and diagnostics;
- connect processed CSS to authored dialect through exact external-stage maps/read sets;
- no native preprocessors;
- create one style identity and scope basis shared by client/server/CSS emission;
- expose the exact inputs required by selector matching without performing it here.

**Suggested predecessor:** `SCP1`.

**Normative source decomposition:** J integration, framework style facts, scope/hash identity, external-stage/maps, client/server style-demand contract, legacy parser/scanner deletion.

**Acceptance:** one CSS parse per exact style block/grammar product; no compiler-local grammar/scanner; client/server share style identity; preprocessing ambiguity returns `NeedInputs`.

**Forbidden:** raw CSS rescans, runtime-IR-owned style semantics, native preprocessors, or selector pruning before exact matching.

**Deletion/abort:** delete competing CSS grammar/scanners after parity; stop if authored/processed map basis is incomplete.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1520-9B7600238C92

- Kind: `context`
- Source: `compiler-proposal.md:1520-1520`
- Applicability: `SST0`
- Exact text SHA-256: `9b7600238c92243746170bfbfe7d912983350e0a2f43ca646cafe76eb037f12d`

~~~~markdown
## `SST0.md` — Svelte framework style semantics and source-stage integration
~~~~

### SRC-COMP-L1522-11C374F04BBB

- Kind: `context`
- Source: `compiler-proposal.md:1522-1522`
- Applicability: `SST0`
- Exact text SHA-256: `11c374f04bbb3bc98da5b65830516a94d85b5309db5de351cfb0d492d2a89e2a`

~~~~markdown
**Intent:** consume J-owned CSS products and establish one Svelte style-semantic authority before matching/planning.
~~~~

### SRC-COMP-L1524-EEBDFAE6E625

- Kind: `context`
- Source: `compiler-proposal.md:1524-1524`
- Applicability: `SST0`
- Exact text SHA-256: `eebdfae6e625024e865f896f49c48188e04aaf2480dd0803923ad2b8c35852b9`

~~~~markdown
**Problem:** a compiler-local CSS grammar/matcher or ambiguous preprocessing stage can create duplicate syntax and incorrect map/scoping behavior.
~~~~

### SRC-COMP-L1526-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1526-1526`
- Applicability: `SST0`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1528-62E4E219E3FE

- Kind: `context`
- Source: `compiler-proposal.md:1528-1528`
- Applicability: `SST0`
- Exact text SHA-256: `62e4e219e3fe80dfd19c486b24aaaa784b352604a7ae153d3b191e9a248bb88c`

~~~~markdown
- consume J `StyleSyntaxIr` and neutral facts;
~~~~

### SRC-COMP-L1529-87B3117B65F6

- Kind: `context`
- Source: `compiler-proposal.md:1529-1529`
- Applicability: `SST0`
- Exact text SHA-256: `87b3117b65f6a7088ee5a1dd8920ec7f056c9ad2c26104d6042a833885992f47`

~~~~markdown
- own Svelte-specific global/local semantics, keyframe meaning, scope-hash inputs, style injection/extraction facts and diagnostics;
~~~~

### SRC-COMP-L1530-BEC067C81C99

- Kind: `requirement`
- Source: `compiler-proposal.md:1530-1530`
- Applicability: `SST0`
- Exact text SHA-256: `bec067c81c99d6a530acc851f3f82c1067e2e2d69fadbe2197176ca196717d99`

~~~~markdown
- connect processed CSS to authored dialect through exact external-stage maps/read sets;
~~~~

### SRC-COMP-L1531-9E29FC0EE4BD

- Kind: `context`
- Source: `compiler-proposal.md:1531-1531`
- Applicability: `SST0`
- Exact text SHA-256: `9e29fc0ee4bdedaf8ed1baf14a3354a50bbc235bc375ba2452a5f00a3ea6ed63`

~~~~markdown
- no native preprocessors;
~~~~

### SRC-COMP-L1532-33C536690F2B

- Kind: `context`
- Source: `compiler-proposal.md:1532-1532`
- Applicability: `SST0`
- Exact text SHA-256: `33c536690f2b23079ca9c7e409c0901529f9268346fc9f34eba524cd9df75c6c`

~~~~markdown
- create one style identity and scope basis shared by client/server/CSS emission;
~~~~

### SRC-COMP-L1533-081F19C27DB1

- Kind: `requirement`
- Source: `compiler-proposal.md:1533-1533`
- Applicability: `SST0`
- Exact text SHA-256: `081f19c27db121c4b986591c2175fd7d388c8beb2ece2bf826721058af4706bf`

~~~~markdown
- expose the exact inputs required by selector matching without performing it here.
~~~~

### SRC-COMP-L1535-5D1CE4FA2351

- Kind: `context`
- Source: `compiler-proposal.md:1535-1535`
- Applicability: `SST0`
- Exact text SHA-256: `5d1ce4fa23518e2a2e9f83c3fe4cc011976d9189340d27dedecf5b6e19b2722b`

~~~~markdown
**Suggested predecessor:** `SCP1`.
~~~~

### SRC-COMP-L1537-93540A246EB5

- Kind: `deletion`
- Source: `compiler-proposal.md:1537-1537`
- Applicability: `SST0`
- Exact text SHA-256: `93540a246eb5542a329d97e44ac2ab46f4f74b25c224027d9ead0dea9dca0b0e`

~~~~markdown
**Suggested subblocks:** J integration, framework style facts, scope/hash identity, external-stage/maps, client/server style-demand contract, legacy parser/scanner deletion.
~~~~

### SRC-COMP-L1539-C8330BC94306

- Kind: `acceptance`
- Source: `compiler-proposal.md:1539-1539`
- Applicability: `SST0`
- Exact text SHA-256: `c8330bc943063f65f3422a1e1cbbfcbc32b05e5e8d0e03274071abb78a62f0d4`

~~~~markdown
**Acceptance:** one CSS parse per exact style block/grammar product; no compiler-local grammar/scanner; client/server share style identity; preprocessing ambiguity returns `NeedInputs`.
~~~~

### SRC-COMP-L1541-71DE4812C345

- Kind: `forbidden`
- Source: `compiler-proposal.md:1541-1541`
- Applicability: `SST0`
- Exact text SHA-256: `71de4812c345d0454398834cfc67d8d06e82db93618d672dc85580916c44e467`

~~~~markdown
**Forbidden:** raw CSS rescans, runtime-IR-owned style semantics, native preprocessors, or selector pruning before exact matching.
~~~~

### SRC-COMP-L1543-BA7D73770D5E

- Kind: `deletion`
- Source: `compiler-proposal.md:1543-1543`
- Applicability: `SST0`
- Exact text SHA-256: `ba7d73770d5e57e03693e1718200f39429286dad6b8a634b31d777aeb5a93e3e`

~~~~markdown
**Deletion/abort:** delete competing CSS grammar/scanners after parity; stop if authored/processed map basis is incomplete.
~~~~

### SRC-COMP-L1545-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1545-1545`
- Applicability: `SST0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
