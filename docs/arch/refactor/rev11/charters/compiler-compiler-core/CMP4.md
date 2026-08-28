<!-- unified-charter-v2
id=CMP4
name=Segmented emission, qualified artifacts, assembly, and host integration
phase=compiler
train=compiler.compiler-core
product=compiler_core
kind=cutover
semantic_role=delivery
class=compiler
predecessors=CMP3
conditional_predecessors=
owner=compiler.compiler-core:data-oriented common compiler substrate with framework-native planning
conflict_domains=compiler_execution,host_service_graph
resource_class=rust-mixed
review_profile=public-3
gate_profile=targeted-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=M
dispatchable=true
optional=false
release_gating=none
source_refs=source:compiler-proposal.md:L1049
external_requirements=
activation_gate=ORC0
charter=charters/compiler-compiler-core/CMP4.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CMP4 — Segmented emission, qualified artifacts, assembly, and host integration

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Segmented emission, qualified artifacts, assembly, and host integration. The current owner is **framework compiler emitters and per-node target dispatch**. The final and sole owner is **data-oriented common compiler substrate with framework-native planning**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_semantic/src`.
- Named API/data boundaries: `CompileRequest`, `CompilerPolicy`, `DemandSet`, `RegionId`, `EmissionSegment`, `ArtifactQualifier`.
- Mutation boundary: only the production surfaces and named API/data boundaries above; every changed path must be inside both that charter surface and the acquired conflict domain, and sibling ownership is excluded.

## Exact predecessor contracts

- **CMP3:** exact current receipt ID and digest for “Framework-native target planning and static physical execution”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** install the final shared compiler output path and remove framework topology from generic sessions.
- **Problem:** ad hoc string generation, map work on no-map paths, fixed SFC output envelopes, and session-level framework assembly limit performance and extensibility.
- **Solution and architecture decisions:**
- define target-owned logical EmitPlan segments:

## Acceptance IDs and discriminating proof

- **CMP4-AC1 — sole-owner proof:** add `cmp4_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **CMP4-AC2 — positive contract:** add `cmp4_publishes_exact_compilerequest`; assert exact identities, provenance, completeness, and deterministic ordering.
- **CMP4-AC3 — incremental equivalence:** add `cmp4_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **CMP4-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
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
- Performance budget: equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance is 0.0% unless an owning-authority amendment supplies exact replacement thresholds; after warmup, retained bytes may not increase across 100 identical requests.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `cargo nextest run -p verter_compiler -p verter_vue_conformance -p verter_svelte_conformance`
2. Run every final command in the bound `targeted-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `public-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three fresh distinct harness reviewers with assigned lenses, deterministic low|medium|high effort, exact task/provider/model bindings, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L1049`

## Reconciled source-plan contract

**Intent:** install the final shared compiler output path and remove framework topology from generic sessions.

**Problem:** ad hoc string generation, map work on no-map paths, fixed SFC output envelopes, and session-level framework assembly limit performance and extensibility.

**Solution and architecture decisions:**

- define target-owned logical `EmitPlan` segments:

  ```text
  SourceSlice
  GeneratedSlice + optional source anchor
  GeneratedUnmappedSlice
  StructuredInsertion
  ArtifactBoundary
  ```

- flatten once with exact or conservative sizing;
- generate runtime map segments during flattening only when requested;
- keep `NoMap` a physically specialized path with zero attributable map work;
- produce `CompileArtifactSet` with root, artifacts, relations, maps, diagnostics, provenance and exact basis;
- make the framework compiler own semantic module assembly;
- make framework-host integration own Vite/Rollup/HMR/virtual IDs/manifests and external-style stages;
- keep OXC internal;
- keep custom blocks opaque unless an admitted future integration consumes them.

**Suggested predecessor:** `CMP3`.

**Normative source decomposition:** emit segment model, text flatten/map specialization, artifact graph, framework assembly adapter migration, host integration migration, old-output deletion ledger.

**Acceptance:** text-only/no-map requests do not build maps or native ASTs; framework modules are complete before the generic session receives them; host-specific decorations do not alter framework semantic decisions; artifact relations support client/server/CSS/metadata without schema changes; output copies/allocations meet locked budgets.

