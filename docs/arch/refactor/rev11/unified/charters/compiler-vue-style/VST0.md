<!-- unified-charter-v2
id=VST0
name=Vue framework style semantics and scope plan
phase=compiler
train=compiler.vue-style
product=vue_style
kind=implementation
semantic_role=delivery
class=compiler
predecessors=VCP1,J4
conditional_predecessors=
owner=compiler.vue-style:Vue-owned style semantics over canonical CSS facts
conflict_domains=style_semantics,semantic_authority,performance_evidence
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
size=M
dispatchable=true
optional=false
release_gating=none
source_refs=source:compiler-proposal.md:L1229
external_requirements=
activation_gate=ORC0
charter=charters/compiler-vue-style/VST0.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# VST0 — Vue framework style semantics and scope plan

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Vue framework style semantics and scope plan. The current owner is **Vue style scope planning**. The final and sole owner is **Vue-owned style semantics over canonical CSS facts**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src/framework/vue`, `crates/verter_css_syntax/src`.
- Named API/data boundaries: `VueStylePlan`, `ScopeId`, `SelectorQuery`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **VCP1:** exact current receipt ID and digest for “Canonical Vue semantic authority convergence”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **J4:** exact current receipt ID and digest for “Dialect preprocessor formatter recovery contract”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** consume J-owned style products and produce canonical Vue-specific style facts once.
- **Problem:** Vue style meaning can be extracted inside compiler/session code, external processing stages can be ambiguous, and template/style scope identity can diverge.
- **Solution and architecture decisions:**
- consume StyleSyntaxIr and J neutral facts only;

## Acceptance IDs and discriminating proof

- **VST0-AC1 — sole-owner proof:** add `vst0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **VST0-AC2 — positive contract:** add `vst0_publishes_exact_vuestyleplan`; assert exact identities, provenance, completeness, and deterministic ordering.
- **VST0-AC3 — incremental equivalence:** add `vst0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **VST0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_compiler/tests`, `crates/verter_css_syntax/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **Vue-local CSS parser**.
- Delete or structurally reject: **string selector matching**.
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

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1229`

## Reconciled source-plan contract

**Intent:** consume J-owned style products and produce canonical Vue-specific style facts once.

**Problem:** Vue style meaning can be extracted inside compiler/session code, external processing stages can be ambiguous, and template/style scope identity can diverge.

**Solution and architecture decisions:**

- consume `StyleSyntaxIr` and J neutral facts only;
- own Vue meaning for `v-bind()`, `:deep`, `:global`, `:slotted`, scoped selectors/keyframes, CSS Modules semantic exposure, and framework diagnostics;
- convert style expressions to source-backed `ExprId`/binding/dependency facts through the canonical Vue semantic authority;
- create one `VueComponentScopePlan` consumed by template, style, SSR and metadata paths;
- consume exact stage-qualified external preprocessor/PostCSS results and compose maps;
- perform no native Sass/Less/Stylus execution;
- do not implement selector-to-template matching in this block.

**Suggested predecessor:** `VCP1`.

**Normative source decomposition:** J integration, Vue selector/directive facts, CSS-variable expressions, scope/keyframe plan, CSS Modules semantic facts, external-stage/map integration.

**Acceptance:** no compiler/session raw CSS scan remains for migrated facts; template/style scope identity cannot disagree; preprocess-dependent work is exact `NeedInputs`; maps compose across all admitted stages; no second CSS grammar exists.

**Forbidden:** CSS reparsing, compiler-owned style semantics, opaque “processed CSS” strings, native preprocessors, or selector pruning.

**Deletion/abort:** delete replaced Vue style scanners/extractors after parity; stop if stage ordering cannot be proven.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1229-35A955C02589

- Kind: `context`
- Source: `compiler-proposal.md:1229-1229`
- Applicability: `VST0`
- Exact text SHA-256: `35a955c025890f1a7b7aad756226b2da9c1c91a1317646563c3f85c396f6f5cc`

~~~~markdown
## `VST0.md` — Vue framework style semantics and scope plan
~~~~

### SRC-COMP-L1231-4DB1265D90FB

- Kind: `context`
- Source: `compiler-proposal.md:1231-1231`
- Applicability: `VST0`
- Exact text SHA-256: `4db1265d90fb803cfe532f3298e941c4ad2eb5de45b31f8b75768f9427655229`

~~~~markdown
**Intent:** consume J-owned style products and produce canonical Vue-specific style facts once.
~~~~

### SRC-COMP-L1233-0E7C29E8DC29

- Kind: `context`
- Source: `compiler-proposal.md:1233-1233`
- Applicability: `VST0`
- Exact text SHA-256: `0e7c29e8dc29a8fd9a0c60753ae51cb63c22f74e1c79134d42a8601b30a67e93`

~~~~markdown
**Problem:** Vue style meaning can be extracted inside compiler/session code, external processing stages can be ambiguous, and template/style scope identity can diverge.
~~~~

### SRC-COMP-L1235-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1235-1235`
- Applicability: `VST0`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1237-E915EDE74014

- Kind: `context`
- Source: `compiler-proposal.md:1237-1237`
- Applicability: `VST0`
- Exact text SHA-256: `e915ede7401433aaf8d0b5588f95a72cf048883a7157f635c9a7ad786a3d155d`

