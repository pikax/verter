# Product experience: cross-surface feature preservation and exposure constitution

Status: RATIFIED by DX0 (charter `charters/expansion-product-observability/DX0.md`), consuming the accepted ORC0 trusted implementation-ledger cutover and the STP0 projection constitution. Machine products: `tests/product-experience/DX0/products/`. Train plan: `plans/expansion-product-observability.md`.

This contract binds every product surface that exposes Verter operations: the VS Code extension, the Neovim/Helix/Lapce/Zed clients, the browser playground, the `verter-lsp`/`verter-tsc`/`verter-mcp` terminals, and future inspection surfaces. It adds consumer and exposure obligations. It does not reopen any producing owner: source identity, semantic authority, TypeScript projection, mapping, lifetime and authored-edit contracts stay with their existing owners, and every shipped valid feature remains RequiredCurrent under `contracts/sfc-typescript-projection.md`.

## 1. Producer-to-surface obligation model

Every user-visible operation is produced by exactly one named producer owner and exposed through typed consumer adapters. A surface may expose, label, or omit an operation; it may not re-implement, silently substitute, or privately fork it. Informal feature lists are promotion evidence only; the canonical capability catalog (`catalogs/product-surface-catalog.toml`, owner L0) remains the single capability truth and is never superseded by this contract.

Obligations per exposed operation:

1. Name the producer owner (existing crate/package owner; never a new duplicate authority).
2. Bind the execution host class (section 3) and engine version.
3. Bind the product receipt basis (section 4) and its completeness state.
4. Satisfy the required-exposure definition (section 6) before any promotion claim.

## 2. FeatureExposureContract v1

Vocabulary (machine product `feature-exposure-contract.v1.json`):

- `operation` — the executable operation id, namespaced by terminal family (for example `lsp.hover`, `tsc.project-check.vue`, `mcp.lint-project`, `inspection.flow-return`).
- `profile` — the consuming surface profile: `editor` (VS Code and non-VS-Code clients), `browser` (playground), `cli`, `agent` (MCP), `inspection` (future inspection facade).
- `version` — the wire/schema version of the exposure, not the tool version.
- `producerOwner` — the existing owning module or team surface.
- `maturity` — one of `stable`, `preview`, `experimental`, `internal`, aligned with `docs/api-stability.md` and the catalog's `stability_class`; `internal` covers audit/diagnostic operations not advertised as product.
- `obligation` — `RequiredCurrent` for every shipped valid feature (inherited from STP0); a surface that cannot execute a RequiredCurrent operation must expose the gap (section 7), never a claim.

Rules:

- A RequiredCurrent row may not be removed, optionalized, or marked external by a consumer surface; only its producing owner's retirement path may retire it.
- Maturity downgrades on an exposed surface require the producing owner's charter, not a presentation change.
- Each row carries `exposure` state: `executable` (route + test + replay case exist), `registered` (DX1 schema/case registered, promotion blocked), or `absent` (not exposed; gaps per section 7).

## 3. HostExecutionClass

Execution classes (machine product `host-execution-class.v1.json`):

- `Portable` — runs in a pure-browser sandbox with no local native process. Today only the playground family: the WASM compiler host (`@verter/wasm`) plus the pinned in-browser `typescript@6.0.3` language service. The playground is fail-closed about engines it cannot ship: there is no WASM tsgo and none is attempted (`packages/playground/src/editor/inContextLs.ts`, pinned by `wasmTsgoFailClosed.spec.ts`).
- `NativeOnly` — requires an explicitly selected local native binary or host process: `verter-lsp` (all editor clients), `verter-tsc` (tsgo engine pinned `>=7.0.2, <7.1.0`, no tsc fallback), `verter-mcp` (stdio or localhost HTTP), native flow/semantic solving, project-wide lint and SSR reports.
- `ExternalOwner` — the executing engine is owned outside Verter and Verter only pins, provisions, and binds its identity: the TypeScript engines under provider modes (`tsgo`, `shared-tsgo`, `tsserver`, `editor-tsserver`, `extension`, `off`), and editor-provided hosts.

Rules:

- A `NativeOnly` operation may be presented in a browser product only as an explicitly selected local-native execution (DX4/DX5 companion route) or as a labelled gap. Claiming it browser-executable without a shipped build is a rejected capability claim (DX0-AC2); no hidden replacement engine may be substituted to make the claim true.
- No class change happens by moving code: a Portable port of a NativeOnly operation is a producer-owned port with its own evidence, not an adapter relabel.
- Lapce/Zed WASM launchers stay launchers: they discover and start the native `verter-lsp` and are `NativeOnly`, not Portable products.

## 4. ProductReceiptBasis

Every exposed product result binds (machine product `product-receipt-basis.v1.json`):

- `sourceRevisions` — the input basis / file revisions the result was computed from (the existing input-basis and source-identity authority).
- `projectConfiguration` — project identity plus configuration facts: tsconfig/paths, `.verterrc.json`, lint preset, vite-config trust state, framework profile.
- `engineIdentity` — engine kind and exact version: the playground's pinned `typescript@6.0.3`, `verter-tsc`'s tsgo pin, or the resolved provider engine.
- `hostIdentity` — the executing host: server binary and version, editor client and version, MCP transport, or browser runtime plus WASM build selector.
- `completenessState` — one of `complete`, `complete-empty`, `partial`, `pending`, `unsupported`, `ambiguous`, `failed`, `cancelled`, `stale`. `complete-empty` is a successful empty answer; every other non-complete state is distinct from it and from each other, and none of them may be published as a complete result.

Rules:

- Stale publication is forbidden: a result handle must be checked against the current basis before display; old-source, old-project, old-engine, old-worker and wrong-handle results are rejected.
- Cancellation prevents additional irrelevant work and releases retained handles; a cancelled request cannot warm a complete cache.
- Receipts reuse existing substrate — audit records (`verter_audit`), env-hash identity, engine pins — and add no second status store.

## 5. Rails: semantic, inspection, comparison

- **Semantic rail (normal).** TypeScript is the authority for strict Vue/Svelte projection typing: hover types, diagnostics, project checks, completion types. Provider modes select which TypeScript engine answers; nothing else does. This restates STP0's type-authority law at the consumer boundary.
- **Inspection rail.** Verter-native analysis — flow facts and flow-return inference, native-checker observations, compiler analyses (bindings, template usage, CSS semantics), component-meta — is separately labelled inspection. Inspection answers about the same source file are allowed and useful, but they are labelled, carry their own provenance, and can never substitute for a semantic-rail answer. Hover provenance display (`verter.hover.provenance`, `hover.provenance` init option) is the existing labelling edge this generalizes.
- **Comparison rail.** Comparing rails (native inspection vs TypeScript) is an explicitly selected operation per request, in the lineage of the STP1 verify-node engine comparisons. Silent substitution, or a comparison offered as the default answer to a semantic question, is forbidden.

DX0-AC3 discriminator: in one source file, native flow-return facts (inspection rail, e.g. `get_flow_return_type_with_audit` surfaced through audit records) and TypeScript hover/diagnostics (semantic rail) must remain separately queryable, separately labelled, and never merged into one answer or one authority.

## 6. Required exposure

An operation is exposed only when all four exist (DX0.4):

1. **Executable operation** — a real route a user or client can invoke on the target surface, not a disabled button, inert card, or JSON screenshot.
2. **Source-linked explanation** — the answer names its producing source and provenance.
3. **Reproducible scenario** — a replay case reproducing the operation with a pinned basis (DX6/DX6R lineage).
4. **Observable error state** — failures surface as an explicit completeness state, not as silence, guessed zeros, or a fake success.

Native-only functionality shown in a browser product requires real explicitly selected native execution; until then the surface must show the registered-gap state with promotion blocked (DX0-AC-EXPOSURE: register the DX1 schema/case first; promotion stays blocked on executable exposure).

## 7. Current-surface inventory and playground gaps

The normative inventory is the machine product `cross-surface-inventory.v1.json` (DX0.1). Summary of what it pins:

- **Editor clients (all preserved; replacements are forbidden without their own charter):** VS Code extension `verter-vscode` (16 commands, views, decorations, TS plugin, optional MCP spawn); Neovim, Helix, Lapce and Zed clients launching `verter-lsp` through the shared `crates/verter-editor-client` six-key initialization contract (`lint`, `inlayHints`, `viteConfig`, `experimental`, `hover`, `statistics`).
- **Terminals:** `verter-lsp` (LSP capabilities plus `$/verter/*` inspection methods), `verter-tsc` (tsgo-only project check/declaration emit), `verter-mcp` (49 agent tools; stdio/localhost-HTTP).
- **Browser:** the playground (WASM compiler, pinned `typescript@6.0.3` worker, per-file lint/TS diagnostics, analysis snapshot, preview).
- **Analysis operations:** 187 lint rules in `crates/verter_diagnostics` (no standalone lint CLI; LSP + MCP + NAPI surfaces), structural-only SFC formatting (content formatting delegated to Prettier/dprint), flow facts internal with audit-record edges, provider mode matrix.

Catalogued playground gaps (promotion blocked until executable route + replay case exist): tsgo project check (`vue.tsc.project_check` / `svelte.tsc.project_check`), project-wide lint (`*.host_lint`), component-meta scalar/batch surfaces, document formatting, flow-fact inspection. Partial rows (per-file browser lint vs project lint) are recorded as `partial`, not `missing`, and cannot be advertised as the full catalog surface.

## 8. Architecture selection

Selected (DX0.3): **shared producer services with typed consumer adapters.** Consumers bind to producer-owned operations through typed adapters (`crates/verter_protocol/src/inspection`, `crates/verter_session/src/inspection`, `packages/language-shared/src/inspection` are the intended future homes; they do not exist yet and are registered by their owning nodes).

Rejected:

- **Independent playground semantics** — the browser product reusing producer services through WASM bindings, never a forked semantic engine.
- **A universal browser proxy** — no surface silently uploads projects or sources to a remote or local service; explicit local-native execution (DX4/DX5) is a distinct, opt-in product with an authenticated trust boundary.

Pure-browser (`Portable`) and explicit local-native (`NativeOnly`, companion-routed) execution are distinct products with distinct receipts.

## 9. Snapshot, cancellation and boundary obligations (specified, not implemented, by DX0)

- Immutable observation basis per request; stale and wrong-handle outcomes rejected (section 4).
- Reduced client capabilities preserve semantic meaning: rich information is fetched on demand; closed or disabled views create no background semantic work.
- Cross-file or browser/native edits are authored, version-checked and atomic through the existing transaction authority.
- A protocol smoke or screenshot is not real-editor responsiveness or semantic-correctness proof; real-client claims require the actual clients (JBT1 owns the JetBrains harness).

## 10. Migration and forbidden routes

Characterise the existing user-visible feature first, route it through its surviving producer, verify consumers, then remove the named obsolete route. Do not remove useful existing behavior because a new presentation is not ready. Inert adapters may land before their activation boundary; a cutover may not retain two active semantic authorities.

Forbidden (from the DX0 charter, binding here): a second semantic/type/project engine inside a client; regex/name-based semantic truth; generated coordinates passed off as authored; hidden source uploads; arbitrary companion shell execution; dropped diagnostics or weakened typing to meet timing; guessed zeros for missing telemetry; sleeps as readiness; stale publication; unexplained UI feature removal; product-superiority claims from installation or protocol smoke alone.

## 11. Scope

DX0 is contract/inventory only: 0 production LOC, 0 production files. Consumers, adapters and runtime evidence belong to DX1–DX8 and their cross-train receivers.

## 12. Ownership

Final owner for this product outcome: `expansion.product-observability`. Semantic algorithms, project resolution, format/lint logic, TypeScript projection and edit transactions stay with their existing producing owners. Conflict domains: `public_protocol`, `capability_catalog`, `product_inspection`.

## 13. Receiving amendments

Machine product `exposure-ownership-map.json` pins the full population. Planned receivers (production-capable unless noted): DX1, DX1G, DX2, DX3, DX3T, DX4, DX5, DX6, DX6R, DX7, DX8, plus cross-train consumers BWH0, COX0, ED0, EPR0, JBT0, LSO0, LSPX0, PG0, PM0, PUB0, RFX0, SVI0, TIF1, VSC0. This contract creates no reverse edges into predecessor trains; combined-graph validation runs after every amendment. DX0's own deletion population is empty; no route is retired by this node.
