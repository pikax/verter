<!-- unified-charter-v2
id=CLI3
name=Aggregate `check` and transactional `fix` commands
phase=expansion
train=expansion.cli
product=cli
kind=terminal
semantic_role=delivery
class=successor
predecessors=CLI5,CLIF0,CLIL0
conditional_predecessors=
owner=expansion.cli:one `verter` application service with thin command adapters
conflict_domains=diagnostic_action_service,cli_application
resource_class=ts-heavy
review_profile=architecture-3
gate_profile=ts-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=S
dispatchable=true
optional=false
release_gating=product
source_refs=source:successor-expansion.md:L1556
external_requirements=
activation_gate=ORC0
charter=charters/expansion-cli/CLI3.md
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# CLI3 — Aggregate `check` and transactional `fix` commands

Authority state is derived at dispatch. The canonical CLI must validate the current phase, exact predecessor receipts, activation/release gates, external authorizations, and atomic admission before mutation.

## Independently acceptable outcome

Aggregate `check` and transactional `fix` commands. The current owner is **separate package launchers and command-local project logic**. The final and sole owner is **one `verter` application service with thin command adapters**. This charter accepts one authority/migration/cutover boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: `packages/binary-launcher`, `packages/verter-lsp`, `packages/verter-tsc`, `crates/verter_mcp_server/src`.
- Named API/data boundaries: `ApplicationServices`, `SelectionPlan`, `Reporter`, `WriteTransaction`, `CommandCapability`.
- Mutation boundary: bounded validation, named residual deletion, and/or one atomic route switch only; no new authority may be introduced.

## Exact predecessor contracts

- **CLI5:** exact current receipt ID and digest for “Base packaging, watch mode, compatibility wrappers, and promotion”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CLIF0:** exact current receipt ID and digest for “Formatter CLI adapter”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **CLIL0:** exact current receipt ID and digest for “Lint CLI adapter”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- **Normative intent:** compose typecheck, formatter-check, and lint into one non-mutating check while keeping transactional fixes limited to lint+format engines.
- **Atomic boundary:** the production surfaces and named API/data boundaries above form this source-owned node's exclusive acceptance subset; this node owns its complete named migration population and exactly one deletion/cutover disposition.
- **Cutover evidence:** prove removal or structural rejection of **command-local semantic engine**, **non-atomic multi-file writes**, **ambiguous offset encoding** and satisfy the node-specific acceptance IDs below. A newly independent outcome requires an amendment and a new node before mutation.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **CLI3-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **CLI3-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **CLI3-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **CLI3-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `packages/binary-launcher/cli.spec.ts`, `crates/verter_mcp_server/tests`.

## Deletions and forbidden designs

- Delete or structurally reject: **command-local semantic engine**.
- Delete or structurally reject: **non-atomic multi-file writes**.
- Delete or structurally reject: **ambiguous offset encoding**.
- Never add a dual-running authority, compatibility fallback, string/regex semantic recovery, test-only production bypass, resource-capacity predecessor, sleep/poll readiness, or unqualified cache/public identity.
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 300 production LOC, 3 production files, 1 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero stale publication, silent fallback, wrong-complete result, map/provenance loss, or identity aliasing.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Stop before mutation if current source disproves the named owner/API boundary, a predecessor receipt is stale, or the complete diff will not fit one review context.
- Abort the candidate on unexplained output, source-map, diagnostic, cancellation, allocation, latency, or RSS divergence; do not convert it into residue locally.

## Targeted verification

1. `pnpm --filter @verter/binary-launcher test`
2. Run every final command in the bound `ts-domain` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and lower-severity findings

Apply `architecture-3`: 3 fresh distinct harness tasks covering exactly `adversarial`, `conformance`, `architecture-specialist`. P0/P1 block final acceptance. A P2 may be accepted only when the currently binding owning policy explicitly authorizes that disposition and the evidence binds its stable fingerprint, named owner, class-wide bounded sweep, and next-cycle receipt; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete 3/3 current-round profile to contain independent clean PASS reports on the exact candidate tree, plus `independent-full` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Dispatch-time immutable bindings

