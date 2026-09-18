# Web-product expansion v1: ownership, disposition and claim constitution

Status: RATIFIED by WDX0 (charter `charters/expansion-web-product-convergence/WDX0.md`), consuming the accepted DX0 cross-surface feature preservation and exposure constitution (`contracts/product-experience.md`). Machine products: `tests/web-product/WDX0/products/`. Train plan: `plans/expansion-web-product-convergence.md`.

This contract is the shared constitution every web-product recommendation head consumes (each head's charter names this file as its normative companion). It binds ownership routing, evidence classes and public claims for the added portfolio: frameworks, extensions, profiles and the product-capability trains. It changes contract bytes only; implementation and deletion belong to the named successor nodes. It reopens no producing owner: source identity, semantic authority, TypeScript projection, mapping, lifetime, authored edits and the capability-catalog truth (`catalogs/product-surface-catalog.toml`, owner L0) stay with their existing owners, and every shipped valid feature remains RequiredCurrent under `contracts/sfc-typescript-projection.md`.

## 1. Ownership and disposition model

Every attached recommendation is ratified into exactly one of three dispositions, recorded in the machine product `web-product-disposition-matrix.v1.json`:

- **producing-train** — the recommendation becomes a delivered product capability through one named train with its own head (constitution), successor implementation nodes and a terminal that proves the capability end-to-end.
- **extension** — the recommendation becomes an opt-in extension surface through one named `extensions.*` train; extension capabilities are separately admitted, separately stable and never silently folded into core product claims.
- **exclusion** — the recommendation is explicitly not pursued, with a recorded reason. An unrecorded or reason-less exclusion is invalid; silence is not a disposition.

Rules:

1. **One final owner.** Each recommendation has exactly one final owner (its train); no recommendation may be ratified into two producing trains, and no train may own two recommendations. A proposal item with two final owners fails coverage review (WDX0-AC2).
2. **Head and terminal are distinct.** The head constitution ratifies scope; only the terminal's completed artifacts may support a product claim. A recommendation backed only by a proof node (architecture proof, installed parser, syntax highlighting) must fail coverage review (WDX0-AC2); proof predecessors (`*P` nodes, for example `RCTP`, `ALPP`) are inputs, never evidence of support.
3. **The matrix is the routing truth.** Adding, splitting or excluding a recommendation is a matrix amendment through this contract's owner, not an informal list. The qualification fan-in (WDX2's predecessors) is exactly the pinned terminal set plus WDX1.
4. **No second authorities.** Producing trains consume existing parsing, types, resolution, maps, query scheduling, public result envelopes and edit transactions; the shared kernel remains sole owner. No new kernel, global web graph, status store or scheduler.

## 2. Ratified portfolio

Disposition population at ratification (machine product pins all 41 rows): 35 recommendations ratified as producing trains, 6 as extensions, 0 exclusions. No recommendation is excluded at this time; the exclusion disposition exists so a future rejection is recorded, not silent.

- **Frameworks (13, producing trains):** `framework.react` (RCT0), `framework.angular` (ANG0), `framework.astro` (AST0), `framework.solid` (SLD0), `framework.preact` (PRE0), `framework.lit` (LIT0), `framework.qwik` (QWK0), `framework.marko` (MRK0), `framework.ember-glimmer` (GLM0), `framework.stencil` (STN0), `framework.alpine` (ALP0), `framework.htmx` (HTX0), `framework.mdx` (MDX0).
- **Product capabilities (18, producing trains):** accessibility (AX0), agent product (AGT0), component workbench (CWB0), config/env (CENV0), CSS intelligence (CSS0), embedded dialects (DIAL0), execution services (XEC0), extension SDK (XSDK0), migrations (MIG0), runtime environments (ENV0), runtime inspector (RTI0), testing product (TST0), utility CSS (TW0), web compat (WBC0), web debugger (DBG0), web performance (WPF0), web security (SEC0), workspace intelligence (WSI0).
- **Extensions (6):** document languages (DATA0), GraphQL (GQL0), i18n (I18N0), OpenAPI (OAPI0), PWA (PWA0), document metadata/SEO (SEO0).
- **Profiles (1, producing train):** Next.js versioned project profile (NXT0).
- **Cross-train consumer amendments (2):** DX1 (exposure descriptor contract, `expansion.product-observability`) and DOC5 (web-product guide templates and executable journeys, `expansion.documentation`) consume this contract inside their own trains; they add no portfolio item.
- **In-train:** WDX1 (cross-domain scenario fixtures and evidence routing) is this train's implementation node and the only non-terminal qualification edge.

