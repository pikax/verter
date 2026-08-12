# Independent compiler-conformance and golden challenge

## Verdict

BLOCKING_FINDINGS

- `docs/arch/refactor/rev11/evidence/framework-conformance/option-inventories.md:10-12`; `docs/arch/refactor/rev11/evidence/framework-conformance/svelte-options.tsv:1-27`; `docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:18` — violates the complete, exactly-once semantics-affecting option inventory required by `docs/arch/refactor/rev11/amendments/AMD-005-framework-compiler-conformance-rescope.md:138-142`. The inventory audit universe is limited to `CompileOptions`, `ModuleCompileOptions`, and `OptimizeOptions`, so it omits the official `svelte/compiler.parse` options `filename`, `modern`, and `loose`. In the exact pinned source, `packages/svelte/types/index.d.ts:878-894` declares those options and `packages/svelte/src/compiler/index.js:86-122` proves that `loose` changes parse recovery and `modern` changes the returned AST. This is not a merely unclaimed extra artifact: `SVELTE-PARSE-LOCAL` is proposed `supported` and explicitly claims parser modern/legacy cases plus errors/recovery. At minimum `loose` is a semantics-affecting option in a claimed product and has no classification or fail-closed treatment. Therefore the claimed Svelte `25/25` coverage is incomplete.
- `docs/arch/refactor/rev11/evidence/framework-conformance/option-inventories.md:10-25`; `docs/arch/refactor/rev11/evidence/framework-conformance/svelte-options.tsv:9`; `docs/arch/refactor/rev11/evidence/framework-conformance/capability-matrix.tsv:26` — violates the same complete, exactly-once option-classification rule. The inventory includes only the compiler-level `customElement` boolean/callback selector, while the exact pinned compiler defines source-authored `<svelte:options>` values that override compile options in `packages/svelte/src/compiler/types/template.d.ts:58-60,77-106`: `customElement.tag`, `customElement.shadow`, per-prop `attribute`, `reflect`, and `type`, plus `customElement.extend`. These values change registration, shadow-root behavior, prop/attribute routing, reflection/type conversion, and the generated custom-element class. The capability row itself claims “customElement boolean/options” and singles out `extend` for fail-closed handling, but none of these official suboptions has one of the seven required classifications. Calling configuration-plugin fields out of scope does not classify these distinct, official in-component compiler options.

This report is bound to candidate commit
`ce1d0e4688af1b5bd548b6b68286632cc0f7ede8`, tree
`1ff1f83d8e994b6f1169b0b209c9f557c23f4728`. The branch was
`work/framework-conformance-rescope`; `HEAD` and `HEAD^{tree}` matched those values at
the start and immediately before this report write. The two findings above block PASS.

## Independent identity resolution

I resolved the upstream Git objects from clean local object stores whose `origin` URLs
are the official GitHub repositories, checked their worktree status, and ran
`git fsck --full --strict`. I did not infer source identity from npm versions or accept
the architect report's strings as proof.

| domain | independently resolved tag chain | commit tree | result |
|---|---|---|---|
| Vue | lightweight `refs/tags/v3.6.0-rc.3` directly names `3adb225775c9b28223a56e07f7a2f874b6fbb138` | `36da8dc8841a35d3e1163e4b9bb5752f95ca527a` | matches the package |
| Svelte | annotated tag object `a49603bbb50f948fd0c2bf5c55582a8f89b4d91c` names `44a7813730579b94004e182e5a67aab27aa9d2a6` | `63390158bfe8f997c474e35215a4fa627194c229` | matches the package |