**Forbidden:** one generic SFC bundle, session knowledge of `_sfc_main` or framework wrappers, raw callback preprocessors, one universal map, or external AST ABI.

**Deletion/abort:** adapters survive only with named VCP/SCP deletion owners; abort if artifact conversion loses map/provenance identity.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L1049-D55B1BF6A927

- Kind: `context`
- Source: `compiler-proposal.md:1049-1049`
- Applicability: `CMP4`
- Exact text SHA-256: `d55b1bf6a92764cfda268eb6f1bcddf4d18f14f6f945e18deebb11bf64caf4d4`

~~~~markdown
## `CMP4.md` — Segmented emission, qualified artifacts, assembly, and host integration
~~~~

### SRC-COMP-L1051-005A02561244

- Kind: `deletion`
- Source: `compiler-proposal.md:1051-1051`
- Applicability: `CMP4`
- Exact text SHA-256: `005a0256124478b2af9ad726e552603a15e41e6c78fbfa5b88f41461b59596bb`

~~~~markdown
**Intent:** install the final shared compiler output path and remove framework topology from generic sessions.
~~~~

### SRC-COMP-L1053-C00D14648FC6

- Kind: `context`
- Source: `compiler-proposal.md:1053-1053`
- Applicability: `CMP4`
- Exact text SHA-256: `c00d14648fc60593ca63ce570eaed3ac145d529cc11499116f83440ae6f10216`

~~~~markdown
**Problem:** ad hoc string generation, map work on no-map paths, fixed SFC output envelopes, and session-level framework assembly limit performance and extensibility.
~~~~

### SRC-COMP-L1055-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:1055-1055`
- Applicability: `CMP4`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1057-EFD460DEB644

- Kind: `context`
- Source: `compiler-proposal.md:1057-1057`
- Applicability: `CMP4`
- Exact text SHA-256: `efd460deb644824932964fe531963442a53d3de275d8db9bc4f7d8cb5530ea84`

~~~~markdown
- define target-owned logical `EmitPlan` segments:
~~~~

### SRC-COMP-L1059-F026D8EF29C2

- Kind: `context`
- Source: `compiler-proposal.md:1059-1065`
- Applicability: `CMP4`
- Exact text SHA-256: `f026d8ef29c2eb3180f2358b49a28d4b6221a3a5b730046590013cb8a9b6dae6`

~~~~markdown
```text
  SourceSlice
  GeneratedSlice + optional source anchor
  GeneratedUnmappedSlice
  StructuredInsertion
  ArtifactBoundary
  ```
~~~~

### SRC-COMP-L1067-0FB5C424256A

- Kind: `requirement`
- Source: `compiler-proposal.md:1067-1067`
- Applicability: `CMP4`
- Exact text SHA-256: `0fb5c424256ab5d3afa3ac4a17fd66a8137efda86dc78aa4fddef55a57c4dea3`

~~~~markdown
- flatten once with exact or conservative sizing;
~~~~

### SRC-COMP-L1068-F10910E0EB0E

- Kind: `requirement`
- Source: `compiler-proposal.md:1068-1068`
- Applicability: `CMP4`
- Exact text SHA-256: `f10910e0eb0e1b36dbec7c99d13ccdaa569ab247dceabf3638691e3e39780d13`

~~~~markdown
- generate runtime map segments during flattening only when requested;
~~~~

### SRC-COMP-L1069-E7F66BB30A2B

- Kind: `context`
- Source: `compiler-proposal.md:1069-1069`
- Applicability: `CMP4`
- Exact text SHA-256: `e7f66bb30a2b12b77f1cdf82395cbbc28a076dd304be519f3f7277721bf5d21a`

~~~~markdown
- keep `NoMap` a physically specialized path with zero attributable map work;
~~~~

### SRC-COMP-L1070-3FE321AAC288

- Kind: `requirement`
- Source: `compiler-proposal.md:1070-1070`
- Applicability: `CMP4`
- Exact text SHA-256: `3fe321aac2881d21db9cdcb65999a1a7cd7615152006bac377c560910e03d158`

~~~~markdown
- produce `CompileArtifactSet` with root, artifacts, relations, maps, diagnostics, provenance and exact basis;
~~~~

