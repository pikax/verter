<!-- unified-charter-v2
id=CMP0
name=Compiler request, policy, compatibility, and identity contract
phase=compiler
train=compiler.compiler-core
product=compiler_core
kind=contract
semantic_role=delivery
class=compiler
predecessors=CPF1,PAR0,DEM0,CCA2
conditional_predecessors=
owner=compiler.compiler-core:data-oriented common compiler substrate with framework-native planning
conflict_domains=source_lineage,compiler_execution
resource_class=docs-light
review_profile=architecture-3
gate_profile=docs-domain
size=M
dispatchable=true
optional=false
release_gating=none
source_refs=source:compiler-proposal.md:L812
external_requirements=
activation_gate=ORC0
charter=charters/compiler-compiler-core/CMP0.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CMP0 — Compiler request, policy, compatibility, and identity contract

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Compiler request, policy, compatibility, and identity contract. The current owner is **framework compiler emitters and per-node target dispatch**. The final and sole owner is **data-oriented common compiler substrate with framework-native planning**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_compiler/src`, `crates/verter_semantic/src`.
- Named API/data boundaries: `CompileRequest`, `CompilerPolicy`, `DemandSet`, `RegionId`, `EmissionSegment`, `ArtifactQualifier`.
- Mutation boundary: authority/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **CPF1:** exact current receipt ID and digest for “Carrier frontend registration and Vue/Svelte cutover”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **PAR0:** exact current receipt ID and digest for “Parser decision, ownership, reuse, and lineage contract”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **DEM0:** exact current receipt ID and digest for “Selection, two-stage activation, and demand planning”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CCA2:** exact current receipt ID and digest for “Compiler artifact, assembly, style-stage, and host boundary”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Intent:** replace mixed options and ambiguous output identity with typed per-framework requests and stage-specific compiler identities.
- **Problem:** cross-framework options, one broad cache key, and an undefined compatibility policy cause over-invalidation, ignored fields, and inconsistent behavior.
- **Solution and architecture decisions:**
- canonical request envelope:

## Acceptance IDs and discriminating proof

- **CMP0-AC1 — sole-owner proof:** add `cmp0_rejects_displaced_authority`; planting any deleted route must make the targeted gate fail.
- **CMP0-AC2 — positive contract:** add `cmp0_publishes_exact_compilerequest`; assert exact identities, provenance, completeness, and deterministic ordering.
- **CMP0-AC3 — incremental equivalence:** add `cmp0_incremental_equals_fresh`; cancellation/stale/partial outcomes must be refused from warm publication.
- **CMP0-AC4 — bounded work:** capture equivalent-work counters for the named surfaces; no extra parse, resolve, plan, emit, copy, allocation, or retained candidate is hidden by wall time.
- Test homes: `crates/verter_compiler/tests`, `crates/verter_vue_conformance/tests`, `crates/verter_svelte_conformance/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **dynamic dispatch inside node loops**.
- Delete or structurally reject: **whole-tree materialization fallback**.
- Delete or structurally reject: **unqualified artifact assembly**.
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

1. `cargo nextest run -p verter_compiler -p verter_vue_conformance -p verter_svelte_conformance`
2. Run every final command in the bound `docs-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Re-run the planted RED mutation, restore, then GREEN, and bind both outputs to the candidate SHA/tree.

## Review and lower-severity findings

Apply `architecture-3`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates the gate and all three review verdicts. Final acceptance requires three independent PASS verdicts on the exact candidate tree; clean 3/3 means no P0/P1 and no unauthorized or undispositioned P2.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; three named independent reviewers with assigned lenses, model `gpt-5.6-sol`, effort `ultra`, and report destinations; and the required report-back schema (candidate SHA/tree, changed paths, migration/deletion counts, RED/GREEN commands and outputs, gate receipt digests, review report digests, residual findings, and abort/rescope decisions). These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:compiler-proposal.md:L812`

## Reconciled source-plan contract

**Intent:** replace mixed options and ambiguous output identity with typed per-framework requests and stage-specific compiler identities.

**Problem:** cross-framework options, one broad cache key, and an undefined compatibility policy cause over-invalidation, ignored fields, and inconsistent behavior.

**Solution and architecture decisions:**

- canonical request envelope:

  ```text
  CompileRequest
      exact source/content basis
      requested products and targets
      CompilePolicy
      DefaultCompilationContractId
      common execution controls
      typed framework request
  ```