~~~~markdown
- consume `StyleSyntaxIr` and J neutral facts only;
~~~~

### SRC-COMP-L1238-C39AA606E1C1

- Kind: `requirement`
- Source: `compiler-proposal.md:1238-1238`
- Applicability: `VST0`
- Exact text SHA-256: `c39aa606e1c118c9cd1975ee815fb5e2feb496fa9cff002b8445d9f4261f7ecb`

~~~~markdown
- own Vue meaning for `v-bind()`, `:deep`, `:global`, `:slotted`, scoped selectors/keyframes, CSS Modules semantic exposure, and framework diagnostics;
~~~~

### SRC-COMP-L1239-9E4597B1CE1E

- Kind: `requirement`
- Source: `compiler-proposal.md:1239-1239`
- Applicability: `VST0`
- Exact text SHA-256: `9e4597b1ce1e0a8fde07732e6deb85f2e3478bc219e2ccb1319d1e9c0ed441ba`

~~~~markdown
- convert style expressions to source-backed `ExprId`/binding/dependency facts through the canonical Vue semantic authority;
~~~~

### SRC-COMP-L1240-861040E632F6

- Kind: `context`
- Source: `compiler-proposal.md:1240-1240`
- Applicability: `VST0`
- Exact text SHA-256: `861040e632f6046690dccd7b3241bbe2b9f219a92d3a89a895b319742c5a53ab`

~~~~markdown
- create one `VueComponentScopePlan` consumed by template, style, SSR and metadata paths;
~~~~

### SRC-COMP-L1241-BB1002931352

- Kind: `requirement`
- Source: `compiler-proposal.md:1241-1241`
- Applicability: `VST0`
- Exact text SHA-256: `bb10029313520cc3992fb0d4153788cf4c970f591bb5ee0e142a2aa672725b50`

~~~~markdown
- consume exact stage-qualified external preprocessor/PostCSS results and compose maps;
~~~~

### SRC-COMP-L1242-C3C4997A5FF0

- Kind: `context`
- Source: `compiler-proposal.md:1242-1242`
- Applicability: `VST0`
- Exact text SHA-256: `c3c4997a5ff0ed9782693771de512fefe7693d662dac0c36dca494bcf489e2eb`

~~~~markdown
- perform no native Sass/Less/Stylus execution;
~~~~

### SRC-COMP-L1243-CC96299BA45B

- Kind: `context`
- Source: `compiler-proposal.md:1243-1243`
- Applicability: `VST0`
- Exact text SHA-256: `cc96299ba45b316ecf1f2bd7b7ed6527a82f0c34975e6579dd6bf63cd5eff454`

~~~~markdown
- do not implement selector-to-template matching in this block.
~~~~

### SRC-COMP-L1245-21BEDA004DA0

- Kind: `context`
- Source: `compiler-proposal.md:1245-1245`
- Applicability: `VST0`
- Exact text SHA-256: `21beda004da0eceadc30037990e9e697609cce20fa9ab17c7ac98c89da679849`

~~~~markdown
**Suggested predecessor:** `VCP1`.
~~~~

### SRC-COMP-L1247-1F80BE81BFE2

- Kind: `context`
- Source: `compiler-proposal.md:1247-1247`
- Applicability: `VST0`
- Exact text SHA-256: `1f80be81bfe2400c5e81bd17b8be110923d8345f888badd9707febaf6c53c8a2`

~~~~markdown
**Suggested subblocks:** J integration, Vue selector/directive facts, CSS-variable expressions, scope/keyframe plan, CSS Modules semantic facts, external-stage/map integration.
~~~~

### SRC-COMP-L1249-4061804FCDD9

- Kind: `acceptance`
- Source: `compiler-proposal.md:1249-1249`
- Applicability: `VST0`
- Exact text SHA-256: `4061804fcdd91996cb53db556cc65c197aa9a3c29511b0f29fe4b8fb2351800f`

~~~~markdown
**Acceptance:** no compiler/session raw CSS scan remains for migrated facts; template/style scope identity cannot disagree; preprocess-dependent work is exact `NeedInputs`; maps compose across all admitted stages; no second CSS grammar exists.
~~~~

### SRC-COMP-L1251-DC4F16CB30E5

- Kind: `forbidden`
- Source: `compiler-proposal.md:1251-1251`
- Applicability: `VST0`
- Exact text SHA-256: `dc4f16cb30e54aeeb82bf5e1d369b618d566de246cc57c2e73678963fdeb72bf`

~~~~markdown
**Forbidden:** CSS reparsing, compiler-owned style semantics, opaque “processed CSS” strings, native preprocessors, or selector pruning.
~~~~

### SRC-COMP-L1253-B1B7689A8677

- Kind: `deletion`
- Source: `compiler-proposal.md:1253-1253`
- Applicability: `VST0`
- Exact text SHA-256: `b1b7689a8677e2539ea99ace46edddb23196f4c2a7abc575f1f3edc5c455eccc`

~~~~markdown
**Deletion/abort:** delete replaced Vue style scanners/extractors after parity; stop if stage ordering cannot be proven.
~~~~

### SRC-COMP-L1255-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1255-1255`
- Applicability: `VST0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