### SRC-COMP-L1071-43D57E73FDC3

- Kind: `context`
- Source: `compiler-proposal.md:1071-1071`
- Applicability: `CMP4`
- Exact text SHA-256: `43d57e73fdc3e1494a36d10425bdebdf470d68cb91321fc67d953a4aaddc9d64`

~~~~markdown
- make the framework compiler own semantic module assembly;
~~~~

### SRC-COMP-L1072-7E55B5C9AD89

- Kind: `context`
- Source: `compiler-proposal.md:1072-1072`
- Applicability: `CMP4`
- Exact text SHA-256: `7e55b5c9ad8944f22921855980ef9a99829041825e2c68cdef0947abf3a94ac1`

~~~~markdown
- make framework-host integration own Vite/Rollup/HMR/virtual IDs/manifests and external-style stages;
~~~~

### SRC-COMP-L1073-AFEDA8865EC7

- Kind: `context`
- Source: `compiler-proposal.md:1073-1073`
- Applicability: `CMP4`
- Exact text SHA-256: `afeda8865ec7eca426e18bf1d13bc27abc8b0e318415c32a3d01ddde323bfc9f`

~~~~markdown
- keep OXC internal;
~~~~

### SRC-COMP-L1074-873051319E00

- Kind: `context`
- Source: `compiler-proposal.md:1074-1074`
- Applicability: `CMP4`
- Exact text SHA-256: `873051319e0046872282a8f4382d3ecafa912df3050c4c535cadc2fdb6c4067b`

~~~~markdown
- keep custom blocks opaque unless an admitted future integration consumes them.
~~~~

### SRC-COMP-L1076-3E55132B5AF9

- Kind: `context`
- Source: `compiler-proposal.md:1076-1076`
- Applicability: `CMP4`
- Exact text SHA-256: `3e55132b5af94e37a68a8d80149a4268811dbce3befbbe6d78bc5d7dc82ec00c`

~~~~markdown
**Suggested predecessor:** `CMP3`.
~~~~

### SRC-COMP-L1078-0DF4653947EA

- Kind: `deletion`
- Source: `compiler-proposal.md:1078-1078`
- Applicability: `CMP4`
- Exact text SHA-256: `0df4653947ea38dd596446cb0ce7f6c155bd5e098ba1b79b7096d9651c8c3bb6`

~~~~markdown
**Suggested subblocks:** emit segment model, text flatten/map specialization, artifact graph, framework assembly adapter migration, host integration migration, old-output deletion ledger.
~~~~

### SRC-COMP-L1080-05BB2569AFAF

- Kind: `acceptance`
- Source: `compiler-proposal.md:1080-1080`
- Applicability: `CMP4`
- Exact text SHA-256: `05bb2569afaf86539f662615c40885941b4990f4862fc9038f3195d3a2d78e15`

~~~~markdown
**Acceptance:** text-only/no-map requests do not build maps or native ASTs; framework modules are complete before the generic session receives them; host-specific decorations do not alter framework semantic decisions; artifact relations support client/server/CSS/metadata without schema changes; output copies/allocations meet locked budgets.
~~~~

### SRC-COMP-L1082-981CCE632EAD

- Kind: `forbidden`
- Source: `compiler-proposal.md:1082-1082`
- Applicability: `CMP4`
- Exact text SHA-256: `981cce632eadba96f80588bf837cef718e0ea0f83cb9d497f4e5bd6e1c2c1131`

~~~~markdown
**Forbidden:** one generic SFC bundle, session knowledge of `_sfc_main` or framework wrappers, raw callback preprocessors, one universal map, or external AST ABI.
~~~~

### SRC-COMP-L1084-66013CDF6FEC

- Kind: `deletion`
- Source: `compiler-proposal.md:1084-1084`
- Applicability: `CMP4`
- Exact text SHA-256: `66013cdf6fec7d3f1f7dd3846457a6f4f137a601c2b1dc94f707fb9c91333267`

~~~~markdown
**Deletion/abort:** adapters survive only with named VCP/SCP deletion owners; abort if artifact conversion loses map/provenance identity.
~~~~

### SRC-COMP-L1086-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:1086-1086`
- Applicability: `CMP4`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
