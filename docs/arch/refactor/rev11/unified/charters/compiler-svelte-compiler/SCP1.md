<!-- unified-charter-v2
id=SCP1
name=Canonical Svelte semantic authority convergence
phase=compiler
train=compiler.svelte-compiler
product=svelte_compiler
kind=convergence
semantic_role=convergence
class=compiler
predecessors=SCP0
conditional_predecessors=
owner=compiler.svelte-compiler:Svelte-owned Default compiler cells over shared compiler substrate
conflict_domains=semantic_authority,svelte_product
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
size=S
dispatchable=true
optional=false
release_gating=none
source_refs=source:compiler-proposal.md:L1453
external_requirements=
activation_gate=ORC0
charter=charters/compiler-svelte-compiler/SCP1.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# SCP1 — Canonical Svelte semantic authority convergence

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Canonical Svelte semantic authority convergence. The current owner is **Svelte runtime emitter and assembly paths**. The final and sole owner is **Svelte-owned Default compiler cells over shared compiler substrate**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/svelte`, `crates/verter_svelte_conformance`, `packages/svelte-runtime-tests`.
- Named API/data boundaries: `SvelteSemanticSnapshot`, `SvelteCompilePlan`, `SvelteTarget`, `SvelteArtifactSet`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **SCP0:** exact current receipt ID and digest for “Exact Svelte Default compiler lock”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** make one Svelte semantic authority own all target-independent framework meaning.
- **Problem:** client/server/style/compiler paths can duplicate runes, stores, scope, dependency, mutation and template analyses.
- **Solution and architecture decisions:**
- one authority for runes/legacy mode, scopes, bindings, references, mutations, stores, runes, read/write/dependency sets, purity/staticness, template scopes, components/elements, actions/transitions/animations/bindings and style cross-language facts;

## Acceptance IDs and discriminating proof

- **SCP1-AC1 — sole-owner proof:** add `scp1_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **SCP1-AC2 — positive contract:** add `scp1_publishes_exact_sveltesemanticsnapshot`; assert exact identities, provenance, completeness, and deterministic ordering.
- **SCP1-AC3 — incremental equivalence:** add `scp1_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **SCP1-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_svelte_conformance/tests`, `packages/svelte-runtime-tests/test`.

## Deletions and forbidden designs

- Delete or structurally reject: **legacy Svelte emitter route**.
- Delete or structurally reject: **per-target prerequisite duplication**.
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

1. `cargo nextest run -p verter_compiler -p verter_svelte_conformance`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1453`

## Reconciled source-plan contract

**Intent:** make one Svelte semantic authority own all target-independent framework meaning.

**Problem:** client/server/style/compiler paths can duplicate runes, stores, scope, dependency, mutation and template analyses.

**Solution and architecture decisions:**

- one authority for runes/legacy mode, scopes, bindings, references, mutations, stores, runes, read/write/dependency sets, purity/staticness, template scopes, components/elements, actions/transitions/animations/bindings and style cross-language facts;
- shared `verter_analysis`/`type_info` machinery, framework-owned interpretation;
- compact dense hot facts and sparse explanations;
- one authoritative script/expression parse and import analysis;
- client/server/module/style consumers use policy-restricted views, never duplicate analysis;
- `Default` performs all required component-local semantics and no project-wide investigation.

**Suggested predecessor:** `SCP0`.

**Normative source decomposition:** script/rune/store facts, scopes/bindings/dependencies, template/component/directive facts, style cross-language hooks, compact storage, duplicate-analysis deletion.

**Acceptance:** client/server/style agree on every shared fact; no raw source semantic searches or downstream reparses remain; unknown dynamic cases fail open/conservative; work ledger shows one fact production.

**Forbidden:** compiler-owned Svelte semantics, universal reactivity schema, source-string structural scanning, or project optimization.

**Deletion/abort:** delete duplicate facts only after cross-consumer parity.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1453-8A81B511151A

- Kind: `context`
- Source: `compiler-proposal.md:1453-1453`
- Applicability: `SCP1`
- Exact text SHA-256: `8a81b511151a02b7857069db9bfc1b099a1b1e6efbd54bef6b85db337334389d`

~~~~markdown
## `SCP1.md` — Canonical Svelte semantic authority convergence
~~~~

### SRC-COMP-L1455-5695E173AE5E

- Kind: `context`
- Source: `compiler-proposal.md:1455-1455`
- Applicability: `SCP1`
- Exact text SHA-256: `5695e173ae5e93acf855f235523f8687e3d19433e1e8092cba0b9160a1d251cf`

~~~~markdown
**Intent:** make one Svelte semantic authority own all target-independent framework meaning.
~~~~

### SRC-COMP-L1457-B6C3392648E9

- Kind: `context`
- Source: `compiler-proposal.md:1457-1457`
- Applicability: `SCP1`
- Exact text SHA-256: `b6c3392648e91921a7f6f998face6b1b1910c2c956652ec13bb3742058401b7f`

~~~~markdown
**Problem:** client/server/style/compiler paths can duplicate runes, stores, scope, dependency, mutation and template analyses.
~~~~

### SRC-COMP-L1459-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1459-1459`
- Applicability: `SCP1`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1461-EB7E957BF2F5