- owner-local `VueCompileRequest`, `SvelteCompileRequest`, and future framework requests;
- normalize public `Default` to an exact versioned internal policy/contract;
- reserve `Optimized`, but return truthful unsupported status until `OPT0` succeeds;
- classify option impact and derive:

  ```text
  ParseKey
  SemanticKey
  CompileStructureKey
  TargetPlanKey
  EmitKey
  TerminalKey
  ```

- reserve the future two-stage optimized identity without implementing analysis:

  ```text
  OptimizationRequestBasis   known before execution
  OptimizationObservationSet discovered during analysis
  ArtifactBasis              request basis + observation digest + decisions
  ```

- candidate reuse validates the old observation set against the captured workspace basis; it does not require the future read set before lookup.

**Suggested predecessors:** successor `CPF1`, `PAR0`, `DEM0`.

**Normative source decomposition:**

1. **CMP0-A — Request and refusal vocabulary.** Typed framework options, products, targets, policy and `NeedInputs`/`Unsupported` outcomes.
2. **CMP0-B — Default contract registry.** Exact framework/target behavior matrices and intentional divergence records.
3. **CMP0-C — Option-impact classifier.** Parse/semantic/structure/target/emit/terminal option partitioning with negative tests.
4. **CMP0-D — Stage identity types.** Stable canonical serialization/hashing, no display strings or ambient “latest”.
5. **CMP0-E — Future optimized basis reservation.** Types only; no project traversal, proof engine, or cache implementation.
6. **CMP0-F — Migration adapters and deletion ledger.** Map old mixed options/output requests and assign deletion owners.

**Acceptance:** changing map encoding does not invalidate semantics; changing one framework option cannot affect another framework; every requested product has one canonical identity; `Default` behavior is exact; `Optimized` cannot execute; no ignored cross-framework fields remain in accepted requests.

**Forbidden:** universal options bag, `HashMap<String, Value>` configuration, family-wide framework version switches, ambient default policy identity, or implementing project optimization.

**Deletion/abort:** delete mixed runtime options when all consumers move; abort any option whose invalidation impact cannot be classified.

---

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-COMP-L812-1B54DAD7C820

- Kind: `context`
- Source: `compiler-proposal.md:812-812`
- Applicability: `CMP0`
- Exact text SHA-256: `1b54dad7c820b828f495464e5736c19c842c3de8ea256693fbd36a31f115e075`

~~~~markdown
## `CMP0.md` — Compiler request, policy, compatibility, and identity contract
~~~~

### SRC-COMP-L814-258C0CB64A62

- Kind: `context`
- Source: `compiler-proposal.md:814-814`
- Applicability: `CMP0`
- Exact text SHA-256: `258c0cb64a6210a4c3f8f5854a5a79cb2c88db43ccc467378b0afafdf7c9a31c`

~~~~markdown
**Intent:** replace mixed options and ambiguous output identity with typed per-framework requests and stage-specific compiler identities.
~~~~

### SRC-COMP-L816-0FFFEE8D38DB

- Kind: `context`
- Source: `compiler-proposal.md:816-816`
- Applicability: `CMP0`
- Exact text SHA-256: `0fffee8d38db8851e1e174e696da03159383ec61dc4c8a7efeee930df50874ab`

~~~~markdown
**Problem:** cross-framework options, one broad cache key, and an undefined compatibility policy cause over-invalidation, ignored fields, and inconsistent behavior.
~~~~

### SRC-COMP-L818-56832D9ECFE1

- Kind: `context`
- Source: `compiler-proposal.md:818-818`
- Applicability: `CMP0`
- Exact text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L820-FE1ED123D96C

- Kind: `context`
- Source: `compiler-proposal.md:820-820`
- Applicability: `CMP0`
- Exact text SHA-256: `fe1ed123d96c636af33a462c68a79dcc51791c63abb3057135c2eea9bec7c219`

~~~~markdown
- canonical request envelope:
~~~~

### SRC-COMP-L822-A1FEE4265AAD

- Kind: `requirement`
- Source: `compiler-proposal.md:822-830`
- Applicability: `CMP0`
- Exact text SHA-256: `a1fee4265aad175f743c95e4b0ad9c8caae39575d877e25eda15b4aa0fd587bb`

~~~~markdown
```text
  CompileRequest
      exact source/content basis
      requested products and targets
      CompilePolicy
      DefaultCompilationContractId
      common execution controls
      typed framework request
  ```
~~~~

### SRC-COMP-L832-96ED21364E94

- Kind: `context`
- Source: `compiler-proposal.md:832-832`
- Applicability: `CMP0`
- Exact text SHA-256: `96ed21364e9438bf9d1149d5f0556545f03ba1dd565ad81228202bc985b823c5`

