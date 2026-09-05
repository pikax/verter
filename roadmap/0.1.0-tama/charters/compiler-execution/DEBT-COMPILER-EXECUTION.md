<!-- unified-charter-v2
id=DEBT-COMPILER-EXECUTION
name=Close the compiler_execution open-debt ledger
phase=compiler
train=compiler_execution
product=compiler_execution
kind=repair
semantic_role=convergence
class=compiler
predecessors=CCA1O2E,CCA1O4
owner=compiler_execution:owner of every open debt row recorded against the compiler-execution area
conflict_domains=compiler_execution,host_service_graph,public_protocol
resource_class=ts-heavy
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
size=L
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-execution/DEBT-COMPILER-EXECUTION.md
max_production_loc=800
max_production_files=16
max_related_packages=6
rescope_loc=1500
rescope_files=24
rescope_unrelated_packages=3
-->

# DEBT-COMPILER-EXECUTION — Close the compiler_execution open-debt ledger

## Independently acceptable outcome and rollback boundary

Close every open debt row recorded against area `compiler_execution`. Acceptance is exactly **each row closed**: for every row id listed below, either the named work is delivered on the named surface, or the row is retracted as stale/already-delivered with the evidence that proves it. No new public surface is created. Reverting this block restores the pre-closure debt state only.

## Debt ledger and closure semantics

- A row closes by doing its work, or — where the row records staleness — by correcting/retracting the stale record itself with evidence. A row may also close by citing the ruling decision or later DAG node that owns the remedy (contract F7); it never closes by silence.
- The three P0 governance rows close only under contract F7: each DEFER/debt row standing in the named decision file must cite the decision id that deferred it, or the work is done.
- No closure may reintroduce a legacy compile-profile call. Where a row's population is owned by a pending sibling or terminal node (CCA1O2B/D/E, CCA1O4D), this block migrates or removes the affected callers itself, or cites that node's ruling; it never restores `compileProfile` request shapes.

### Governance and decision records (contract F7)

- `debt_0mtn8umdp005drokg` [P0]: the debt row in `roadmap/0.1.0-tama/decisions/2026-09-03-complete-only-transport-probe-classification-deviation.md` stands without a ruling — cite the deferring decision id or do the work.
- `debt_0mtn8tvr1004s6jf1` [P0]: same contract-F7 defect in the same 2026-09-03 decision file — rule it or do it.
- `debt_0mtn49w9c000x1sdg` [P0]: the debt row in `roadmap/0.1.0-tama/decisions/2026-09-04-cca1o2j-scope-disposition.md` stands without a ruling — cite the deferring decision id or do the work.

### Authority-ledger hygiene

- `debt_0mtndkvk4001i62u3` [P3]: the open row "Vue typed-route byte proof now exists" in `authority/state/implemented.toml` is stale — retract it, citing the real-WASM typed-route test and the legacy-vs-typed byte-identical probe.
- `debt_0mtndkvk3001gtrob` [P3]: the budget-wording row (3 playground files touched vs "2 production/test files" ceiling sentence) — reconcile the record's wording with the delivered file set or restate the ceiling.

### Playground typed-route defects

- `debt_0mtncuwda0023djpd` [P1]: `packages/playground/vite.config.ts` aliases `@verter/wasm` (a runtime dependency) onto the types-only leaf `packages/wasm/src/compile-request-types.ts`, and `vitest.config.ts`/`tsconfig.json` repeat it — point a non-colliding specifier (or a relative import) at the leaf types file so the runtime alias is never shadowed.
- `debt_0mtncuwd900206iep` [P1]: the SSR branch of `compileVueRenderAssembly` (`packages/playground/src/core/compiler.ts`) still try/catches `wasmHost.compileRequest` and publishes `'// SSR compilation failed'` plus an empty `runtimeServer` row — use the shared refusal path (requestCompile/recordCompileRefusal/withMissingRuntimeGuard) so a refusal yields a missing-product diagnostic, never fake or empty JS.
- `debt_0mtncuwdl0026hmcl` [P2]: the outer catch-all around `compileFile` maps every throw through `recordCompileRefusal` — keep that helper for compile-request refusals only and let unexpected throws surface.
- `debt_0mtncuwdm00292784` [P2]: `packages/playground/tsconfig.json` path-resolves `@verter/native/host-types` — replace the temporary resolver with a WASM types-only export so playground does not know native.
- `debt_0mtncuwdm002cw7fd` [P2]: `isHostBinding` rejects older WASM builds the version switcher still offers, so `switchWasmVersion` can present a build that then reports HOST_UNAVAILABLE_ERROR — make the version UI not offer builds lacking `compileRequest` as equivalent compilers (fail-closed dispatch stays).
- `debt_0mtncuwdn002frw5c` [P3]: `WasmModule.VerterHost` in `packages/playground/src/core/wasmLoader.ts` still describes the legacy virtual-file surface — update the typing to the compile-request surface.
- `debt_0mtncuwdn002ixadv` [P3]: the rune-module probe in `packages/playground/src/core/compiler.spec.ts` still calls `getVirtualFile` with a legacy compileProfile — migrate it to the typed route (or delete it with the legacy surface, citing the owning node).
- `debt_0mtncuwdn002llgsv` [P3]: `generateRealTsxOutput` accepts empty IDE code (only `null` fails) — use a length/presence check consistent with the sourcemap tests consuming the helper.