The packet is incomplete and dispatch must fail unless it embeds: exact candidate base SHA/tree; canonical whole-authority digest; current charter digest; each source/corpus revision and excerpt digest; exact frozen worktree and `codex/<node>` branch; the static conflict-domain path/symbol sets and acquired lease receipt; the complete gate command list; 3 fresh distinct harness review tasks for exactly `adversarial`, `conformance`, `architecture-specialist`, deterministic low|medium|high effort and exact author/task/agent/provider/model bindings; immutable-review-worktree and cleanup policy; and the required terse report-back schema. These values are derived by the canonical CLI from current authority and evidence; implementers never invent or restamp them.

## Citations

- `source:successor-expansion.md:L1556`

## Reconciled source-plan contract

**Intent:** compose typecheck, formatter-check, and lint into one non-mutating check while keeping transactional fixes limited to lint+format engines.
**Predecessors:** `CLI5`, `CLIF0`, `CLIL0`.
**Subblocks:** (1) non-mutating aggregate `check = typecheck + lint + fmt --check`; (2) explicit fix plan/order excluding typecheck mutation; (3) combine safe lint edits and formatting against one exact revision; (4) conflict/stale/rollback behavior; (5) atomic multi-file commit; (6) solely build, hash, sign, republish, and clean-install-test a new aggregate artifact through the platform/package matrix and process established by `CLI5`, with watch/report/performance tests and exact-candidate review.
**Acceptance:** the aggregate artifact records verified lineage to the accepted `CLI5` base artifact and packaging process but has its own identity/version reflecting the new command registry; `check` never writes and preserves each service’s result/provenance; `fix` previews and validates one transaction; partial failure leaves authored files unchanged.
**Forbidden:** implicit fixes during check, formatter-as-lint, arbitrary external edit auto-application, or per-file commits that leave a half-updated project.
**Deletion/abort:** `CLI3` is the sole aggregate republish/version owner; remove duplicate aggregate command adapters only after parity; abort aggregate mutation without recoverable atomicity or any attempt to present the changed binary as byte-identical to `CLI5`.

## 15. Future vertical portfolio—not active kernel charters

### 15.1 Definition of a complete tooling vertical

A future vertical is first-class only when its manifest truthfully covers every applicable cell below. “Full LSP” means all applicable operations, not registered no-op handlers.

| Domain | Required disposition |
|---|---|
| Syntax | Owned carrier parser, admitted shared parser, or explicit host-language reuse |
| Activation | Pre-projection and post-snapshot claims with exact provenance |
| Semantics | Framework-local facts and TypeInfo operation ownership |
| Component information | TypeInfo-backed `ComponentInfo` facets and public projection |
| Maps | Exact authored/generated/embedded/format/action map cells |
| Diagnostics | Parse, framework, type, configuration, and project diagnostics as applicable |
| Lint/actions | Native rules, safe fixes, refactors, and applicability/version matrix |
| Formatter | Outer carrier/embedded composition or explicit host-printer reuse |
| IDE/LSP | Completion, hover, signature help, definition/type-definition/implementation, references, highlights, symbols, rename, code actions, formatting, links, colors, folding, selection, semantic tokens, inlay hints, call/type hierarchy, linked editing, and file operations where applicable |
| Workspace | Index contributions, imports/consumers, auto-import, moves, assets/links/routes as applicable |
| Custom Elements | Separate producer and consumer disposition with evidence |
| Coexistence | `auto|disabled|workspace|full` presets, per-capability ownership mask, and zero-work proof |
| Public products | Rust, NAPI, WASM, LSP, MCP, and CLI capability truth |
| Quality | Immutable oracle/corpus, incremental=fresh, cancellation, Unicode/maps, equivalent-work performance, RSS plateau, security, and three independent exact-candidate reviews |
| Compiler | `Supported`, `FutureSeparateTrain`, or `NotApplicable`; never inferred from tooling |