~~~~markdown
- owner-local `VueCompileRequest`, `SvelteCompileRequest`, and future framework requests;
~~~~

### SRC-COMP-L833-2A403E3BEE51

- Kind: `requirement`
- Source: `compiler-proposal.md:833-833`
- Applicability: `CMP0`
- Exact text SHA-256: `2a403e3bee5157470a65daa2b057922084d194a906ba49a49a59fa29589161c2`

~~~~markdown
- normalize public `Default` to an exact versioned internal policy/contract;
~~~~

### SRC-COMP-L834-221B1D0B6098

- Kind: `context`
- Source: `compiler-proposal.md:834-834`
- Applicability: `CMP0`
- Exact text SHA-256: `221b1d0b60986c5f6be6841d3c84a0d10f22fb569ee1affab6771009159ac0bf`

~~~~markdown
- reserve `Optimized`, but return truthful unsupported status until `OPT0` succeeds;
~~~~

### SRC-COMP-L835-CDEBD450FFDD

- Kind: `context`
- Source: `compiler-proposal.md:835-835`
- Applicability: `CMP0`
- Exact text SHA-256: `cdebd450ffdd7c4ec0a573109277b4a33f3e6697f7875092e98c3645aa600732`

~~~~markdown
- classify option impact and derive:
~~~~

### SRC-COMP-L837-309DA772A80D

- Kind: `context`
- Source: `compiler-proposal.md:837-844`
- Applicability: `CMP0`
- Exact text SHA-256: `309da772a80df4aae2ddcad52b2e92d901c9ccd88739a633dd64c5f370c85a4e`

~~~~markdown
```text
  ParseKey
  SemanticKey
  CompileStructureKey
  TargetPlanKey
  EmitKey
  TerminalKey
  ```
~~~~

### SRC-COMP-L846-1E4A3D928CBB

- Kind: `context`
- Source: `compiler-proposal.md:846-846`
- Applicability: `CMP0`
- Exact text SHA-256: `1e4a3d928cbb1c295008503fb69972ae063a2a79db5ac28b945ec952ec0e707e`

~~~~markdown
- reserve the future two-stage optimized identity without implementing analysis:
~~~~

### SRC-COMP-L848-35A44B60DCD6

- Kind: `context`
- Source: `compiler-proposal.md:848-852`
- Applicability: `CMP0`
- Exact text SHA-256: `35a44b60dcd6f06270b2f41227156bd6ed1e43f9305d51a7c21910d5195460e3`

~~~~markdown
```text
  OptimizationRequestBasis   known before execution
  OptimizationObservationSet discovered during analysis
  ArtifactBasis              request basis + observation digest + decisions
  ```
~~~~

### SRC-COMP-L854-883EF62A7671

- Kind: `context`
- Source: `compiler-proposal.md:854-854`
- Applicability: `CMP0`
- Exact text SHA-256: `883ef62a7671fcb13add3bb99b9b767b832c78eb1fc9b60923ee0db0dc0bd8c1`

~~~~markdown
- candidate reuse validates the old observation set against the captured workspace basis; it does not require the future read set before lookup.
~~~~

### SRC-COMP-L856-3BA42AC2FB60

- Kind: `context`
- Source: `compiler-proposal.md:856-856`
- Applicability: `CMP0`
- Exact text SHA-256: `3ba42ac2fb601647bef8629223f310f0a63c55f213a4bcc7c46f5e7c12d9dda6`

~~~~markdown
**Suggested predecessors:** successor `CPF1`, `PAR0`, `DEM0`.
~~~~

### SRC-COMP-L858-D484DA845654

- Kind: `context`
- Source: `compiler-proposal.md:858-858`
- Applicability: `CMP0`
- Exact text SHA-256: `d484da845654c11ff55391c9fb769e6e24b252647a5f06264f41d3df2c7d79c8`

~~~~markdown
**Suggested subblocks:**
~~~~

### SRC-COMP-L860-A371598569B6

- Kind: `context`
- Source: `compiler-proposal.md:860-860`
- Applicability: `CMP0`
- Exact text SHA-256: `a371598569b661d3be4c5d19d85392e82236d3a0525c7f1a83ff95e84d8e9889`

~~~~markdown
1. **CMP0-A — Request and refusal vocabulary.** Typed framework options, products, targets, policy and `NeedInputs`/`Unsupported` outcomes.
~~~~

### SRC-COMP-L861-5BDA1668D60A

- Kind: `requirement`
- Source: `compiler-proposal.md:861-861`
- Applicability: `CMP0`
- Exact text SHA-256: `5bda1668d60a17dd55b1a48d27dcacf8df92397f6714386b704da8749d54ff32`

