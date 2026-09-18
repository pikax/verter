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