The two workflow skills instantiate the generic vertical lifecycle from §5.3 only when that vertical becomes the selected next investment. This keeps detailed implementation charters current with the then-accepted kernel and exact framework release.

### 15.2 Portfolio dossiers

| Vertical | Geometry and parser decision hypothesis | Verter-specific product value | Required special cells | Compiler disposition |
|---|---|---|---|---|
| **MDX** | Dedicated Markdown/MDX carrier; reuse OXC for embedded ESM/JSX, not Volar architecture | React-component auto-import, path/link completion, file-move link updates, refactors, bounded CPU, cross-framework component graph; reusable Vue `<block lang="md">` content tooling | Markdown/MDX recovery, ESM/JSX maps, generic provider first and React-specific provider after `RCTP`, assets/links/headings, remark config capture without plugin execution | `NotApplicable` |
| **Astro** | Dedicated heterogeneous `.astro` carrier; owned frontend selected by proof, with embedded OXC/HTML/CSS products | First-class IDE, lint/fixes, formatting, TypeInfo, component metadata, island navigation, assets, cross-framework graph, Rust/NAPI/WASM/CLI | Frontmatter, directives, component kinds, client islands, script/style regions, exact nested maps, all applicable LSP rows, CE consumption | `FutureSeparateTrain` |
| **Lit** | TS/JS host parsed by OXC plus symbol-proven tagged HTML attachments | High-quality embedded HTML/CSS, TypeInfo/WC production, cross-framework consumption | Hole maps, directives, decorators/static properties, reactive fields, events, slots, parts/CSS properties | `NotApplicable` |
| **React** | OXC/TypeScript TSX plus semantic overlay | Component/hook/props/ref/children metadata, React Compiler lint, cross-framework graph; later Next project semantics | Server/client directives remain framework/project scoped; CE consumption and explicit wrapper production only when proven | `NotApplicable` |
| **Solid** | Same OXC/TypeScript TSX geometry, separate semantic profile | Signals/effects/resources/control flow and component semantics; prevents React-shaped kernel | Immediate React counterfixtures, JSX binding/event differences, SolidStart later | `NotApplicable` |
| **Preact** | OXC/TypeScript TSX; separate native and `preact/compat` profiles/evidence | Low-cost reach after React while retaining real semantic differences | Compat resolution, signals where admitted, CE consumption; no claim that React support makes Preact automatic | `NotApplicable` |
| **Qwik 2** | OXC/TypeScript TSX semantic overlay for one exact Qwik 2 epoch only | Resumability/QRL/component semantics and underserved tooling if release stabilizes | QRLs, `$` boundaries, optimizer directives, serializability, Qwik City later; Qwik1 negative fixtures | `NotApplicable` |
| **Stencil** | OXC/TypeScript TSX semantic overlay plus CE producer | Strong standards interoperability and component metadata across consumers | Decorators, props/state/events/methods/watchers, generated declarations/CEM as oracle inputs, CE production | `NotApplicable` |
| **Angular** | Neutral HTML plus TS host roles; external and embedded attachment profiles | Large ecosystem, exact template TypeInfo, metadata, lint/actions, cross-framework CE consumption | Decorators, standalone/modules, signals, directives/pipes, microsyntax/control flow, project association, Angular Elements separately | `NotApplicable` |
| **Alpine** | Neutral HTML with attribute-level claims and embedded JS expressions | High marginal DX: scope-aware definition/rename/hover/completion/diagnostics | `x-data` descendant scopes, refs, loops, modifiers, dynamic uncertainty, CE consumption | `NotApplicable` |
| **HTMX** | Neutral HTML with attribute-level request/target/trigger/swap claims | Low-cost navigation from selectors and captured project routes; underserved HTML-first DX | attribute inheritance, selectors, extensions, route metadata as optional project input, CE consumption | `NotApplicable` |
| **Marko** | Dedicated `.marko` carrier/parser | Native performance, unified metadata/index/public surfaces; cross-framework intelligence beyond incumbent tooling | tag resolution, params/attributes/events, control flow, style/script, exact recovery/maps | `NotApplicable` |
| **Ember/Glimmer** | Dedicated `.gjs/.gts`/template-tag or attached template geometry per exact Glimmer/Ember epoch | Framework-aware TypeInfo, component metadata, layout/resolution, cross-project graph | strict/classic modes, helpers/modifiers, named/positional args, Ember resolver/layouts; CE consumption | `NotApplicable` |
| **HTML** | Independent neutral parser initially forked from Vue and de-Vue’d | Standards source tooling and substrate for Angular/Alpine/HTMX/WC | standards recovery, a11y, links/assets/selectors, formatter, full applicable LSP | `NotApplicable` |
| **Web Components** | Standards semantic/interoperability facet, not a source carrier | Cross-framework props/attrs/events/slots/methods/parts intelligence and CEM | scoped registries, declaration/registration/consumer distinction, producer/consumer dispositions | `NotApplicable` |