## 3. Evidence classes and claim law

Machine product `evidence-class-policy.v1.json` pins three classes; they are never interchangeable:

- **static proof** — schema validation, structural invariants, compile/import boundaries, ownership checks. May claim a ratified contract, disposition or invariant; may never claim executed behavior, framework/version support, or performance.
- **runtime observation** — real execution on a named host/profile with the ProductReceiptBasis bound (source revisions, project/configuration, engine and host identity, completeness state, per `contracts/product-experience.md` §4). Required for any support, behavior or performance claim.
- **estimate** — labelled projections only. Never promotable to completion, conformance, source-revision, speedup or supported-version claims.

Claim laws binding every producer and every public surface:

1. A framework/extension/profile support claim requires runtime observation from that item's own terminal. This ratification is static proof: at ratification every added-portfolio item has claim basis `none` and status `planned`.
2. Qualifying an upstream feature as Verter support requires the owned authored integration and public operation evidence; installing or pinning upstream is static proof at best.
3. Estimates stay labelled; guessed zeros, sleeps as readiness and fabricated conformance or speedups are forbidden.
4. Negative, partial, conditional and unsupported results are truthful states, not routes for hiding unfinished required scope.

## 4. Historical completions preserved

The implementation ledger (`authority/state/implemented.toml`) remains the sole completion truth; this constitution adds no second status store. Every implemented-ledger completion keeps its standing — nothing is reopened, downgraded or deleted by this ratification. Named anchors: ORC0 (trusted ledger cutover), STP0 (projection constitution), DX0 (product-experience constitution), all `implemented` and all static-proof-class constitutions whose runtime obligations are bound to their successors.

The advertised capability catalog today is exclusively the historical vue/svelte portfolio (24 surfaces in `catalogs/product-surface-catalog.toml`, families `vue.*` and `svelte.*`). The added portfolio starts with zero advertised catalog surfaces; a new family appears there only when its producing train's terminal lands the real capability and the catalog's owning amendment ships with it. Historical completions and their evidence classes are recorded per class in `evidence-class-policy.v1.json` — static proof for constitutions, runtime observation where a terminal executed, estimates never presented as either.

## 5. Required capability matrix (obligations on every producing head)

Each producing train fills the same capability dimensions before its terminal may claim support — the dimensions its own head charter enumerates: parse, semantic facts, type/checker projection, edits, diagnostics, formatting, build interoperability and product exposure. Shared rules:

- **Tooling, not runtime.** Verter implements compile-time and static tooling (adapters, facts, projections, diagnostics, formatting). Framework runtime and compiler output stays upstream: obtained through explicit official upstream adapters with exact versions and official upstream acceptance sources (VID0 exact-release law); no new native compiler or runtime is promised by this contract, and Verter-owned compiler transforms remain with the compiler trains.
- **Exact-version lock.** Each framework/profile head locks one exact target version with official sources in its own contract; `latest`-style or unversioned support claims are invalid.
- **Kernel sole owner.** No second JSX/TS/CSS parser, resolver or type owner inside a producing train; no regex recovery for semantic identity; no whole-context helper or hidden cross-project state.
- **Exposure obeys DX0.** Every operation a train exposes registers through DX1's descriptor contract with executable route, replay case and completeness states; native-only and browser execution classes obey `HostExecutionClass`; promotion stays blocked until executable exposure exists.

## 6. Evidence routing and qualification

- **WDX1** owns cross-domain scenario fixtures and evidence routing over the disposition matrix: every recommendation's discriminating scenarios route to a real case with a pinned basis.
- **WDX2** qualifies cross-product authority and lifecycle: its predecessor fan-in is exactly the 38 portfolio terminals plus WDX1; qualification checks one final owner, evidence-class correctness and lifecycle truth (fresh/incremental, edit/revert, cancellation, stale/partial rejection, deterministic ordering) on real artifacts.
- **WDX3** is the acceptance terminal: it checks real completed artifacts — capability rows with runtime-observation receipts, DOC1-tested public examples/reference, migration notes — never templates or screenshots.
- **DX1/DOC5** bind this contract inside their own trains (descriptor registration; guide templates and executable journeys under the evidence-class policy).
- **Producer obligations (specified here, delivered by producers).** Each head updates the relevant VIM/DX capability and exact host/profile evidence when its capability lands; public examples/reference are DOC1-tested through DOC5/DOC1; uncertainty and migration notes ship with each breaking change.