### Playground and typed-route completeness proofs

- `debt_0mtncuwdo002oxwc3` [P2]: prove Vue `compileFile` output/map/diagnostic bytes equal to the legacy playground path, or close under a recorded ruling.
- `debt_0mtn9xa0h007bow8y` [P2]: prove Vue `compileFile` output/map bytes on the typed route, or close under a recorded ruling.
- `debt_0mtn5y0ru003mfx48` [P2]: legacy WASM methods claimed covered by an unlinked suite — link the suite or correct the claim.
- `debt_0mtn5y0rt003kwhy5` [P2]: typed-route IDE/success results were compared against profile-cache host APIs — re-prove against the typed route or correct the recorded claim.

### ssr-baseline scratch and probe scripts

- `debt_0mtnktp1a00cqeb3y` [P3]: `_test-vbind-attrs2.mjs` dereferences the main node unguarded (`result.code` after a double `.find`) — use the sibling scripts' optional-chaining shape so absence prints as absence.
- `debt_0mtngxcf50063nv4s` [P3]: nine sibling probe/compare scripts (`_test-mergeProps.mjs`, `_test-mergeProps2.mjs`, `_test-slot-props.mjs`, `_test-style-merge.mjs`, `_test-vmodel.mjs`, `_test-vmodel2.mjs`, `ssr-baseline/compare.mjs`, `scripts/compare-per-file.mjs`, `vue-behavior-compare/run.mjs`) still build legacy compileProfile requests — migrate them to typed requests or cite the ruling that defers each population to its owning node.
- `debt_0mtngbato00bpdqst` [P3]: the thirteen underscore-prefixed scratch scripts under `scripts/ssr-baseline/` still pass `compileProfile` — migrate or remove them so the population does not break when the legacy profile goes away.
- `debt_0mtngbatn00bk8dok` [P3]: the tracked one-off debuggers (`_test-*.mjs`, `_check-*.mjs`, `_casing-*.mjs`) still call `host.getVirtualFile` with a legacy profile — same closure as above.

### Benchmark fence and axis debts

- `debt_0mtnbdxu0005ly026` [P2]: `FenceReport.cacheMode` still publishes `"verter-stateless-attested-per-sample"` after per-sample attestation was deleted — rename to the typed-route contract or drop the attestation claim.
- `debt_0mtncew220079tlfv` [P2]: the Svelte fence dropped per-sample cache attestation with no observable rail left — restore an observable equal-work rail for the typed route or record the construction-level argument as the ruling.
- `debt_0mtncew1m007666mp` [P2]: the Axis-A content-equality hash compiles a different lane (host-backed `ideCompanion` supplied-request) than the timed audit (`compile_registered_vue_artifact`) — hash the measured lane or document the divergence as a ruling.
- `debt_0mtnbdxu0005oncjx` [P2]: missing-IDE handling models empty products, but native `compileRequest` throws on `ProductNotProduced`/refusal and the content pass does not catch — catch and record `missingCarrier`; make `FakeHost` throw like native.
- `debt_0mtnbdxu1005rzgg2` [P3]: `vueIdeCompanionRequest` sets `identity.filename` to a raw backslash OS path — omit filename or pass the host canonical id so map sources match the legacy profile.
- `debt_0mtncew22007c0485` [P3]: `verter-mt-worker.ts` is renamed from the charter-named `.mjs`, has zero spawn sites, and its comment still claims apple-to-apple use — fix the comment, typecheck the worker against the generated request type, or remove the orphan with a ruling.
- `debt_0mtnbdxu1005ujjue` [P3]: the MT worker's untyped Vue `runtimeClient` request duplicates `verter.ts`'s builder and swallows closed-decode refusals — share one typed builder or typecheck the copy and count refusals.

