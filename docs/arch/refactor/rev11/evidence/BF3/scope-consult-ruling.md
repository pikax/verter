## Ruling

BF3 should build **no new production guard, typed refusal, artifact-withholding path, retraction table, or runtime tracking mechanism** for incorrect-but-successful output.

The existing `RETROACTIVE-NO-FORWARD-ONLY` ruling is procedurally understandable but architecturally wrong. Ratification history can explain why an implementer may not silently ignore current text; it does not justify preserving an inferior mechanism. The standing rule applies equally to Vue, Svelte, and carrier-generic products: a supported request that produces wrong output needs a regression and a correction, not a production path that recognizes the bug and refuses the request.

Current BF3 nevertheless expressly mandates detection, refusal, withholding, and whole-cell retraction ([BF3.md:11](docs/arch/refactor/rev11/charters/BF3.md:11)). AMD-006 expressly retained that mechanism for Svelte and non-Vue-runtime cells ([AMD-006:79](docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:79)), and records the prior ruling at [AMD-006:149](docs/arch/refactor/rev11/amendments/AMD-006-vue-known-defect-correction.md:149). Therefore this correction requires formal amendment and re-ratification; an implementer must not merely deviate.

The narrow repository sources I checked record the maintainer direction in Vue-specific form ([deviation-memo.md:5](docs/arch/refactor/rev11/evidence/vue-known-defect-correction/deviation-memo.md:5)). I did not find the full project-wide wording reproduced verbatim in those sources, so this ruling treats the project-wide formulation supplied in the consult as binding. On that premise, there is no principled Svelte exception.

### Permissible production refusal

A typed refusal remains legitimate only when it represents a real, independently specified capability boundary determined from the typed request before compilation—for example, “the server backend is not implemented.” It must not be triggered by:

- a fixture identity;
- a known compiler defect;
- an oracle mismatch;
- a syntax pattern selected because it currently miscompiles;
- a version-specific known-divergence list.

The existing Svelte `ServerGenerate` refusal meets that distinction: an SSR request returns `Unsupported(ServerGenerate)` before emitter work ([client_compile.rs:110](crates/verter_compiler/src/svelte/runtime/client_compile.rs:110)). BF3 should test and record it but add nothing to it. Its eventual replacement is owned by the block that implements conforming Svelte server generation—currently BS1, whose charter owns client and server output at exactly `svelte@5.56.8` ([BS1.md:8](docs/arch/refactor/rev11/charters/BS1.md:8)). It is not a temporary BF3 guard and needs no BF3 removal ID.

### Correct BF3 scope

BF3 should be reshaped into a **pre-B2/B3 conformance-exhaustion and correction-dispatch block**, not a safety-retraction block.

It must:

1. Build and run a Svelte counterpart to the genuine shipped-path authoritative gate.
2. Exercise the exact six `svelte@5.56.8` client cells individually across applicable parse, real-package link, structural, diagnostics, mapping, and runtime axes.
3. Include mutation/control tests proving each claimed axis actually detects an independently planted defect.
4. Exhaust the remaining reachable-success product/route inventory.
5. Distinguish genuine compiler failures from harness, normalizer, source-content, or route-assembly artifacts before assigning ownership.
6. Add a precise failing regression for every genuine defect.
7. Route every genuine defect to an immediate correction owner; never translate it into a runtime guard.

The manifest contains 48 entries—36 Vue and 12 Svelte—with six Svelte client and six server entries; the Svelte entries begin in the manifest’s entries inventory at [manifest.json:57](packages/framework-conformance-harness/goldens/manifest.json:57). The current Rust seed loader explicitly filters to `vue/` ([bf2_seed_matrix.rs:170](crates/verter_session/src/compile/map_equality_tests/bf2_seed_matrix.rs:170)), while the full-axis gate claims all six axes over genuine production results ([bf2_full_axis_gate.rs:14](crates/verter_session/src/compile/map_equality_tests/bf2_full_axis_gate.rs:14)). Consequently, the six Svelte client results are unknown until that equivalent gate actually runs. BF3 must make no disposition based on presumed outcomes.