## 7. Scope, migration and deletion population

WDX0 is contract-only: 0 production LOC, 0 production files, no runtime spawned. The boundary is genuinely additive — the deletion population is empty and no route is displaced or retired by this node; producing trains own their own cutover inventories before mutation. A zero-production ceiling is not permission: crossing it requires a scope amendment, not the rescope figure as allowance.

## 8. Forbidden designs

No new kernel, global web graph, status store or scheduler; no change to the existing L4 release gate; no second semantic authority, unqualified cache, regex recovery for semantic identity, whole-context helper or hidden cross-project state; no fabricated conformance, completion, source-revision, speedup or supported-version claim; no qualifying an upstream feature as Verter support without the owned authored integration and public operation evidence; no framework runtime implementation or hidden vendored compiler fork; no promotion from architecture proof to implementation without product evidence.

## 9. Ownership

Final owner for this outcome: `expansion.web-product-convergence` — cross-product scope, evidence routing and selected release claims; not semantic execution. Conflict domains: `capability_catalog`, `validation_observability` (both registered in `catalogs/conflict-domains.toml`; this node registers no new domain). Gates: `docs-domain`; review: `architecture-3`.

## 10. Receiving amendments

Machine product `web-product-ownership-map.json` pins the full population: the 41 recommendation heads (38 constitution heads production-incapable at ratification; DX1, DOC5 and WDX1 production-capable implementation nodes) plus the in-train WDX2/WDX3 obligations. This contract creates no reverse edges into predecessor trains; combined-graph validation runs after every amendment. Drift between the live repository (authority DAG, ledger rows, capability-catalog families, preserved contracts) and the machine products is a failing case, not a stale pass.

---

# Web product expansion contract v1 — live responsibility and debt inventory

Established by ARH0 (Live responsibility and debt inventory), train `expansion.architecture-health`. This is the normative companion of `charters/expansion-architecture-health/ARH0.md`. ARH1 (responsibility, visibility and dependency contracts), ARH2 (behavioral characterization and complexity measurements), ARH12 (architecture-drift prevention) and DOC1/DOC2 amend this file; they do not fork it.

## Inventory identity

- Implementation candidate: `f69c7eee7aba2b1a260cc4d85a282bbedbbde349` (branch `tama_dag/0.1.0-tama/ARH0`, 2026-09-18). Every number below was measured on that tree; the machine-readable copies with per-row derivation live in `tests/architecture-health/ARH0/products/`.
- Platform: Windows x64, Node 26, cargo metadata of the workspace manifest. Classification method is recorded per section and in the products; counts are line counts of source files, not tokens of prose.
- Rounding rule: a number in this document always equals the corresponding field in the products; if they disagree, the products win and this document must be amended.

## 1. Production / test / generated separation

Classification rules (applied to Rust and TypeScript alike, recorded in `products/codebase-inventory.json`):

- **Production** — files under `src/` that are not test-named and not generated-marked. For Rust, `#[cfg(test)]` modules inside production files remain production files; the guard-level classification is filename-based (`*_tests.rs`, `tests.rs`, `tests/`), which is the same convention the `god_module_size_budget` guard uses (`crates/verter_session/tests/cases/architecture_guards.rs:356`).
- **Test** — `tests/` directories, test-named files (see above), TS `test|tests|__tests__|e2e|fixtures` directories and `*.test.*`/`*.spec.*`/`*.bench.*` files.
- **Generated** — checked-in artifacts whose header names a generator script. Rust: `crates/verter_compiler/src/svelte/bind_contract_data.rs` and `svelte/runtime/entity_table.rs` (the guard allowlist population), `crates/verter_semantic/src/analysis/html_intrinsics_data.rs`. TypeScript: `packages/proto/src/gen/**` (buf/protoc-gen-es output of `crates/verter_protocol/proto`, freshness-gated by `scripts/check-proto-generated.mjs`), `packages/types/audit.generated.ts`, `packages/language-shared/src/generated/`, `packages/native/src/generated/`. `@ai-generated` headers are authorship metadata, not generation; generated test corpus/manifest files (`scripts/gen-corpus-audit-tests.mjs`, `scripts/gen-typeinfo-ignore-manifest.mjs` outputs) stay in the test population.