### Batch route, FFI, and typed-product completeness

- `debt_0mtmii2x000bffg6a` [P2]: the batch route's `ideCompanion` product — the one product whose per-entry source pairing can be mis-mapped — is untested; test it end-to-end.
- `debt_0mtmfn50m00332npw` [P2]: published `HostDiagnostic` docs omit `arguments` and WASM serde emission is unproven — document and prove emission.
- `debt_0mtmfn50l002zs4oj` [P2]: UNRULED debt rows on batch-route publicApi/declarations coverage — rule or close each.
- `debt_0mtm7s5ol008n9lmu` [P2]: `runtimeServer`/`analysis`/`publicApi`/`declarations` products are untested (partly unrepresentable) end-to-end through the typed route — test the representable ones, rule the rest.
- `debt_0mtm7s5ok008ip560` [P2]: the addon harness does not execute analysis or publicApi/declarations arms — execute them or rule the omission.
- `debt_0mtm7i9hj007hznlh` [P2]: the freshness re-pin repeats the open pin defects rather than closing them — close them here.
- `debt_0mtn0px2v0035d0y3` [P3]: the batch entry wrapper silently ignores unknown own keys while options and the request graph refuse them — align the wrapper on fail-closed.
- `debt_0mtmuwipw005byw5x` [P3]: same-canonical concurrent compiles are newly reachable on one non-repeated test — make the test repeated/deterministic or rule the coverage gap.
- `debt_0mtmuwipv0059jgyr` [P3]: the order guard fails the whole batch on any id mismatch and its idempotency premise is argued only in prose — encode the premise as a test or rule it.
- `debt_0mtmuwipu0056ovym` [P3]: batch entry wrapper fields are read prototype-walking while options and the request graph are own-only — make the read own-only.
- `debt_0mtmmkx9d004msanr` [P3]: per-call budgets bind Rust retention, not the V8 handle scope the traversal fills — bound the traversal or rule the exposure.
- `debt_0mtmmkx9c004kkvtb` [P3]: `VALUE_REFUSED_*` option lists are hand-maintained mirrors with no derivation or completeness guard — derive them or add a completeness guard.
- `debt_0mtmfn50k002vkxpg` [P3]: u64/i64→f64 precision loss now also reaches the shared FFI/WASM diagnostic path — close or re-rule the widened row.
- `debt_0mtm7i9hh00791xbi` [P3]: `carrierIdByCanonicalId` is not populated for dependency-only upserts inside `resolveUpsertDependencies` (`packages/unplugin`) — populate it or rule the gap.

## Exact predecessor contract

- **CCA1O2E:** implemented ledger row for "Native transport-probe host-request migration"; the native transport probe runs on the typed request, so probe- and script-side rows close on the typed route without reopening the legacy profile route.
- **CCA1O4:** implemented ledger row for "Native unplugin host-request convergence"; unplugin's native host calls are typed, so the unplugin-side typed-route rows (carrier population, batch pairing) close on the converged route.

## Acceptance and evidence

- Every row id above is closed: work delivered, or the stale/deferred record corrected with an explicit ruling citation. The implementing commit message states that the ledger is exhausted.
- Byte-proof rows close with real-WASM or built-host probes comparing script/template/style/IDE/map/diagnostic bytes legacy-vs-typed, or with a recorded ruling — never with an unexecuted claim.
- Bug-fix rows (P1/P2 defects) land with the discriminating test named by the row.

## Deletions, budgets, and aborts

- Scratch scripts are migrated to typed requests or removed only where their owning row calls for it; no production NAPI/WASM type is deleted here (that is CCA1O4D's surface).
- Ceiling: 800 production LOC, 16 production/test files, 6 related packages; rescope above 1500 LOC, 24 files, or if any unrelated package or another consumer enters.
- Abort on reintroducing a legacy compile-profile call, on closing a P0/P1 row by silence, or on deleting a public type this block does not own.

## Verification and review

Run `node scripts/gate.mjs` plus the touched packages' focused suites (playground typecheck/specs, benchmark fence and axis tests, unplugin specs, wasm suites, `node --check` on touched scripts). Apply `public-3`. Add only DEBT-COMPILER-EXECUTION's ledger row.