- Kind: `requirement`
- Source: `compiler-proposal.md:1461-1461`
- Applicability: `SCP1`
- Exact text SHA-256: `eb7e957bf2f51b17c3c9434e46b4f995efa7bf08a944d1ce36376ad05fa63146`

~~~~markdown
- one authority for runes/legacy mode, scopes, bindings, references, mutations, stores, runes, read/write/dependency sets, purity/staticness, template scopes, components/elements, actions/transitions/animations/bindings and style cross-language facts;
~~~~

### SRC-COMP-L1462-00098E725F00

- Kind: `context`
- Source: `compiler-proposal.md:1462-1462`
- Applicability: `SCP1`
- Exact text SHA-256: `00098e725f00c1ba4b3028c10f93abc9ead2982fce45d1e4f3cf9d1730447866`

~~~~markdown
- shared `verter_analysis`/`type_info` machinery, framework-owned interpretation;
~~~~

### SRC-COMP-L1463-FB8D1528B370

- Kind: `context`
- Source: `compiler-proposal.md:1463-1463`
- Applicability: `SCP1`
- Exact text SHA-256: `fb8d1528b37085e21f3213237e4c8e5caadfd963ead7b54dd7e941a6240e1384`

~~~~markdown
- compact dense hot facts and sparse explanations;
~~~~

### SRC-COMP-L1464-01D655EEEBA3

- Kind: `context`
- Source: `compiler-proposal.md:1464-1464`
- Applicability: `SCP1`
- Exact text SHA-256: `01d655eeeba33ce1042c6a7658048466bd62b6405b712758ca403938f393ffce`

~~~~markdown
- one authoritative script/expression parse and import analysis;
~~~~

### SRC-COMP-L1465-677819916B6A

- Kind: `forbidden`
- Source: `compiler-proposal.md:1465-1465`
- Applicability: `SCP1`
- Exact text SHA-256: `677819916b6af9a9f294fae565df7d4b66e896078d0ed54b3f9618dc798444e4`

~~~~markdown
- client/server/module/style consumers use policy-restricted views, never duplicate analysis;
~~~~

### SRC-COMP-L1466-50E579A0FF07

- Kind: `requirement`
- Source: `compiler-proposal.md:1466-1466`
- Applicability: `SCP1`
- Exact text SHA-256: `50e579a0ff07a7be7345a041903319a35f6d4b66a7b59134ccb23a7741868ffb`

~~~~markdown
- `Default` performs all required component-local semantics and no project-wide investigation.
~~~~

### SRC-COMP-L1468-2D4DEA423422

- Kind: `context`
- Source: `compiler-proposal.md:1468-1468`
- Applicability: `SCP1`
- Exact text SHA-256: `2d4dea423422ad163b14b5b9646dee0dd2a4925a2ce1777e2a9e040be31cb922`

~~~~markdown
**Suggested predecessor:** `SCP0`.
~~~~

### SRC-COMP-L1470-67EB3F726DC5

- Kind: `deletion`
- Source: `compiler-proposal.md:1470-1470`
- Applicability: `SCP1`
- Exact text SHA-256: `67eb3f726dc55455791ba0863975683a263ec701eea6f0a2944b1df124725fa6`

~~~~markdown
**Suggested subblocks:** script/rune/store facts, scopes/bindings/dependencies, template/component/directive facts, style cross-language hooks, compact storage, duplicate-analysis deletion.
~~~~

### SRC-COMP-L1472-18257E86C6F5

- Kind: `acceptance`
- Source: `compiler-proposal.md:1472-1472`
- Applicability: `SCP1`
- Exact text SHA-256: `18257e86c6f5164c66f6c5af60dd9dd7002213cfee40600108a229c9266fb029`

~~~~markdown
**Acceptance:** client/server/style agree on every shared fact; no raw source semantic searches or downstream reparses remain; unknown dynamic cases fail open/conservative; work ledger shows one fact production.
~~~~

### SRC-COMP-L1474-43D76DF3ECDC

- Kind: `forbidden`
- Source: `compiler-proposal.md:1474-1474`
- Applicability: `SCP1`
- Exact text SHA-256: `43d76df3ecdc917152ad0852c082b991b040b8ff48e1bccbb520957580a32dd8`

~~~~markdown
**Forbidden:** compiler-owned Svelte semantics, universal reactivity schema, source-string structural scanning, or project optimization.
~~~~

### SRC-COMP-L1476-8E1A64E44A24

- Kind: `deletion`
- Source: `compiler-proposal.md:1476-1476`
- Applicability: `SCP1`
- Exact text SHA-256: `8e1a64e44a24937afa7732f9f528a9d71514e6132567c853e499d1461fcf77c1`

~~~~markdown
**Deletion/abort:** delete duplicate facts only after cross-consumer parity.
~~~~

### SRC-COMP-L1478-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1478-1478`
- Applicability: `SCP1`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