Measured baseline (candidate `f69c7eee7`):

| Population | Production LOC | Test LOC | Generated LOC |
| --- | --- | --- | --- |
| Rust workspace (48 `crates/*` + `xtask`; the `verter_napi` fixture-addon member is counted inside `verter_napi`'s test tree) | 1,108,242 | 1,062,523 | 12,926 |
| TypeScript `packages/*` (26; root-level build/config scripts, 16,518 LOC, counted separately) | 121,543 | 178,326 | 20,397 |

Notable ratios the successor nodes must not lose: `verter_session` production 425,049 vs test-classified 543,807 LOC — the largest crate is already majority test-named code by filename; `packages/vue-vscode` is 13,417 src vs 54,866 test/e2e LOC. Any future "test economy" claim has to re-derive these populations with the same rules, not quote them.

The five Phase 11 god-module targets are all directory modules today (`meta_resolve/`, `resolver_core/component_meta_query_engine/`, `host_manage/`, `compiler/src/ide/script/`, `lsp/src/server/`), with documented thin shells (`meta_resolve.rs` 142 LOC, `host_manage.rs` 1,352 LOC); `server.rs` and `ide/script.rs` no longer exist as files. The `god_module_size_budget` guard keeps the 4,000-LOC production budget over whichever form exists.

## 2. Responsibility map — surviving owners

Every production module routes to exactly one surviving owner. "Surviving" means the named train/node keeps authority after this train; ARH0 reassigns nothing. Evidence kinds: `documented` (architecture skill or crate-level contract states the responsibility), `charter-declared` (the owning train's charters name the path), `graph` (fan-in/fan-out from `cargo metadata`), `coupling` (shared-commit history since 2026-06-01, 781 commits).

The full table is `products/responsibility-map.json`; the load-bearing rows:

| Module | Surviving owner | Responsibility (one line) |
| --- | --- | --- |
| `verter_span`, `verter_language`, `verter_identity`, `verter_protocol`, `verter_ecma` | `expansion.kernel` | zero/low-dependency identity, language registry, transport DTO law (VID0/CAT0) |
| `verter_semantic` (+`analysis`) | `rev11.flow` | reusable semantics and static-analysis facts |
| `verter_compiler` | `compiler.compiler-core` (Vue/Svelte codegen split across `compiler-vue-compiler`, `compiler-svelte-compiler`) | lowering, template codegen, framework bridges |
| `verter_css_syntax` | `rev11.style` (dialect consumers: `compiler-vue-style*`, `compiler-svelte-style`) | lossless five-dialect style token/IR authority |
| `verter_session` (`resolver_core`, host stores, `project_semantic_dispatch`) | `rev11.flow` with `expansion.kernel` cache law | host/session boundary, resolver orchestration, semantic dispatch |
| `verter_scheduler` | `rev11.scheduler-runtime` | CPU/IO pool submission, batching, admission |
| `verter_tsgo_api`, `verter_type_runtime`, `verter_relay_shim` | `rev11.provider-runtime` (tsgo plane), `rev11.typescript-mapper` (dual-plane law TCM0R) | provider processes, tsserver/tsgo IPC, relay egress |
| `verter_tsc`, `verter_type_expr(+_oxc)`, `packages/type-ir` | `rev11.typescript-mapper` / `rev11.type-algebra` / `rev11.type-evaluation` | TS checker projection and type expression facts |
| `verter_lsp`, `packages/vue-vscode`, `packages/typescript-plugin`, `packages/language-shared`, `packages/lsp-test-client`, `packages/svelte-jsx`, `crates/verter-editor-client`, `extensions/{zed,lapce}`, `editors/{helix,nvim}` | `expansion.language-service` | language-server features, editor hosts, launch contracts |
| `verter_napi`, `verter_wasm`, `verter_ffi`, `packages/native`, `packages/wasm` | `compiler.compiler-bridge` | native/WASM bridge surface |
| `verter_mcp`, `verter_mcp_server`, `packages/verter-mcp`, `mcp/*.mcp.json` | `expansion.cli` (MCP tool surface) | analysis MCP server and manifests |
| `packages/verter-lsp`, `packages/verter-tsc`, `packages/binary-launcher` | `expansion.cli` | published binary launchers |
| `packages/unplugin` | `expansion.bundler-host` | bundler plugin host integration |
| `packages/nuxt` | `expansion.project-profiles` | Nuxt project profile |
| `packages/component-meta` | `expansion.kernel` (component-meta native/compat boundary per `/component-meta`) | component-meta TS surface |
| `packages/typeinfo`, `packages/types` | `rev11.public-typeinfo` (typeinfo), `expansion.language-service` (TS utility types) | public typeinfo surface, injection utility types |
| `packages/playground`, `packages/example`, `examples/` | `expansion.kernel` playground surface; `examples/reference` obligations specified in §7 | in-repo apps |
| conformance: `verter_vue_conformance`, `verter_svelte_conformance`, `packages/framework-conformance-harness`, `packages/vue-conformance-oracle`, `packages/svelte-runtime-tests` | `compiler-vue-compiler` / `compiler-svelte-compiler` | framework conformance oracles and harnesses |
| `verter_diagnostics`, `verter_actions` | `expansion.lint` | lint rules and code actions |
| `verter_bench`, `packages/benchmark` | `compiler.compiler-perf` | benchmark corpora and harnesses |
| `verter_dx_baseline`, `packages/dx-harness` | DX0 cross-surface preservation | DX baseline and feature-preservation harness |
| `verter_validation_probe` | `compiler.validation-observability` | validation probes |
| `verter_audit` | `rev11.flow` audit substrate (`audit-infrastructure`) | structured audit events |
| `scripts/`, `xtask/`, `tools/` | `governance.github-control-plane` (CI/gate), release tooling under `governance.release-control` | build, gate, release scripts |
| `tests/` (root), `test-corpora/` | per-feature test owners; aggregate rules in `/testing` | cross-cutting test evidence |

Modules with no single surviving owner are not silently absorbed; they are debt rows (§5) assigned to ARH1 for an explicit visibility decision.

## 3. Fan-in / fan-out and mutable state

Crate graph facts (`cargo metadata`, internal non-dev edges):

- Highest fan-in: `verter_span` (24), `verter_semantic`/`verter_type_expr`/`verter_language`/`verter_debug_assert` (13 each), `verter_audit` (10), `verter_session` and `verter_compiler` (9 each). These are the substrate: a change in them re-compiles nearly everything, so their owners hold the compatibility pins.
- Highest fan-out: `verter_session` (18), `verter_lsp` (16), `verter_napi` (13), `verter_compiler` (12), `verter_mcp` (7). These are composition roots; they must not grow new semantic authority, only route to owners.
- `verter-editor-client` is consumed by `extensions/zed`, `extensions/lapce` and pinned by the `editors/nvim`+`editors/helix` config contract tests — a surviving shared launch-contract leaf, not an orphan.

Mutable state census (owned, not re-enumerated here): the enforced inventories are the architecture guards — `every_db_field_in_project_type_store_appears_in_inventory` (ProjectTypeStore fields), `no_thread_local_oxc_caches` + `wave_3_entry_points_propagate_tls` (TLS/audit propagation), `no_off_store_host_caches*` (off-store cache law), `compile_batch_options_banned_thread_fields` (scheduler thread fields). The scheduler's cross-request state lives behind `verter_scheduler` admission; session stores behind `ProjectTypeStore` + resolver stores. ARH2 measures these populations; ARH0 adds none.

## 4. Change coupling (781 commits, since 2026-06-01)

Top shared-commit pairs: `verter_compiler <-> verter_session` (105), `verter_session <-> docs` (103), `verter_session <-> roadmap` (90), `verter_lsp <-> verter_session` (80), `verter_semantic <-> verter_session` (63), `verter_compiler <-> verter_lsp` (44). Interpretation rule for successors: coupling between a composition root and its owners is expected; coupling that crosses two semantic owners (e.g. `compiler <-> semantic`, 30) is the drift signal ARH12 must watch.

Most-touched production files in the window: `crates/verter_scheduler/src/scheduler.rs` (structure), `verter_session/src/host_resolve/virtual_file_pipeline.rs` (52), `verter_session/src/project_semantic_dispatch/flow_return.rs` (48), `verter_compiler/src/framework_common/vue_bridge.rs` (44), `verter_compiler/src/svelte/carrier.rs` (39).

## 5. Complexity debt and god-module evidence

33 production files exceed 4,000 LOC (full list with LOC and generated-flag in `products/responsibility-map.json` `largeFilePopulation`; one of them, `html_intrinsics_data.rs`, is a generated data source, not a complexity claim). Declaration discipline (ARH0-AC2, enforced by `tests/architecture-health/ARH0/verify.mjs`):

- A file is a **god-module candidate** only with responsibility evidence: at least two distinct named responsibilities plus coupling evidence (shared-commit count or fan-in). Size alone never qualifies.
- The five previously-split Phase 11 targets are **retired debt**, not god modules; reclassifying any of them (or their successors) requires the same fresh multi-responsibility evidence, not the pre-split history.
- Current top candidates by evidence (full rows in the product): `verter_scheduler/src/scheduler.rs` (20,743 LOC; admission + batching + pool routing + cancellation in one file — three responsibilities; crate fan-in 5 and 19 file touches in the §4 window), `verter_session/src/project_semantic_dispatch/flow_return.rs` (15,021; flow-return dispatch + tests-adjacent fixtures), `verter_session/src/semantic_query.rs` (10,634; query envelope assembly), `verter_session/src/flow_slice_content.rs` (9,427; crate fan-in 9, 34 touches). `verter_semantic/src/analysis/html_intrinsics_data.rs` (9,971) is a generated data source and is explicitly not a god-module candidate.
- Disposition: measurement and behavioral characterization are ARH2's independently acceptable work; ARH0 records the evidence and declares nothing split. No LOC target exists (forbidden design), and lower LOC alone is not success (ARH0-AC5).

## 6. Required capability matrix

Exact engine/host pins of the candidate, with production consumers (machine-readable: `products/capability-matrix.json`; every `source` path is existence-checked by the verifier):

| Capability | Engine / pin | Production consumers |
| --- | --- | --- |
| Vue SFC compilation & conformance | `vue`/`@vue/compiler-sfc`/`@vue/compiler-core` `3.6.0-rc.5` (workspace-collapsed) | `crates/verter_vue_conformance`, `packages/vue-conformance-oracle`, `packages/unplugin`, `packages/playground`, `examples/` |
| Svelte compilation & conformance | `svelte` `5.56.10` | `crates/verter_svelte_conformance`, `packages/svelte-runtime-tests`, `packages/unplugin` |
| TypeScript dual plane | tsserver `typescript ^6.0.3` (packages) and root `typescript 7.0.2` (tsgo/native preview toolchain) | `crates/verter_type_runtime` (tsserver IPC), `crates/verter_tsgo_api` + `crates/verter_relay_shim` (tsgo plane), `packages/typescript-plugin` |
| Rust parser substrate | oxc `0.126.0` (`Cargo.toml`) | `verter_parser`, `verter_type_expr_oxc`, scheduler-path parses |
| Transport schemas | buf + `@bufbuild/protoc-gen-es` from `crates/verter_protocol/proto` | `packages/proto/src/gen`, `packages/native` |
| Bundler host | `vite ^8.0.14` (dev/test), tailwind `4.3.0` fixture | `packages/unplugin` |
| Nuxt profile | `@nuxt/kit ^4.4.6` | `packages/nuxt` |
| Editor hosts | VS Code (`packages/vue-vscode` + `verter-lsp` binary), Zed + Lapce (`extensions/*`, wasm32), Helix + Neovim (`editors/*` config contracts) | `crates/verter-editor-client` launch contracts |
| Analysis MCP | `verter-mcp`/`verter-mcp-server` binaries, `mcp/verter.mcp.json`, `mcp/verter-http.mcp.json` | `packages/vue-vscode/src/mcpServer.ts` lifecycle |

Required-but-not-yet-implemented cells (web-product delta) stay owned by their trains — e.g. Solid and htmx targets (`d71b9c7a6` roadmap decision), Alpine/Angular/Astro and the other DOC1 successor terminals, lint/native-checker expansions, formatter. ARH0 records them as `required-planned` rows only where a repo authority row or merged decision names them; it invents no support claim (forbidden design: no fabricated supported-version claims).

## 7. Producer obligations specified by this constitution (ARH0-AC4)

ARH0 ships contract bytes only; the following are binding on the named producers, not deliverables of this node:

- **DOC1 — `examples/reference`:** build the executable example/reference-validation harness under `examples/reference` (currently absent; `examples/` today is the Vue examples app). Every capability row in §6 that is `implemented` must be demonstrable from `examples/reference` with the exact pinned versions above; `required-planned` rows must have a referenced example stub that fails honestly, not a green placeholder. Permissions, uncertainty and migration notes live with the examples DOC1 tests.
- **ARH1 — responsibility/visibility/dependency contracts:** consume `products/responsibility-map.json` as the baseline; convert each `unassigned` debt row into an explicit visibility decision; do not reopen owners that survive per §2 without a charter amendment.
- **ARH2 — measurements:** re-derive §1 populations with the recorded classification rules before making any complexity claim; bind budgets to the ratified performance methodology (`performance-gates.toml`) before measuring.
- **ARH12 — drift prevention:** extend the existing `architecture_guards.rs` pattern (not a new status system) for any rule that survives review; coupling thresholds come from §4 re-measured, not from this snapshot.
- **Public docs:** DOC2 updates contributor architecture documentation from §2–§5 when those nodes land.

## 8. Deletion and retirement register

Dispositions with concrete owners; ARH0 deletes nothing (empty production mutation set — this is a genuinely additive inventory boundary; there is no superseded ARH0-era API to remove):

| Candidate | Evidence | Disposition / owner |
| --- | --- | --- |
| `packages/core` | directory contains only `.gitignore`; no `package.json`, no sources | delete or populate with a named purpose; decision belongs to ARH1 visibility pass (tracked `unassigned-purpose`) |
| SIMP3–SIMP6, SIMP8–SIMP15 retirement blocks | `contracts/codebase-simplification.md`; SIMP1/SIMP2/SIMP7 already delivered per ledger | execute under their own `rev11.simplification` nodes; ARH0 does not pre-delete |
| Phase 11 thin shells (`meta_resolve.rs`, `host_manage.rs`) | documented intended post-split layout in `god_module_size_budget` | keep; retiring them is not debt, it would break the guard's dual-form walk |
| `STATUS-vue-collapse.md` (repo root) | committed chore working-note for the completed Vue `3.6.0-rc.5` collapse | fold into `docs/` history or remove via `governance.feedback-intake`; not architecture debt |
| `.scratch-arh0/` inventory scripts | uncommitted measurement scratch of this node | never committed; method recorded in products |

## 9. Acceptance dispositions (ARH0)

- **AC1:** outcome and consumers are the charter + this contract + the four products, validated by the sole owning interface `tests/architecture-health/ARH0/verify.mjs` (structural contract enforcement; a compile-time boundary does not exist for markdown contracts, and pretending one does would be a second authority).
- **AC2:** the verifier rejects a god-module row whose only evidence is size, and rejects re-classifying a previously split module on split history alone; dirty twins in `arh0.test.mjs` prove both rejections fail-closed. This extends the existing `god_module_size_budget` evidence rather than duplicating it.
- **AC3:** no state/query/map boundary is touched (zero production mutation). Fresh-vs-incremental, edit/revert, cancellation and ordering remain owned by their untouched owners (B4R0 lineage, LSO snapshot publication, scheduler admission); rationale recorded in `products/responsibility-map.json#ac3Rationale`.
- **AC4:** VIM/DX capability evidence is §6 with exact host/profile pins; `examples/reference` obligations are specified for DOC1 (§7); uncertainty (rc/preview pins, `required-planned` rows) is stated, not hidden.
- **AC5:** no new latency/work/allocation/RSS budget is created (none measured); obsolete-path retirement is out of scope for a zero-production node and bound to §8 owners. No LOC-reduction claim is made.

## Amendment rule

Amendments append a dated section and update the products together; numbers never drift from products (§Inventory identity). This contract owns no runtime, spawns no runtime, and gates nothing in CI by itself.
