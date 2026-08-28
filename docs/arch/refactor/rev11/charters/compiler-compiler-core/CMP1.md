<!-- unified-charter-v2
id=CMP1
name=Demand-refined semantic consumption and admissions
phase=compiler
train=compiler.compiler-core
product=compiler_core
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CMP0,CPER1
conditional_predecessors=
owner=compiler.compiler-core:data-oriented common compiler substrate with framework-native planning
conflict_domains=semantic_authority
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
source_refs=source:compiler-proposal.md:L956
external_requirements=
activation_gate=ORC0
charter=charters/compiler-compiler-core/CMP1.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CMP1 — Demand-refined semantic consumption and admissions

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Demand-refined semantic consumption and admissions. The current owner is **framework compiler emitters and per-node target dispatch**. The final and sole owner is **data-oriented common compiler substrate with framework-native planning**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_semantic/src`.
- Named API/data boundaries: `CompileRequest`, `CompilerPolicy`, `DemandSet`, `RegionId`, `EmissionSegment`, `ArtifactQualifier`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **CMP0:** exact current receipt ID and digest for “Compiler request, policy, compatibility, and identity contract”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CPER1:** exact current receipt ID and digest for “Compiler work ledger and lifetime attribution”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** ensure runtime compilation reuses the canonical framework analysis and computes only demanded fact families.
- **Problem:** compiler-local semantic analysis, repeated import/expression parsing, and a demand plan created after semantic work cause disagreement and unnecessary work.
- **Solution and architecture decisions:**
- specialize successor DEM0 into a finite compiler demand closure;

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **CMP1-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **CMP1-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **CMP1-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **CMP1-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `crates/verter_compiler/tests`, `crates/verter_vue_conformance/tests`, `crates/verter_svelte_conformance/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **dynamic dispatch inside node loops**.
- Delete or structurally reject: **whole-tree materialization fallback**.
- Delete or structurally reject: **unqualified artifact assembly**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_vue_conformance -p verter_svelte_conformance`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 2/2 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 2 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L956`

## Reconciled source-plan contract

**Intent:** ensure runtime compilation reuses the canonical framework analysis and computes only demanded fact families.

**Problem:** compiler-local semantic analysis, repeated import/expression parsing, and a demand plan created after semantic work cause disagreement and unnecessary work.

**Solution and architecture decisions:**

- specialize successor `DEM0` into a finite compiler demand closure;
- create exact reason edges from target/product to required parse, semantic, style, map, planning and emission capabilities;
- obtain `ParseAdmission` from each demanded frontend/region;
- ask the exact framework semantic authority for demanded fact families;
- obtain `SemanticAdmission` with exact source/fact basis and coverage;
- compose `CompileAdmission` without rerunning analysis;
- expose policy-restricted read-only compiler views over the same facts;
- allow `Default` component-local provenance through immutable aliases and literal canonical framework imports without loading external files;
- do not use ambient LSP/tsgo state;
- return `NeedInputs` for genuinely required external style stages and resume on the same basis.

**Suggested predecessors:** `CMP0`, `CPER1`.

**Normative source decomposition:** demand universe, closure engine, parse admission, semantic admission/view, compile admission/resume, duplicate-analysis deletion.

**Acceptance:** each exact expression region has one authoritative parsed representation after grammar selection; import/binding/reactivity/dependency facts have one framework owner; the compiler cannot call a second parser/analyzer; capabilities absent from closed demand have zero ledger work; alias-proven local reactivity reaches `Default` target planning.

**Forbidden:** per-node calls into external providers, field-wise fact merging, compiler-specific import scanning, late demand expansion after target execution begins, or a monolithic eager semantic snapshot.

**Deletion/abort:** delete duplicate compiler-local analysis only with fact/output parity; rescope any semantic fact that lacks one framework owner.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L956-7B9E25A055F5