~~~~markdown
2. **CMP0-B — Default contract registry.** Exact framework/target behavior matrices and intentional divergence records.
~~~~

### SRC-COMP-L862-4D58F00EC665

- Kind: `context`
- Source: `compiler-proposal.md:862-862`
- Applicability: `CMP0`
- Exact text SHA-256: `4d58f00ec66596585fef9914a5c1bbe88275a080bad26dc5890c88909aa1358b`

~~~~markdown
3. **CMP0-C — Option-impact classifier.** Parse/semantic/structure/target/emit/terminal option partitioning with negative tests.
~~~~

### SRC-COMP-L863-3B49D851CF27

- Kind: `context`
- Source: `compiler-proposal.md:863-863`
- Applicability: `CMP0`
- Exact text SHA-256: `3b49d851cf273206fb7fdcbc97e53dd3979d1ffb3163c419f13a12dd84d22889`

~~~~markdown
4. **CMP0-D — Stage identity types.** Stable canonical serialization/hashing, no display strings or ambient “latest”.
~~~~

### SRC-COMP-L864-E2EFA7F1047B

- Kind: `context`
- Source: `compiler-proposal.md:864-864`
- Applicability: `CMP0`
- Exact text SHA-256: `e2efa7f1047bf7244b1870747bc45e9c695d4d5f29da98e7200ad6b94955829d`

~~~~markdown
5. **CMP0-E — Future optimized basis reservation.** Types only; no project traversal, proof engine, or cache implementation.
~~~~

### SRC-COMP-L865-07DA07851994

- Kind: `deletion`
- Source: `compiler-proposal.md:865-865`
- Applicability: `CMP0`
- Exact text SHA-256: `07da078519940e8daca96dc6ebd71819c7d7e45dca561c5993b27204acfa85b8`

~~~~markdown
6. **CMP0-F — Migration adapters and deletion ledger.** Map old mixed options/output requests and assign deletion owners.
~~~~

### SRC-COMP-L867-E0FA27F56F23

- Kind: `acceptance`
- Source: `compiler-proposal.md:867-867`
- Applicability: `CMP0`
- Exact text SHA-256: `e0fa27f56f23dc72e0229d6e17c5419ca66f868e29a64edfd09c72a06399e71d`

~~~~markdown
**Acceptance:** changing map encoding does not invalidate semantics; changing one framework option cannot affect another framework; every requested product has one canonical identity; `Default` behavior is exact; `Optimized` cannot execute; no ignored cross-framework fields remain in accepted requests.
~~~~

### SRC-COMP-L869-CE91169C7D4D

- Kind: `forbidden`
- Source: `compiler-proposal.md:869-869`
- Applicability: `CMP0`
- Exact text SHA-256: `ce91169c7d4d74c71f77194af644e944fbc1ea09c0afbff6f36c707f1a1b3a13`

~~~~markdown
**Forbidden:** universal options bag, `HashMap<String, Value>` configuration, family-wide framework version switches, ambient default policy identity, or implementing project optimization.
~~~~

### SRC-COMP-L871-CE4798C2B053

- Kind: `deletion`
- Source: `compiler-proposal.md:871-871`
- Applicability: `CMP0`
- Exact text SHA-256: `ce4798c2b05309cdb4119027fecf134c5cdfa2fff2aee30372eabd3da0d1f8ff`

~~~~markdown
**Deletion/abort:** delete mixed runtime options when all consumers move; abort any option whose invalidation impact cannot be classified.
~~~~