The user’s “htmlx” item is interpreted as **HTMX**, consistent with the supplied examples and `hx-*` semantics. If a separate language named HTMLX was intended, it requires its own feasibility dossier rather than silently sharing the HTMX profile.

### 15.3 Astro first-class commitment

Astro is not an adapter around the official language server and is not blocked on a compiler. Its eventual full vertical must own:

- Astro parsing/recovery and exact authored region/source-unit maps;
- frontmatter and all admitted embedded language composition;
- component/directive/island/asset semantics;
- TypeInfo projections and public component information;
- diagnostics plus native Astro-specific lint rules and safe fixes/actions;
- complete applicable IDE/LSP, including cross-framework island navigation, auto-import, rename, and file moves;
- native whole-document and range formatting;
- workspace graph/index contributions;
- Rust, NAPI, WASM, MCP, LSP, and CLI surfaces with capability truth;
- cancellation, incremental=fresh, Unicode/map, performance, RSS, security, and conformance evidence.

A future Astro compiler train may investigate an owned Rust frontend/code generator or an `@astrojs/compiler-rs`-like product. That train has its own oracle, ABI, runtime-output scope, and terminal. Astro tooling neither waits for it nor claims it.

### 15.4 MDX first-wedge commitment

The MDX vertical is intended to replace the Volar-based integration path, while respecting existing MDX syntax/type semantics as oracles rather than importing Volar as Verter architecture. Its full release must specifically close:

- React-component discovery and auto-import intelligence;
- Markdown/MDX path and link completion;
- exact, atomic link updates on file moves;
- component/link/heading-aware refactoring;
- measurable high-CPU and long-session memory regressions;
- TypeInfo/component metadata for MDX exports/props/provided components;
- reusable Markdown/MDX embedded-content service for admitted Vue custom blocks.

`MDXR0` is evidence only. Before full MDX can advertise React-specific discovery/auto-import, the future vertical program must ratify a bounded production train: `RCP0-FUTURE` locks one exact React release, provider API, oracle, maturity, zero-work, and performance gates; `RCP1-FUTURE` implements/promotes the React `ComponentInfo` provider over accepted React facts; `RCP2-FUTURE` migrates or deletes proof code and passes public/index/performance conformance. The full MDX terminal depends on `RCP2-FUTURE`, never on `RCTP` or `MDXR0`. Generic MDX links, moves, refactors, and framework-neutral component candidates remain independently useful.

Arbitrary remark/rehype plugin execution remains outside Rust/WASM. Static captured configuration may select admitted syntax extensions; unsupported executable transforms return `NeedInputs`/`Unsupported` or run only behind a separately trusted host contract.

### 15.5 Project-profile roadmap

Project profiles are semantic overlays over already-resolved carrier/framework facts and captured project structure. They never become TypeScript project owners.