- Kind: `context`
- Source: `compiler-proposal.md:956-956`
- Applicability: `CMP1`
- Exact text SHA-256: `7b9e25a055f59ee98469e983a12d76a52f2c48d4f681c099a763adddb9959506`

~~~~markdown
## `CMP1.md` — Demand-refined semantic consumption and admissions
~~~~

### SRC-COMP-L958-139F3B75F069

- Kind: `requirement`
- Source: `compiler-proposal.md:958-958`
- Applicability: `CMP1`
- Exact text SHA-256: `139f3b75f0694a60897bc7cf1239e9baea9189fcfcf0443c038a9c577a70a2d5`

~~~~markdown
**Intent:** ensure runtime compilation reuses the canonical framework analysis and computes only demanded fact families.
~~~~

### SRC-COMP-L960-F160EF349642

- Kind: `context`
- Source: `compiler-proposal.md:960-960`
- Applicability: `CMP1`
- Exact text SHA-256: `f160ef3496423a18bc641f6a4a805298a7eb9a9773d38b3c8bb9a4c7b33f31cf`

~~~~markdown
**Problem:** compiler-local semantic analysis, repeated import/expression parsing, and a demand plan created after semantic work cause disagreement and unnecessary work.
~~~~

### SRC-COMP-L962-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:962-962`
- Applicability: `CMP1`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L964-40F1F006AFEC

- Kind: `context`
- Source: `compiler-proposal.md:964-964`
- Applicability: `CMP1`
- Exact text SHA-256: `40f1f006afec724ea3b83b111475f5a6840ef2fe8e1d131020034d0079fb4c4f`

~~~~markdown
- specialize successor `DEM0` into a finite compiler demand closure;
~~~~

### SRC-COMP-L965-F81248E9F3FE

- Kind: `requirement`
- Source: `compiler-proposal.md:965-965`
- Applicability: `CMP1`
- Exact text SHA-256: `f81248e9f3fe7402437863cf01e67eb7361b6c40fad0f7955519009b03a478e0`

~~~~markdown
- create exact reason edges from target/product to required parse, semantic, style, map, planning and emission capabilities;
~~~~

### SRC-COMP-L966-856F04BE7F1A

- Kind: `context`
- Source: `compiler-proposal.md:966-966`
- Applicability: `CMP1`
- Exact text SHA-256: `856f04be7f1a727c301f97442f4ae60046b6097606f7389f912679a5e3d6ace9`

~~~~markdown
- obtain `ParseAdmission` from each demanded frontend/region;
~~~~

### SRC-COMP-L967-910B6CDC47B5

- Kind: `requirement`
- Source: `compiler-proposal.md:967-967`
- Applicability: `CMP1`
- Exact text SHA-256: `910b6cdc47b5988a05f1938d1626218c2a7df1898eae91ca42ddea057239c0e9`

~~~~markdown
- ask the exact framework semantic authority for demanded fact families;
~~~~

### SRC-COMP-L968-04EB3AB51E07

- Kind: `requirement`
- Source: `compiler-proposal.md:968-968`
- Applicability: `CMP1`
- Exact text SHA-256: `04eb3ab51e07d9e6aee845b6a0c581ab81600db6ff767a12a541aad7c49f36ab`

~~~~markdown
- obtain `SemanticAdmission` with exact source/fact basis and coverage;
~~~~

### SRC-COMP-L969-9D93926C5DE6

- Kind: `context`
- Source: `compiler-proposal.md:969-969`
- Applicability: `CMP1`
- Exact text SHA-256: `9d93926c5de6390829f5b20c9c3dbeac101ffcd9bc3a120f514c6c7baedc2e06`

~~~~markdown
- compose `CompileAdmission` without rerunning analysis;
~~~~

### SRC-COMP-L970-B25701236E50

- Kind: `requirement`
- Source: `compiler-proposal.md:970-970`
- Applicability: `CMP1`
- Exact text SHA-256: `b25701236e50805b397255155d8a40321ef1d63491ac02bae666c1e4b2d27522`