### SRC-COMP-L873-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:873-873`
- Applicability: `CMP0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~

### SRC-COMP-L1977-47FC91B41FB7

- Kind: `context`
- Source: `compiler-proposal.md:1977-1977`
- Applicability: `CMP0`
- Exact text SHA-256: `47fc91b41fb755cc31bb003f9bfdfe81fa61d8f311b009dd84080ba7c0379ee1`

~~~~markdown
# 12. Architecture laws
~~~~

### SRC-COMP-L1979-3FF10BE17459

- Kind: `context`
- Source: `compiler-proposal.md:1979-1979`
- Applicability: `CMP0`
- Exact text SHA-256: `3ff10be17459869bd7e5de402d815458b37e480d3535de01780a9fe3a34fdc1d`

~~~~markdown
## 12.1 LAW — correctness and authority
~~~~

### SRC-COMP-L1981-AC2C9F98F6C2

- Kind: `context`
- Source: `compiler-proposal.md:1981-1981`
- Applicability: `CMP0`
- Exact text SHA-256: `ac2c9f98f6c27dd803f9df9cb5ac557db71e76e6765249974ad1f70e6d3f0121`

~~~~markdown
1. Each framework semantic epoch has exactly one framework semantic authority.
~~~~

### SRC-COMP-L1982-DA0EE924C10A

- Kind: `context`
- Source: `compiler-proposal.md:1982-1982`
- Applicability: `CMP0`
- Exact text SHA-256: `da0ee924c10a0095cbf20ef7942309e275a7acb7cb580f16821f1aabee7c09a3`

~~~~markdown
2. `verter_analysis` and `type_info` provide shared machinery/facts, not universal framework semantics.
~~~~

### SRC-COMP-L1983-201D1206AF10

- Kind: `context`
- Source: `compiler-proposal.md:1983-1983`
- Applicability: `CMP0`
- Exact text SHA-256: `201d1206af105ed306eb57c51fe27344a7b70cdf421e34dd1f44699131aa7099`

~~~~markdown
3. The compiler consumes semantic facts and cannot recreate a competing analyzer.
~~~~

### SRC-COMP-L1984-D8D59BA9FD9F

- Kind: `context`
- Source: `compiler-proposal.md:1984-1984`
- Applicability: `CMP0`
- Exact text SHA-256: `d8d59ba9fd9f9c5016303ebc12c95fcbacb3e1f45b13bb74af7259fac9d9d9b9`

~~~~markdown
4. `Default` may use all safe component-local canonical facts and may correct prelocked upstream gaps.
~~~~

### SRC-COMP-L1985-49DED183B431

- Kind: `context`
- Source: `compiler-proposal.md:1985-1985`
- Applicability: `CMP0`
- Exact text SHA-256: `49ded183b431ee839cd0a35713082463be8ade5ff6b0893fd550dd70151b8d6a`

~~~~markdown
5. `Optimized` remains unsupported until `OPT0` is rescoped and successor blocks are ratified.
~~~~

### SRC-COMP-L1986-CA58C683149D

- Kind: `forbidden`
- Source: `compiler-proposal.md:1986-1986`
- Applicability: `CMP0`
- Exact text SHA-256: `ca58c683149d2c4b358567963dd46e8461c67a3776db6463c0b2a8752facd5f0`

~~~~markdown
6. Unknown or incomplete facts never enable a stronger optimization.
~~~~

### SRC-COMP-L1987-C8CCE94AC6D0

- Kind: `context`
- Source: `compiler-proposal.md:1987-1987`
- Applicability: `CMP0`
- Exact text SHA-256: `c8cce94ac6d0ead717c6cd3c376319fd3cae26570e16e99cc432d1df4c1255d2`

~~~~markdown
7. Parse, semantic and compile admission remain distinct.
~~~~

### SRC-COMP-L1988-0ADA8B811530

- Kind: `context`
- Source: `compiler-proposal.md:1988-1988`
- Applicability: `CMP0`
- Exact text SHA-256: `0ada8b81153041adcda358a3986d0c5567eece34ba8378f6acff529438efb3a5`

~~~~markdown
8. J owns CSS-family syntax and neutral style facts; framework authorities own framework style meaning.
~~~~

### SRC-COMP-L1989-6AE12543DEB1

- Kind: `context`
- Source: `compiler-proposal.md:1989-1989`
- Applicability: `CMP0`
- Exact text SHA-256: `6ae12543deb18549f9ee7d4b1297c8eff9ba113717364e118ea3c00b7af23b39`

~~~~markdown
9. No runtime/compiler CSS preprocessor execution in this program.
~~~~

### SRC-COMP-L1990-0AA83E06953D

- Kind: `context`
- Source: `compiler-proposal.md:1990-1990`
- Applicability: `CMP0`
- Exact text SHA-256: `0aa83e06953daba81149d5cb1c4c60ef32323f38eea68d8eeffdaf70500d481f`

~~~~markdown
10. No semantic decision from raw-source searching after authoritative parsing.
~~~~

### SRC-COMP-L1991-8A3D178FA161

- Kind: `requirement`
- Source: `compiler-proposal.md:1991-1991`
- Applicability: `CMP0`
- Exact text SHA-256: `8a3d178fa161144235a81495666c9a3307aa10866e59018dd1547cb33062c2de`

~~~~markdown
11. No redundant authoritative parse of an exact region/grammar product.
~~~~

### SRC-COMP-L1992-127C8560D02C

- Kind: `context`
- Source: `compiler-proposal.md:1992-1992`
- Applicability: `CMP0`
- Exact text SHA-256: `127c8560d02cef2d3a55680f0b4c507a1b30ccff8479c1e2dbc05ed6ccbdfbd8`

~~~~markdown
12. Lossless/recovery tooling data does not enter admitted compiler nodes.
~~~~

### SRC-COMP-L1993-29A5E03ACA81

- Kind: `context`
- Source: `compiler-proposal.md:1993-1993`
- Applicability: `CMP0`
- Exact text SHA-256: `29a5e03aca81b39791b4272b4aa9562344cc1e2fb272abb7c608ddcaa488d91c`

~~~~markdown
13. Framework compiler structures remain framework-native.
~~~~

### SRC-COMP-L1994-BB852A64DB7A

- Kind: `context`
- Source: `compiler-proposal.md:1994-1994`
- Applicability: `CMP0`
- Exact text SHA-256: `bb852a64db7acf09b2dcb31c172eea8e78234abf552a50c65870b98ca7075f0b`

~~~~markdown
14. No universal reactivity AST.
~~~~

### SRC-COMP-L1995-15CBCB86C45B

- Kind: `context`
- Source: `compiler-proposal.md:1995-1995`
- Applicability: `CMP0`
- Exact text SHA-256: `15cbcb86c45bc1bf960ef9c6028097b9b6cba4fbfb3a2f7e684833022df4faf2`

~~~~markdown
15. No per-node dynamic target dispatch in accepted hot paths.
~~~~

### SRC-COMP-L1996-94E2BAA60F98

- Kind: `requirement`
- Source: `compiler-proposal.md:1996-1996`
- Applicability: `CMP0`
- Exact text SHA-256: `94e2baa60f98e4f049862fd1dea04e02f0c30d7581b4a3ef428df7a689705b3c`

~~~~markdown
16. Server-only targets perform zero client-effect planning.
~~~~

### SRC-COMP-L1997-4B1873AFE9C1

- Kind: `context`
- Source: `compiler-proposal.md:1997-1997`
- Applicability: `CMP0`
- Exact text SHA-256: `4b1873afe9c1c0cd70054400afccf71c1763aa63f619fcca28e4429f68d5834d`

~~~~markdown
17. Map-disabled requests perform zero attributable map construction.
~~~~

### SRC-COMP-L1998-7E11C1DB3F02

- Kind: `context`
- Source: `compiler-proposal.md:1998-1998`
- Applicability: `CMP0`
- Exact text SHA-256: `7e11c1db3f02e3f5c8322970ef55161c502e9a2c098527658acbb671d4f373ef`

~~~~markdown
18. Framework compilers own semantic module assembly.
~~~~

### SRC-COMP-L1999-76D3C7A0FD14

- Kind: `context`
- Source: `compiler-proposal.md:1999-1999`
- Applicability: `CMP0`
- Exact text SHA-256: `76d3c7a0fd14317c3e4161e9dad70404c738d97bd4b5ee9077874817978915ee`

~~~~markdown
19. Framework-host integration owns host policy, not framework semantic recovery.
~~~~

### SRC-COMP-L2000-D57A94200ABE

- Kind: `context`
- Source: `compiler-proposal.md:2000-2000`
- Applicability: `CMP0`
- Exact text SHA-256: `d57a94200abe2c3faa1abb29161973d66da708ba3650853ae6d0af0de01d92e2`

~~~~markdown
20. Custom blocks remain opaque unless a separately admitted integration owns them.
~~~~

### SRC-COMP-L2001-C73C91A66450

- Kind: `context`
- Source: `compiler-proposal.md:2001-2001`
- Applicability: `CMP0`
- Exact text SHA-256: `c73c91a6645081ad10a1ebfb5d76ca15ad09519cb1befd247d8429bd92def2ba`

~~~~markdown
21. OXC remains internal; no external AST ABI is implied.
~~~~

### SRC-COMP-L2002-5A780D0BBFFA

- Kind: `context`
- Source: `compiler-proposal.md:2002-2002`
- Applicability: `CMP0`
- Exact text SHA-256: `5a780d0bbffa418e44a3ea2fafecf6eb02437f80f4f659c1a582ec18af529347`

~~~~markdown
22. Dense node IDs are arena indices; authored offsets and incremental lineage are separate.
~~~~

### SRC-COMP-L2003-56F24DC9AA7D

- Kind: `context`
- Source: `compiler-proposal.md:2003-2003`
- Applicability: `CMP0`
- Exact text SHA-256: `56f24dc9aa7d3d15acde4937f07e1e66d9275a03a44d7d6291d7de69f9958970`

~~~~markdown
23. Direct, prepared and managed results are semantically equivalent for the same request/basis.
~~~~

### SRC-COMP-L2004-EE8BD668419A

- Kind: `context`
- Source: `compiler-proposal.md:2004-2004`
- Applicability: `CMP0`
- Exact text SHA-256: `ee8bd668419abc0c7dae205b5f42f911dcafc415b90fde09db5cac2bd535dd53`

~~~~markdown
24. Incremental output is exactly equivalent to fresh output.
~~~~

### SRC-COMP-L2005-ED8F0EEE42CB

- Kind: `context`
- Source: `compiler-proposal.md:2005-2005`
- Applicability: `CMP0`
- Exact text SHA-256: `ed8f0eee42cbed5fc16206640c0712d0795d0586ac8d05fa2467a4b962db13ea`

~~~~markdown
25. Performance claims require equivalent requested work, behavior, maps, options, cache/thread state and RSS.
~~~~

### SRC-COMP-L2026-3B7EA9600F5B

- Kind: `context`
- Source: `compiler-proposal.md:2026-2026`
- Applicability: `CMP0`
- Exact text SHA-256: `3b7ea9600f5be384bc3c751b1204ecc9209a892bd6a51f55fa196715ad31848c`

~~~~markdown
## 12.3 METRIC — observed before interpretation
~~~~

### SRC-COMP-L2028-91EC373FC986

- Kind: `context`
- Source: `compiler-proposal.md:2028-2028`
- Applicability: `CMP0`
- Exact text SHA-256: `91ec373fc98663614b9b13f5e60d2767caa7a8cbf0c8fe06c3af5e422f501bbc`

~~~~markdown
- region visits;
~~~~

### SRC-COMP-L2029-67CEFF488662

- Kind: `context`
- Source: `compiler-proposal.md:2029-2029`
- Applicability: `CMP0`
- Exact text SHA-256: `67ceff488662c986f2e5566a496098644c9a9ee5aab61b5c18bc6f90c5ed88cc`

~~~~markdown
- dependency/effect graph density;
~~~~

### SRC-COMP-L2030-8508955C52EE

- Kind: `context`
- Source: `compiler-proposal.md:2030-2030`
- Applicability: `CMP0`
- Exact text SHA-256: `8508955c52ee8c61ad4bb82fbef6e2b76df32ba6ad326511a6da132abc21d992`

~~~~markdown
- selector plan/index hit rates;
~~~~

### SRC-COMP-L2031-B4E724A22F8C

- Kind: `context`
- Source: `compiler-proposal.md:2031-2031`
- Applicability: `CMP0`
- Exact text SHA-256: `b4e724a22f8c4fcca12a5e4a6db7dbca7fc3e4bcf30be8515d74f98e69a5b561`

~~~~markdown
- direct versus indexed matcher crossover;
~~~~

### SRC-COMP-L2032-B4030777F047

- Kind: `context`
- Source: `compiler-proposal.md:2032-2032`
- Applicability: `CMP0`
- Exact text SHA-256: `b4030777f0471e7ffe7dec3a6e996bef9357add55a78918f885b12005a36ad53`

~~~~markdown
- target overlay density;
~~~~

### SRC-COMP-L2033-50BF33283D2C

- Kind: `context`
- Source: `compiler-proposal.md:2033-2033`
- Applicability: `CMP0`
- Exact text SHA-256: `50bf33283d2cd8d440a201b9f5189b69e215d0c7bf4b5d793b8875a61cf68b21`

~~~~markdown
- multi-target prerequisite reuse;
~~~~

### SRC-COMP-L2034-98D56E4B4F47

- Kind: `context`
- Source: `compiler-proposal.md:2034-2034`
- Applicability: `CMP0`
- Exact text SHA-256: `98d56e4b4f472d82e3619aa39a4e873e9ae3ebce2a0c49575cb079e73b9b010e`

~~~~markdown
- intern-table density;
~~~~

### SRC-COMP-L2035-36D4D3574805

- Kind: `context`
- Source: `compiler-proposal.md:2035-2035`
- Applicability: `CMP0`
- Exact text SHA-256: `36d4d3574805e69a7898fd74314e02e3667bdbde16981f51659303825f41d8f2`

~~~~markdown
- scratch versus retained memory;
~~~~

### SRC-COMP-L2036-A9BAAFC30F02

- Kind: `context`
- Source: `compiler-proposal.md:2036-2036`
- Applicability: `CMP0`
- Exact text SHA-256: `a9baafc30f029b39085a6ec4ea07e9f69e03081a5964bb9ebbedaffcdf399bd3`

~~~~markdown
- output segment distribution;
~~~~

### SRC-COMP-L2037-740C777E7C3C

- Kind: `context`
- Source: `compiler-proposal.md:2037-2037`
- Applicability: `CMP0`
- Exact text SHA-256: `740c777e7c3cc090481cf419ced35e91fb2e6b8625c6f87fa17249d0bc1a30bc`

~~~~markdown
- cache candidate validation rates.
~~~~

### SRC-COMP-L2039-94803AB7BA81

- Kind: `requirement`
- Source: `compiler-proposal.md:2039-2039`
- Applicability: `CMP0`
- Exact text SHA-256: `94803ab7ba81c1e3aaf89e2e1329754540cb80850b5f46b3d22254727705c168`

~~~~markdown
Metrics become gates only through a preimplementation lock.
~~~~

### SRC-COMP-L2041-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:2041-2041`
- Applicability: `CMP0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~

### SRC-COMP-L2043-4E76237BFB51

- Kind: `context`
- Source: `compiler-proposal.md:2043-2043`
- Applicability: `CMP0`
- Exact text SHA-256: `4e76237bfb51e163d272d0a8ec2ca5c0f9972069c298405b742049916c8b8cf6`

~~~~markdown
# 13. Recommended execution order
~~~~

### SRC-COMP-L2045-0F30729AE291

- Kind: `context`
- Source: `compiler-proposal.md:2045-2047`
- Applicability: `CMP0`
- Exact text SHA-256: `0f30729ae2910fb79c7c377fccb493144beac04b2af61f8748d83f767dcf595f`

~~~~markdown
```text
Revision 11:
    CCA0 → CCA1 → CCA2 → continue C2+