Only Next is selected as an implementation candidate. Every other row below is explicitly deferred and unordered; row position carries no priority. Scored profiles show the dated §6 hypothesis, while `unscored` means a feasibility lock must produce evidence before the profile may enter any investment order.

| Decision | Profile | Score | Required semantic focus | Prerequisites / reason deferred |
|---|---|---:|---|---|
| First candidate | Next.js | 4.2 | App Router file roles, layouts/pages/loading/error/metadata, RSC, client/server directives, Server Functions, route/cache/rendering semantics | React + MDX full prerequisites; generic project identity/index |
| Deferred; counterfixture first | Nuxt 4 | 3.3 | pages/layouts/plugins/middleware/server routes, auto-imports, client/server boundaries, Vue/Nitro associations | Vue accepted; challenge generic project vocabulary before any implementation rank |
| Deferred; counterfixture first | SvelteKit | 3.1 | routes/layouts/load/actions/hooks, universal/server files, Svelte associations | Svelte accepted; challenge generic project vocabulary before any implementation rank |
| Deferred; unranked | Astro project | unscored | file-based routes/endpoints, layouts, content collections, integrations, assets, islands and source-observable build-mode facts | full Astro tooling vertical plus independent feasibility/score lock |
| Deferred; unranked | Angular workspace | unscored | projects/configurations, routes/lazy boundaries, templates/styles/assets, build targets and library/app relationships that are source/config-observable | full Angular semantic vertical plus exact project/config epoch and feasibility/score lock |
| Deferred; unranked | React Router | unscored | route modules, loaders/actions, server/client boundaries, framework-mode conventions | exact release and React prerequisite plus independent feasibility/score lock |
| Deferred; unranked | Remix | unscored | route/file conventions, loaders/actions, server/client data and deployment-visible source semantics | exact release/lineage decision and React prerequisite plus independent feasibility/score lock |
| Deferred; unranked | SolidStart | unscored | routing, server functions, islands/hydration, data/cache boundaries | Solid vertical plus independent feasibility/score lock |
| Deferred; blocked | Qwik City | unscored | routes/layouts/loaders/actions, resumability boundaries and source-observable optimizer facts | accepted exact Qwik 2 profile first; Qwik 1 remains excluded |
| Deferred; unranked | TanStack Start | unscored | file/code routing, server functions, loaders/cache and client/server boundaries | exact stable product/release evidence plus independent feasibility/score lock |
| Deferred; unranked | Docusaurus | unscored | docs routes, MDX component environment, sidebars, links/assets and plugin-config facts | MDX vertical plus independently bounded static-config contract and feasibility/score lock |
| Deferred; unranked | VitePress | unscored | Markdown/Vue content, routes, theme/components, links/assets and static config facts | Vue + Markdown/MDX substrate plus independent feasibility/score lock |

Next is the intended first implementation because it combines reach and high-value semantics TypeScript does not know, not because it defines the generic schema. Before any project-profile contract becomes Stable, Nuxt and SvelteKit adversarial fixtures must demonstrate that route/module/client-server vocabulary is not merely Next renamed. Promotion of Next does not automatically rank any deferred row.

### 15.6 Non-active horizontal semantics ledger

After the architecture and one full new vertical are proven, prioritization should compare new framework work against horizontal semantics that benefit several verticals at once:

- CSS Modules, Sass/SCSS/Less semantic references, custom properties, and later evidence-gated utility-framework semantics;
- Vite/source-module facts such as aliases, assets, query imports, `import.meta.glob`, and environment typing, without bundler/HMR ownership;
- JSON/JSONC/YAML and statically captured configuration projections, without executable configuration in Rust/WASM;
- package exports/imports/workspaces and monorepo cross-package component relationships.

These are portfolio records, not active DAG nodes or hidden vertical prerequisites. Each needs its own authority/reuse dossier and may be selected ahead of a lower-value framework when measured cross-vertical unlock exceeds the next vertical score.

## 16. Superseded-proposal disposition

Useful architecture from revision 3 is migrated rather than lost:

| Revision-3 area | Revision-4 disposition |
|---|---|
| Global `EXT0`, `TVG0`, `PJG0`, `X1` | Superseded by `UAK1`, independent terminals, and continuous soak suites |
| `KX` catalog rename | Replace with `VID0/CAT0`; preserve one snapshot/owner and avoid cosmetic rename |
| `CDX0` activation | Split and strengthen in `VID0`, `DEM0`, `EAK0`, `COX0` |
| `EMB0` | Preserved as the sole embedded codec/authored-map-chain authority, consuming repaired `SourceUnitId` plus stable `AttachmentId`/`RegionId`; defines no independent embedded identity |
| `CMX0/CMX1` | Type-bearing envelope rejected; useful presentation compatibility moves to `TIF1` |
| `SGX0A/B` | Retained conceptually under `IDX0` with TypeInfo/index authority correction |
| `PJX0` | Projection admission/maps go to carrier owners + TCM1/TCM2; formatter/action maps stay distinct |
| `ACT0` | Retained as authored transaction substrate consumed by `LRA0`, formatter, CLI fix/moves |
| `OBS0/SEL0` | Retained as captured-input/selection concepts consumed by `CFG0/DEM0/CLI1` |
| `RFX0/AIX0` | Retained as downstream refactor/auto-import consumers of `IDX0`, TypeInfo, and exact actions |
| `FCX0` | Replaced by explicit optional `CarrierCompilerBackend` and per-vertical compiler disposition |
| `VWC*/SWC*` | Consumer-only framing replaced by `VCE0/SCE0` producer + consumer retrofits |
| Fifteen full vertical charter families | Removed from active DAG; regenerated one at a time after `UKS0` |
| Formatter/lint/CLI mega-terminal chain | Replaced by independent `FMT4`, `LNT3` plus rule packs, base `CLI5`, and optional aggregate `CLI3` promotions |

No revision-3 charter i

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-EXP-L1556-739A0A78C831

- Kind: `context`
- Source: `successor-expansion.md:1556-1556`
- Applicability: `CLI3`
- Exact text SHA-256: `739a0a78c831ae87d18b64e2ad4e41ac834d4b5aa05ba89570070c48c848317f`

~~~~markdown
### `CLI3.md` — Aggregate `check` and transactional `fix` commands
~~~~

### SRC-EXP-L1558-FD0A8F2A8568

- Kind: `forbidden`
- Source: `successor-expansion.md:1558-1563`
- Applicability: `CLI3`
- Exact text SHA-256: `fd0a8f2a8568863270c0cbe0f9003634b0f018b7c59ed34be867390f9b53d8b2`

~~~~markdown
**Intent:** compose typecheck, formatter-check, and lint into one non-mutating check while keeping transactional fixes limited to lint+format engines.
**Predecessors:** `CLI5`, `CLIF0`, `CLIL0`.
**Subblocks:** (1) non-mutating aggregate `check = typecheck + lint + fmt --check`; (2) explicit fix plan/order excluding typecheck mutation; (3) combine safe lint edits and formatting against one exact revision; (4) conflict/stale/rollback behavior; (5) atomic multi-file commit; (6) solely build, hash, sign, republish, and clean-install-test a new aggregate artifact through the platform/package matrix and process established by `CLI5`, with watch/report/performance tests and exact-candidate review.
**Acceptance:** the aggregate artifact records verified lineage to the accepted `CLI5` base artifact and packaging process but has its own identity/version reflecting the new command registry; `check` never writes and preserves each service’s result/provenance; `fix` previews and validates one transaction; partial failure leaves authored files unchanged.
**Forbidden:** implicit fixes during check, formatter-as-lint, arbitrary external edit auto-application, or per-file commits that leave a half-updated project.
**Deletion/abort:** `CLI3` is the sole aggregate republish/version owner; remove duplicate aggregate command adapters only after parity; abort aggregate mutation without recoverable atomicity or any attempt to present the changed binary as byte-identical to `CLI5`.
~~~~