The official [Vue RC.3 release](https://github.com/vuejs/core/releases/tag/v3.6.0-rc.3)
and [Svelte 5.56.8 release](https://github.com/sveltejs/svelte/releases/tag/svelte%405.56.8)
also identify those release commits.

Independently recomputed package identities:

| artifact | SHA-256 |
|---|---|
| AMD-005 | `1d4a354b72c9ec2458e47ac570b0bc6cc893576e7d364c6033b5f98f85302d81` |
| amended DAG | `335e0863ba1f21473a24befc0093dc01bad4f065ff03e6716c113448be054489` |
| Vue `package.json` | `df17ad96a1dc2b18783b2040e35bcd1e83239e8b7d4bd3255b5bdf2dbbf3b6e4` |
| Vue `package-lock.json` | `0dd2290c0b7d01f4727953b838610727b18bcb999b634eeb8ab726508a34b951` |
| Vue `closure.tsv` | `d5caba234d8545b8b7bc7cc4cca8b8cf63f8ed594140d7cae80f3c7ae64606b2` |
| Vue case manifest | `30123a6d88e1e7382afdcc752b5438c3486dd462e59ce831742ad0a3a3dd95bd` |
| Svelte `package.json` | `ac1b539596a6ea3e1151b00720edaf73c42a4aab4aac5caafb1079e858a6578a` |
| Svelte `package-lock.json` | `0c27c9fc7bed24be3fd7a546b55b6ee5858b244a57613390a213fdb454b92ce2` |
| Svelte `closure.tsv` | `3dc4209c2911700de92858e350ddda2e6f5f333874a2eb330125ee808910dbce` |
| Svelte case manifest | `c251be5b8b1de3e58c526700c426e2502e8bd1eb1dd622e22119b667adee7a8e` |

These values agree with the `identity_bindings` section of
`.agent-run/architect-report.yaml`. That report's package-preparation candidate
commit/tree fields are historical and are not used as this review's candidate binding.

## Package locks and exact closures

I parsed the committed root manifests, locks, and closure TSVs. Separately, I compared
every lock entry with cached official npm registry version metadata and checked each
resolved regular, optional, and peer edge against its declared semver range. I then
reconstructed both TSVs independently from Node's lock resolution rules and compared
the result byte-for-byte with the committed files.

| domain | package nodes | regular edges | optional edges | peer edges | mismatches |
|---|---:|---:|---:|---:|---:|
| Vue | 25 | 49 | 0 | 2 | 0 |
| Svelte | 20 | 25 | 0 | 2 | 0 |
| total | 45 | 74 | 0 | 4 | 0 |

For all 45 nodes, exact name, version, registry tarball URL, SRI integrity, regular
dependencies, optional dependencies, peers, and optional-peer metadata agree with the
registry record. All 78 edges resolve to a satisfying locked version or to the explicit
omitted optional peers (`typescript` for Vue and `@typescript-eslint/types` for
Svelte). Both reconstructed closure TSVs are byte-identical to the committed files.
The Vue root names all twelve required exact RC.3 compiler/runtime packages, and the
Svelte root names exact `svelte@5.56.8`; neither root uses a floating selector.

## Option inventory audit

I parsed the exact pinned source interfaces rather than comparing only TSV counts.
Within the package's declared extraction universe, the mechanical results are correct:

- Vue: 118 phase-specific rows, 89 unique semantic names, and all 80 unique base names;
  no missing/extra source-derived row in the declared Vue interface set. The eight
  `CompilerDeprecationTypes` keys plus `MODE`, SFC async-style fields, asset URL fields,
  and CSS Modules fields are present.
- Svelte: 26 rows and all 25 base names from `CompileOptions`,
  `ModuleCompileOptions`, and `OptimizeOptions`, with `experimental.async` and
  `compatibility.componentApi` expanded.
- Every committed row uses exactly one value from the seven-class closed set and has a
  nonempty canonical treatment/refusal.

That mechanical 25/25 result is not a complete semantic inventory. The two blocking
findings identify official option surfaces omitted from the extraction universe. In
particular, the `SVELTE-PARSE-LOCAL` and `SVELTE-CUSTOM-ELEMENT` capability claims make
the omissions observable rather than harmless internal fields.

No additional blocking classification error was found in the rows that do exist. The
higher-risk existing dispositions are explicit: Vue compat behavior fails closed,
compiler/transform injection is test-only, preprocessing/PostCSS is external, project
filesystem/global types are host-resolved, the nonexistent Vue SSR×Vapor backend fails
closed, Svelte accessors/immutable/HMR/API-4 fail closed, experimental async is a
separate cell, and the unclaimed official AST product is not applicable.

## Capability, maturity, and default rows

The matrix contains 34 unique proposed cells and separates Vue VDOM, Vapor, and SSR;
Svelte runes, legacy, server, module, custom element, experimental async, hydration,
and TypeScript-visible products; plus incompatible-version and not-applicable cells.
No Vue RC cell is labelled Stable. Current reachability/default observations, target
disposition, target maturity, owner, compatibility domain, and acceptance ID are
populated on every row.

The matrix is still explicitly proposed, and its `VERIFY`/pre-train observations are
not acceptance evidence. Apart from the two Svelte option-surface gaps above, I found
no silent success cell or invented SSR×Vapor Cartesian compiler mode.

## Official-case enumeration

I reran `generate-official-case-manifests.mjs` against the exact clean upstream trees
using a pinned Babel TypeScript parser. Both regenerated outputs were byte-identical to
the candidate manifests:

| domain | rows | distribution |
|---|---:|---|
| Vue | 2,003 | compiler-core 570; compiler-dom 137; compiler-sfc 509; compiler-ssr 134; compiler-vapor 653 |
| Svelte | 3,457 | 3,452 sample directories plus 5 suite sentinels across all 22 top-level official test-suite directories |

Vue records 1,998 single declarations and 5 parameterized declarations; all 2,003 are
blocked seeds. Svelte records 3,313 blocked seeds and 144 `not_applicable` rows for the
explicitly excluded migrate/preprocess/print or non-compiler suites. Representative
locators for compiler-core slot transforms, DOM `v-text`, compiler-sfc cases, Vapor
template refs, Svelte compiler errors, modern parser, legacy/runtime-runes, validator,
hydration, SSR, CSS, and sourcemaps resolve to the exact blob/tree objects stored in the
manifest.

These counts are declaration/sample scaffolding, not runner-expanded cases. Dynamic
Vue `.each` expansion and Svelte per-sample profile expansion remain BF2 work, as the
package states. I found no place that legitimately treats the row counts as proof of
candidate conformance.

## Oracle and exclusion boundaries

The named boundaries are complete:

- `official-core-oracles.md:7-15` binds exact source and package domains and rejects
  source/tree/package/integrity/closure drift before expectations or candidate runs.
- `language-tools-exclusion.md:5-18` forbids both `vuejs/language-tools` and
  `sveltejs/language-tools` as oracle, corpus, expected output, golden, baseline,
  acceptance source, or production dependency across runtime, semantic, diagnostics,
  public API, TSC/TSX, declaration, mapping, and route products.
- `third-party-exclusion.md:5-18` forbids Vize, rsvelte, PrimeVue,
  `pikax/vue-benchmarks`, `pikax/svelte-benchmarks`, and all other third-party apps,
  libraries, compilers, benchmarks, and fixture repositories as oracle/corpus/baseline.
  It also forbids semantic adaptation and using them to select normalizer rules.

No forbidden source is authorized as a fallback, repair provider, or acceptance
shortcut. TypeScript-visible products use the owned TypeScript domain, TypeScript
compiler/API observations, ratified Verter contracts, and independently authored local
fixtures.

## Golden provenance and normalizer

`conformance-goldens.md:20-30` requires expected results to come only from the exact
locked official compiler install, records source/lock/generator/options/environment/raw
and normalized digests, makes candidate output read-only, and rejects network access,
drift, output patching, or candidate-sourced expectations. It does not permit a Verter
candidate to self-update its expected result.

`conformance-normalizer.md:9-24` permits only:

- whitespace/line layout outside literals or comments with semantic force;
- parser-proven harmless redundant parentheses;
- quote spelling with the same decoded literal value; and
- scope-aware alpha-normalization of private generated names while preserving binding,
  shadowing, references, and authored/public names.

It explicitly forbids normalizing away import/export sources, helper families,
declarations and semantic order/side effects, DOM topology, block/effect topology,
events, props versus attributes, component invocation, slots, hydration markers, SSR
topology, diagnostics, mappings, literal values, source-authored names, or public
names. Lines 26-36 require applied-mutation discrimination and keep parse, link,
runtime, diagnostic, and mapping oracles outside the normalizer.

## Sixteen-point per-case obligation audit

The package allocates all sixteen original obligations conjunctively:

| obligation | binding text |
|---:|---|
| 1. correct requested products | `fragment-assembly.md:23-28`; `conformance-goldens.md:44-48` |
| 2. atomic artifact publication | `fragment-assembly.md:23-30` |
| 3. fragment validity | `fragment-assembly.md:10-14` |
| 4. assembled JavaScript parse | `fragment-assembly.md:13-15` |
| 5. real official-package link | `fragment-assembly.md:14-15`; `official-core-oracles.md:24-26` |
| 6. normalized structural equivalence | `conformance-goldens.md:44-48`; normalizer contract |
| 7. helper/import/call topology | `conformance-goldens.md:44-48`; normalizer forbidden set |
| 8. official-runtime execution | `official-core-oracles.md:17-26`; `conformance-goldens.md:44-48` |
| 9. server/SSR behavior | `ssr-hydration.md:5-13` |
| 10. hydration behavior | `ssr-hydration.md:15-35` |
| 11. diagnostics | `conformance-goldens.md:44-48`; `typescript-product-conformance.md:17-22` |
| 12. mappings | fragment source-space rules; `typescript-product-conformance.md:17-22` |
| 13. TypeScript observations | `typescript-product-conformance.md:5-30` |
| 14. route equivalence | `typescript-product-conformance.md:20-22`; `FC-ROUTES-001` |
| 15. zero unrequested work | `fragment-assembly.md:23-28`; `FC-ZERO-WORK-001` |
| 16. locked performance gates | `conformance-goldens.md:42-49`; BF1 performance exit |

Server checks include parse/link, official helper sources, escaping, attributes/props,
component/slot and async/marker topology, deterministic output, diagnostics, and maps.
Hydration includes official/official control, Verter/Verter, official-server/Verter-
client, and the inverse when officially meaningful, with marker preservation, no DOM
replacement, events/effects, updates, async/boundary behavior, diagnostics, and final
DOM. TypeScript cases bind exact version/options/libs/virtual inputs and observable
diagnostics, assignability, exposed types, component/JSX surface, declarations,
mappings, and route equivalence.

The two blocking option omissions mean these otherwise complete obligations cannot yet
be applied exhaustively to the claimed Svelte parse/custom-element profiles.

## Pre-candidate performance lock and seed-evidence boundary

`performance-impact.md:9-22` names harness, Vue VDOM/Vapor/SSR, Svelte
runes/legacy/server, route-overhead, and project-staging cells before a BF2/BV1/BS1
candidate exists. Lines 24-31 require correctness, non-vacuity,
zero-unrequested-work, absolute and relative limits, memory, counters, and independent
fixtures conjunctively. `charters/BF1.md:27-38` requires exact fixtures, digests,
correctness oracles, repetitions, thresholds, counters, memory ceilings, and lease
policy to be frozen before BF2 starts; `performance-impact.md:3-7` prohibits choosing
or changing thresholds after successor observation. The DAG keeps BF2 locked behind
BF1. This is a genuine pre-candidate lock process, not a post-candidate waiver.

The official-case files remain seeds only. `README.md:3-5,23-25`,
`validation.md:56-77`, AMD-005 lines 211-215, and BF2's exits all require runner
enumeration, an allowed final disposition, product/profile evidence, complete goldens,
and the applicable sixteen-point proofs. Supported cells cannot retain `blocked` or a
semantic known-divergence allowlist. I found no conflation of seed enumeration with
accepted compiler conformance.

## NON_BLOCKING_DISCOVERIES

- `docs/arch/refactor/rev11/evidence/framework-conformance/validation.md:42-46` says
  `validate-package.mjs` checks exact resolved dependency-edge manifests. The validator
  checks package counts/direct pins and digest-pins the closure files, but it does not
  semantically reconstruct the closure or compare every registry dependency field.
  My independent registry/lock/edge audit found zero actual mismatch, so this is a
  validation-description/defense-in-depth issue rather than a data blocker.
- The exact candidate commit contains previously authored challenge reports, while
  `validate-package.mjs:242-249` intentionally rejects the presence of those files.
  Consequently the package-preparation validation command is phase-specific and does
  not pass on the post-review candidate commit itself. This does not alter the
  independently verified compiler-conformance data above, but later governance should
  avoid presenting that pre-review command as a fresh exact-candidate validation.