~~~~

### SRC-COMP-L2049-64DFA37E83E7

- Kind: `context`
- Source: `compiler-proposal.md:2049-2052`
- Applicability: `CMP0`
- Exact text SHA-256: `64dfa37e83e7241697230ec80fc245b5cc7f5afc2dd6f849bf09aa7304444a52`

~~~~markdown
Successor compiler foundation:
    CPER0
      ↘
       CMP0 → CPER1 → CMP1 → CMP2 → CMP3 → CMP4 → CPER2 → CMP5
~~~~

### SRC-COMP-L2054-4DA7BB84C2FC

- Kind: `context`
- Source: `compiler-proposal.md:2054-2056`
- Applicability: `CMP0`
- Exact text SHA-256: `4da7bb84c2fc2a6c4d4db163dfb34bcaaedca896af01b596144003d82f203637`

~~~~markdown
First product train:
    Vue Default + Vue style integration
    Vue selector-query feature may proceed independently after its prerequisites
~~~~

### SRC-COMP-L2058-6DDDA4C7083F

- Kind: `context`
- Source: `compiler-proposal.md:2058-2059`
- Applicability: `CMP0`
- Exact text SHA-256: `6ddda4c7083f7f93b7188eeb8dc93f0753817c8682d7077e0bd87063453f190d`