~~~~markdown
- expose policy-restricted read-only compiler views over the same facts;
~~~~

### SRC-COMP-L971-EF22C6881636

- Kind: `context`
- Source: `compiler-proposal.md:971-971`
- Applicability: `CMP1`
- Exact text SHA-256: `ef22c6881636bd169755fa719d3c519dd831e0dc2277ce730352a03a7238b0a7`

~~~~markdown
- allow `Default` component-local provenance through immutable aliases and literal canonical framework imports without loading external files;
~~~~

### SRC-COMP-L972-90D91DCE44C3

- Kind: `context`
- Source: `compiler-proposal.md:972-972`
- Applicability: `CMP1`
- Exact text SHA-256: `90d91dce44c3947506774062209643f560107cd0ffd91e74e57bae550aa1195f`

~~~~markdown
- do not use ambient LSP/tsgo state;
~~~~

### SRC-COMP-L973-E7F8655452BA

- Kind: `requirement`
- Source: `compiler-proposal.md:973-973`
- Applicability: `CMP1`
- Exact text SHA-256: `e7f8655452bac6a575a3f574082335c69ff277bf535cdc0d11b9921df82b68e3`

~~~~markdown
- return `NeedInputs` for genuinely required external style stages and resume on the same basis.
~~~~

### SRC-COMP-L975-0806E5E726F9

- Kind: `context`
- Source: `compiler-proposal.md:975-975`
- Applicability: `CMP1`
- Exact text SHA-256: `0806e5e726f9f096489f20004fd8db34179cbbca2061f6313b6106b92a99bb7b`

~~~~markdown
**Suggested predecessors:** `CMP0`, `CPER1`.
~~~~

### SRC-COMP-L977-AEE3F0CA187C

- Kind: `deletion`
- Source: `compiler-proposal.md:977-977`
- Applicability: `CMP1`
- Exact text SHA-256: `aee3f0ca187cacee8aabcbd775fa2f865bd07078c611549fceb3228e2ddbe120`

~~~~markdown
**Suggested subblocks:** demand universe, closure engine, parse admission, semantic admission/view, compile admission/resume, duplicate-analysis deletion.
~~~~

### SRC-COMP-L979-3E38B2129EAA

- Kind: `acceptance`
- Source: `compiler-proposal.md:979-979`
- Applicability: `CMP1`
- Exact text SHA-256: `3e38b2129eaac3c00ad0b46d64101f546014792addfbb84cc959191ae7810e0d`

~~~~markdown
**Acceptance:** each exact expression region has one authoritative parsed representation after grammar selection; import/binding/reactivity/dependency facts have one framework owner; the compiler cannot call a second parser/analyzer; capabilities absent from closed demand have zero ledger work; alias-proven local reactivity reaches `Default` target planning.
~~~~

### SRC-COMP-L981-4C9DC1F4E15A

- Kind: `forbidden`
- Source: `compiler-proposal.md:981-981`
- Applicability: `CMP1`
- Exact text SHA-256: `4c9dc1f4e15a7d7856da1876379e8d9950a9fb8a8cb8c19a7470c52206c1c3de`

~~~~markdown
**Forbidden:** per-node calls into external providers, field-wise fact merging, compiler-specific import scanning, late demand expansion after target execution begins, or a monolithic eager semantic snapshot.
~~~~

### SRC-COMP-L983-9E0B31C24B35

- Kind: `deletion`
- Source: `compiler-proposal.md:983-983`
- Applicability: `CMP1`
- Exact text SHA-256: `9e0b31c24b35abd19cc78f77f1beee727b1d9a14194a490ad7262d894be8e8b3`

~~~~markdown
**Deletion/abort:** delete duplicate compiler-local analysis only with fact/output parity; rescope any semantic fact that lacks one framework owner.
~~~~

### SRC-COMP-L985-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:985-985`
- Applicability: `CMP1`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