The remaining inventory does not disappear. It includes Svelte client modes, PublicApi/TSC/declaration for both frameworks, diagnostics/maps/CSS, and NAPI/WASM/host/bundler spellings ([bf3-safety-retraction-scope.md:10](docs/arch/refactor/rev11/evidence/framework-conformance/bf3-safety-retraction-scope.md:10)). Route aliases need route-identity and atomic-publication proof, but should not be misrepresented as independent semantic cases when they converge on the same typed request.

Ownership must follow root cause:

- Svelte adapter/emitter defects go to a new immediate pre-B2/B3 Svelte correction block—conceptually `BS0`—rather than waiting for post-B4 BS1.
- Carrier-generic planning, product, atomicity, or transport defects go to their common-layer owner, not to a Vue- or Svelte-specific block.
- Harness/oracle/normalizer defects are corrected in the conformance infrastructure.
- Vue runtime rows already assigned to and corrected by BV0 remain outside BF3’s runtime probe scope.

If a genuine defect requires broad repair, BF3 must rescope into the appropriate correction block. It must not fall back to retraction. B2/B3 must remain locked until every immediate correction block arising from BF3 is accepted. That provides safety by refusing to advance the program, rather than by contaminating the shipped compiler with temporary defect recognition.

### Restated exits

The amendment should replace BF3’s current exits as follows:

- **Inventory exhaustion remains mandatory and non-vacuous.** Every reachable-success cell in the retained inventory must have an actual result.
- **Every failure receives:** exact request/route/profile/products/domain evidence, independent discriminating regression, root-cause classification, named correction block, and correction acceptance/test ID.
- **Removal IDs are eliminated.** There are no BF3 guards to remove. A correction ID and permanent regression replace the guard/removal pair.
- **Actual correction is required before downstream dispatch.** BF3 may close its audit after complete disposition only if the DAG makes every resulting correction block a required predecessor of B2/B3; otherwise BF3 itself remains open until those corrections land.
- **`FC-ATOMIC-001` remains non-vacuous** for all successful results and all genuine contract-defined refusals. It means no partial publication on success or refusal ([AMD-005:208](docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:208)). “Every BF3-created refusal” is an empty set.
- **Cold-path guard tests become vacuous** because no guard exists. Replace them with route-parity tests, harness mutation controls, and correction-owner regressions proving unrelated supported cells retain behavior.
- If no genuine failures are found, the per-failure correction clauses are explicitly vacuous; inventory, oracle-execution, route, atomicity, and mutation-control exits are not.

### Required amendment

A new ratified amendment must explicitly supersede, not silently reinterpret:

- BF3’s title and objective;
- procedure steps 3–5;
- step 7’s guard-deletion requirement;
- the retained-retraction paragraph;
- the “no broad correction” abort logic;
- the required-exit clause demanding a guard and removal ID;
- AMD-006 §4’s retention of the original mechanism;
- AMD-006 §8.1’s `RETROACTIVE-NO-FORWARD-ONLY` ruling;
- the BF3 ledger note and any DAG edges needed for immediate correction blocks;
- the scope document’s `BF3-RET-*` production-record scheme.

Historical text may remain as superseded history, but no live normative document should continue authorizing the mechanism.

### The stale Svelte corpus pin

The `5.56.3` migration does **not** belong inside BF3 and should not wait for BS1. The root package still pins `svelte` to `5.56.3` ([package.json:92](package.json:92)), while the authoritative domain is `5.56.8` ([domain-pin.mjs:66](packages/framework-conformance-harness/src/domain-pin.mjs:66)). Existing Svelte conformance code also describes itself as grounded in `5.56.3` ([model.rs:24](crates/verter_svelte_conformance/src/model.rs:24)).

That is a separate conformance-infrastructure migration and should receive its own authorized block, preferably as a BF3 prerequisite. It should migrate the root pin and both stale corpora atomically, regenerate their expected artifacts against `5.56.8`, classify every changed expectation, and delete the old pin. Folding it into BF3 would mix oracle migration with candidate adjudication; leaving it to BS1 would permit BF3 to operate while the repository still carries two purported Svelte truths.

No designated-maintainer-only architectural decision is needed. Maintainer ratification is required to change the current normative program, but the correct architectural direction is determinable: **probe exhaustively, fix actual defects in their owning layer, add no BF3 production retraction mechanism, and make downstream work wait for the corrections.**