~~~~markdown
Second product train:
    Svelte Default + canonical style matcher
~~~~

### SRC-COMP-L2061-037315FAC52A

- Kind: `context`
- Source: `compiler-proposal.md:2061-2062`
- Applicability: `CMP0`
- Exact text SHA-256: `037315fac52a9495cd9a72418a28f3035486b5c4f7e4233f8ded92daf114e401`

~~~~markdown
After both products:
    CMP6 + CPER3
~~~~

### SRC-COMP-L2064-A147D06DB4A3

- Kind: `requirement`
- Source: `compiler-proposal.md:2064-2067`
- Applicability: `CMP0`
- Exact text SHA-256: `a147d06db4a30dafbe9dbb7db5e7e5005fca8dbafb63e55b1b1ff9cd40cd45c4`

~~~~markdown
Explicitly deferred:
    OPT0 RESCOPE_REQUIRED
    VCB0 RESCOPE_REQUIRED
```
~~~~

### SRC-COMP-L2069-B4499DA0C8D8

- Kind: `deletion`
- Source: `compiler-proposal.md:2069-2069`
- Applicability: `CMP0`
- Exact text SHA-256: `b4499da0c8d8faea2213bb4041bbf1e6704e5b45e2eca6abdb3be10bc0156444`

~~~~markdown
Do not run Vue and Svelte implementation as one mega-stack. Within each framework, use short PR-sized subblocks and keep cutover/deletion atomic at the named framework cutover block.
~~~~

### SRC-COMP-L2071-F52D711103D5

- Kind: `context`
- Source: `compiler-proposal.md:2071-2071`
- Applicability: `CMP0`
- Exact text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`

~~~~markdown
---
~~~~
