# tsserver carrier configured-project membership

Status: DESIGN (uncommitted draft for CTO/user review). Provider: tsserver only — tgo is unaffected and must stay unchanged.

## Problem

Verter compiles each `.vue`/`.svelte` source to in-memory virtual carrier TS surfaces — an IDE surface (`src/App.vue.tsx`, template+script as one TS/JSX program) and a public-API surface (`src/App.vue.ts`, the component's `$props`/`$emit`/`$slots`). The LSP feeds these to tsserver with the tsserver `open` command carrying in-memory `fileContent` plus a `projectRootPath` hint (`crates/verter_type_runtime/src/tsserver/ipc.rs:823`). The carrier files exist only in Verter's host/VFS — they are never written to disk.

tsserver assigns each separately-`open`ed in-memory virtual carrier to its **own inferred project** (`/dev/null/inferredProject*`), NOT the configured `tsconfig.json` project. This breaks three IDE features on the tsserver backend, all sharing this one root cause:

- **(A) Case V** — auto-import completion-resolve of an **unimported workspace sibling** from a `.vue`/`.svelte` carrier. The inferred project's auto-import export index does not include configured-project siblings the carrier does not already import, so the completion carries no `source`/`data`/`hasAction` and resolve produces no import edit.
- **(B) imported-type cross-file rename** — renaming a prop usage on `<Child headline=…>` in a parent `.vue` whose prop type is an imported interface in a third `.ts` file must reach that third file. This requires the relevant carrier surfaces to share a program with the parent.
- **(C) closed-child cross-file rename via `handle_rename`'s own sync** — `ensure_provider_synced` opens the parent first then children, but at rename time the parent program is already built, so a child opened after it lands in its own inferred project and tsserver returns only the parent's rename group.

tgo (the other provider) does not have this problem: it uses a whole-folder LSP project model where every file under the initialized root is implicitly in scope.

## Confirmed root-cause analysis (raw-tsserver `projectInfo` evidence)

All evidence below is from read-only experiments driving a real tsserver 5.9.3 the same way Verter's `open_file` does (in-memory `fileContent`, `projectRootPath`, the virtual `.vue.tsx`/`.vue.ts` extension). Scripts and raw output are retained under `.feedback/_mem_*.mjs` / `_mem_*.out`.

### Membership of in-memory virtual carriers

| Scenario | Result |
| --- | --- |
| in-memory virtual `src/App.vue.tsx`, `projectRootPath` set | `configFileName=/dev/null/inferredProject1*` — **INFERRED** |
| control: real on-disk `src/utils.ts`, same `projectRootPath` | `configFileName=…/tsconfig.json` — **CONFIGURED** |
| in-memory virtual `src/App.vue.ts` (`.ts` extension) | `/dev/null/inferredProject3*` — **INFERRED** |

The `.tsx` vs `.ts` extension is **not** the cause; it is on-disk-vs-virtual. `projectRootPath` causes the configured project to *load* (a `projectLoadingFinish` for `tsconfig.json` fires) but does **not** make the virtual file a *member* of it.

### Why some cross-file features partly work

- Cross-file **go-to-definition** works even from an inferred carrier — tsserver follows the module-resolution graph from the carrier's own imports.
- Cross-file **rename/references** work when the carrier **directly imports the declaration file** — the inferred project's program includes the carrier's import closure, and the reverse search runs within that program.
- Cross-file **auto-import of an unimported sibling** does **not** work — the inferred project's export index excludes configured-project siblings not in the carrier's import closure (Case V).

### The decisive late-open membership experiment

Topology: parent `App.vue.tsx` imports child `MyComp.vue` (→ `MyComp.vue.ts` API surface); rename the prop from the parent.

| Phase | Result |
| --- | --- |
| parent only (child not opened) | 1 rename group |
| child opened LATE (after parent program built), 10 s of retries | **NEVER recovers** — 1 group |
| touch parent (no-op `updateOpen`) | still 1 group |
| close + reopen parent AFTER child already open | **2 groups (reachesChild=true)** — RECOVERED, but parent still inferred |

This proves the gap is **project membership, not indexing latency**: a configured-project (or any) program is fixed at build time; a file opened after it is built does **not** retroactively join it. Cross-file rename between two carriers only works if they end up in **one shared program** — which happens only when the parent is (re)opened while the child is already open, or children are opened before the parent (the `did_open` prewarm).

### Two distinct membership properties

- **Shared-program membership** — parent + child carriers in one program (even inferred). Fixes (B)/(C). Achievable by ordering (children before parent) or close+reopen.
- **Configured-project membership** — the carrier sees the full tsconfig file set + the project-wide export index. **Required for (A)/Case V**, because an unimported sibling is not in the carrier's import closure; ordering/shared-program cannot help (A).

The durable fix must deliver configured-project membership; it then subsumes shared-program membership.

### The three symptoms share the root cause

| Symptom | Mechanism | Shared root cause |
| --- | --- | --- |
| (A) Case V auto-import | inferred export index excludes configured siblings | carrier not a configured-project member |
| (B) imported-type rename | parent + child carriers + third file not in one program | carriers in separate inferred projects |
| (C) closed-child rename | child opens after parent program built | per-carrier inferred project, fixed at build time |

### Production-state reconciliation (the B/C flakiness)

The real tests for (B)/(C) (`rename_cross_file_imported_prop_tsserver`; the `#[ignore]`'d `rename_cross_file_prop_child_closed_unprewarmed_tsserver`) currently **pass** on an unloaded machine under `VERTER_REQUIRE_TSSERVER=1` (5/5 and 1/1 respectively, real tsserver, not vacuous skips). A prior session measured `rename_cross_file_imported_prop_tsserver` failing 5/5 under heavy co-resident full-suite load, on byte-identical test code.

This is consistent: the production masking — the `did_open` imported-carrier prewarm (children before parent), `ensure_provider_synced`, and a 12×500 ms settle loop — usually lands the right ordering on an idle machine but **races under load**. The membership gap is deterministic at the raw boundary; the masking is timing/load-fragile. The symptom is real and presents as flaky. Case V (A) is unmasked and currently uncovered by any discriminating test (the only carrier auto-import E2E, `AutoImportCase.vue`, imports `computed` from `vue`/node_modules, which the inferred export map *does* include, so it does not discriminate the configured-sibling gap).

## Fix-shape matrix (what actually confers configured membership)

All measured against real tsserver 5.9.3.

| Shape | Membership | Case V auto-import | Notes |
| --- | --- | --- | --- |
| `projectRootPath` hint alone (current Verter) | INFERRED | fails | the status quo |
| `openExternalProject` with carriers as `rootFiles` | INFERRED | fails | external project does not confer configured auto-import membership for the carrier |
| `openExternalProject` with the generated tsconfig file as a `rootFile` | INFERRED | fails | referencing a config as an external root did not create a serving configured project |
| generated config (non-`tsconfig.json` name) in a cache dir | INFERRED | fails | tsserver auto-discovery only walks up for `tsconfig.json`/`jsconfig.json` |
| `include` glob `**/*.vue.tsx`, carrier NOT on disk | INFERRED | fails | globs enumerate only on-disk files |
| **`tsconfig.json` whose `files` lists the carrier, on the carrier's upward-discovery chain** | **CONFIGURED** | **works** | the only auto-discovery mechanism that works |
| **carrier written on disk under `include`** | **CONFIGURED** | **works** | works but writes generated code into the user tree |
| **empty on-disk stub at the include path + in-memory `open` override** | **CONFIGURED** | **works**, correct specifier | lighter disk footprint, but still user-tree pollution |
| **relocated carrier under a cache `vroot` + generated `tsconfig.json` with `rootDirs`** | **CONFIGURED** | **works**, correct specifier (incl. nested) | the chosen design — see below |
| relocated carrier + generated config with broad `paths` mapping | CONFIGURED | works but **wrong** specifier (`siblingExports` not `./siblingExports`) | `paths` corrupts auto-import specifier generation — use `rootDirs`, not `paths` |

Key tsserver facts established:
- The only way to make an in-memory virtual carrier a configured-project member is a `tsconfig.json` that tsserver **auto-discovers** by walking up from the carrier's own path, whose `files` array explicitly names the carrier. tsserver honors an in-memory-opened buffer for an explicitly-`files`-named path even with no disk file.
- tsserver exposes **no** "add a root file to an existing configured project" request. `openExternalProject` does not confer configured-project auto-import membership for a carrier; `updateOpen` is content/open-state sync; `compilerOptionsForInferredProjects` only affects inferred projects.

## Chosen design — Verter-owned shadow configured project (tsserver)

Per the un-primed codex-architect verdict (two neutral legs; `.feedback/_mem_architect.out`, `_mem_architect2.out`), with the loader mechanism empirically validated.

Relocate **only the tsserver carrier provider paths** under a stable Verter cache root, and generate a real `tsconfig.json` on that relocated carrier's upward-discovery chain that merges the shadow carrier tree with the real source tree via `rootDirs`. The user's real `tsconfig.json` is never read-modified or shadowed; no files are written into the user's source tree.

### Layout

```
<verter-cache>/tsserver-shadow/<workspace-id>/<project-id>/
  tsconfig.json            # generated, discovered by walking up from vroot/src
  vroot/
    src/App.vue.tsx        # relocated tsserver carrier (content stays in-memory)
    src/App.vue.ts
```

Real source is untouched: `<userRoot>/src/App.vue`, `<userRoot>/src/siblingExports.ts`.

### Generated `tsconfig.json`

```jsonc
{
  "extends": "<userRoot>/tsconfig.json",
  "compilerOptions": {
    "rootDirs": ["<verter-cache>/.../vroot", "<userRoot>"]
  },
  "files": [
    "<userRoot>/src/siblingExports.ts",
    "<verter-cache>/.../vroot/src/App.vue.tsx"
  ],
  "references": [ /* preserved from the real config if not inherited via extends */ ]
}
```

- `files` is generated from `verter_workspace`'s parsed `ConfiguredMembership.materialized_files` (the real physical members) plus the relocated tsserver carrier roots. Use an explicit `files` array — do not rely on inherited `include`/`exclude` for root membership.
- `rootDirs` lists `[vroot, realRoot]`. If the real config already has `rootDirs`, resolve them to absolute paths and merge. **Do not** add broad `paths` mappings (they corrupt auto-import specifier generation — validated).
- Inherited `baseUrl`/`paths`/`moduleResolution`/`jsx`/`types`/`references` come through `extends`; the shadow config adds only what virtual membership and `rootDirs` equivalence require. Preserve `references` explicitly if `extends` does not carry them.

### Why this works (validated)

A `rootDirs`-only shadow project (relocated carrier under `vroot/src`; generated `tsconfig.json` at the shadow project root with `rootDirs:[vroot, realRoot]`, `extends` the real config, `files` = carrier + real members):
- `projectInfo` for the carrier → **CONFIGURED** (the generated config is discovered walking up from `vroot/src`).
- auto-import of an unimported sibling → `source=./siblingExports`, specifier `import { unimportedHelper } from "./siblingExports";` — **correct**; a nested import resolves to `./nested/deep` — **correct**.
- definition resolves to the real `<userRoot>/src/siblingExports.ts`; semantic diagnostics show **no** TS2307 (module-not-found) — `rootDirs` resolved the import.

### Request targeting

For carrier-originating tsserver requests, pass the generated project's `projectFileName` where the protocol allows it, so the carrier is served by the shadow configured project rather than an accidental inferred/real project. Assert via `projectInfo`.

### Performance contract

- The generated `tsconfig.json` is rewritten only when **root membership** changes (SFC add/remove, real tsconfig membership change, carrier-path-scheme change, configured-project reload) — **membership-rate, not keystroke-rate**.
- Keystroke edits update virtual carrier **content** through the normal `open`/`updateOpen` path; they never rewrite the config.
- Keep all project SFC **public-API** carriers as managed virtual roots (auto-import completeness). Add **open** IDE-TSX carriers for direct-query membership; do not keep all closed IDE-TSX carriers as roots (they are heavier).
- Acknowledged cost: the shadow project duplicates the real TS project graph inside tsserver (memory/warm-state). This is the primary reason to evaluate the plugin alternative first (below).

### Acceptance gate — auto-import specifier correctness

The hard correctness gate is that auto-import suggests the specifier the user would write in the real `.vue`. Required cases: virtual carrier → real sibling (`./siblingExports`); nested relative imports; inherited `baseUrl`; inherited `paths`; monorepo package configs; Windows paths; rename edits crossing carrier and real files. Rely on `rootDirs` for this — do not reimplement TypeScript specifier generation. A narrow defensive postprocessor may rewrite only exact shadow-prefix leaks (`<verter-cache>/…/vroot/…` → real path) if any appear, but it is a backstop, not the primary mechanism.

## Rejected alternatives

1. **On-disk carrier materialization (real content under the user tree).** Correct in tsserver terms but writes keystroke-rate generated code into user-visible paths; pollutes git/editors/watchers/linters/build; risks stale carriers after crashes; depends on the user's `include` shape. Reject.
2. **Empty on-disk stubs under the user tree + in-memory override.** Validated to work and to produce correct specifiers, with a lighter disk footprint than (1), but still creates user-visible artifacts for an internal tsserver workaround, fails explicit `files` configs, and affects build/lint/test/watch tooling. Reject as the architecture (cache-dir artifacts are acceptable; user-tree stubs are not).
3. **Mutating the user's real `tsconfig.json`.** Confers membership but violates ownership, creates churn/merge noise, and has a bad crash-mid-update failure mode. Reject.
4. **`openExternalProject` / `projectRootPath` / `configure` / `compilerOptionsForInferredProjects` / ordering as membership fixes.** None confers configured-project auto-import membership for a virtual carrier (probed). Ordering/shared-program fixes (B)/(C) only and cannot fix (A). Reject as the architecture for (A); a shared-program ordering fix may ship only as an explicitly scoped partial for (B)/(C).
5. **A generated config in an arbitrary cache dir, targeted via `projectFileName`/`openExternalProject` (the first-draft loader).** Does not load — auto-discovery only finds `tsconfig.json` on the file's upward path, and `openExternalProject` referencing the config did not serve the carrier. Reject.
6. **Relocation with broad `paths` mappings.** Confers membership but produces wrong import specifiers. Reject in favor of `rootDirs`.

## Cleaner alternative to prove first — tsserver plugin

A tsserver **plugin** that injects Verter carriers into the **existing** configured project (server-side, via the language-service host `getExternalFiles` / snapshot hooks) would be architecturally superior: no duplicate configured project, real paths preserved, no generated config files. If it can make the in-memory carriers configured-project members with correct project-wide auto-imports and snapshots, it is the better design.

Current state: a `@verter/typescript-plugin` package already exists and proxies the language-service host (`resolveModuleNameLiterals`, `getCompilationSettings`, `getScriptSnapshot`, and many LS methods — `packages/typescript-plugin/src/index.ts`), but it is **not loaded in the production tsserver spawn** (`tsserver_plugin_args` is test-only; `TsserverTypeProvider::spawn` does not pass plugin args in production — `crates/verter_type_runtime/src/tsserver/ipc.rs:679-727`). It does not currently use `getExternalFiles` for membership.

The implementation block runs **one focused feasibility probe**: does a plugin's `getExternalFiles` (plus snapshot wiring for the in-memory carrier content) make the carriers members of the existing configured project with correct project-wide auto-import and rename? If yes, adopt the plugin path. If no, the shadow-project relocation is mandatory.

## Crate / module impact

**`verter_session` is NOT required.** Carrier content production (`get_public_api`, IDE TSX compile via `CompileTarget::IDE`) already exists in `verter_session` and is reused unchanged for content only. The membership architecture lives in:

- **`verter_workspace`** — owns real-config parsing, `ConfiguredMembership.materialized_files`, the carrier path scheme (`resolver::provider_id_for_source` / `provider_ide_id_for_source` / `source_id_from_provider_id`, `resolver.rs:245-299`), the shadow-project plan (cache layout, per-configured-project manifest), and the generated `tsconfig.json` contents. The carrier-path scheme becomes **provider-specific**: for tsserver it returns the relocated shadow path (`<cache>/…/vroot/src/App.vue.tsx`); tgo keeps the co-located path. The reverse map (`source_id_from_provider_id`) must invert the shadow path.
- **`verter_type_runtime`** — owns tsserver behavior: a new **defaulted no-op** `TypeProvider` lifecycle method (e.g. `configure_project_membership(manifests)`) that tsserver implements to ensure the generated configs exist/are loaded and to target requests via `projectFileName`; tgo no-ops it (consistent with existing defaulted lifecycle methods `resync_open_files`, `configure_paths`, `update_workspace_folders`, `load_file`). tsserver opens relocated carriers with in-memory content and never relies on `openExternalProject` for membership. (If the plugin path is chosen, this instead wires plugin loading + `getExternalFiles` snapshot delivery.)
- **`verter_lsp`** — owns sync orchestration (`ensure_provider_synced`, the `did_open` prewarm, `carrier_sync_state_for_source`, `provider_sync.rs`): apply the membership manifest and ensure required carrier content is synced before queries, then map diagnostics/edits/locations between the shadow carrier paths and the real `.vue`/`.svelte` sources (the existing `external_ide_context` / `resolve_carrier_ide_range_strict` mapping layer extends to the shadow path).

`verter_session` is touched only if its public API turns out to be the sole place to carry new provider/sync metadata (e.g. a batch public-API generation API for performance) — not required by the membership architecture itself, and gated on user sign-off if it arises.

> Note: editing `verter_session` requires explicit user sign-off (project policy). The design is structured to avoid it. If implementation discovers an unavoidable `verter_session` need, STOP and escalate to the CTO/user before proceeding.

## Block decomposition

This is **one architectural fix**, staged. It is **not** "ordering first" — ordering is a partial mitigation for (B)/(C) and cannot solve (A).

1. **Feasibility + failing tests (foundation).**
   - Prove the plugin `getExternalFiles` membership path OR the shadow `rootDirs` auto-import correctness (the latter is already validated by the diagnostic probes; the implementation re-proves it as a committed Rust test).
   - Land the discriminating REQUIRE-mode tests **red** first (see below).
2. **Workspace shadow-project model** — `verter_workspace`: shadow cache layout, per-configured-project manifest, generated `tsconfig.json` contents, provider-specific carrier path scheme + reverse map.
3. **Provider membership wiring** — `verter_type_runtime`: the defaulted `configure_project_membership` lifecycle method, tsserver generated-config materialization/load + `projectFileName` targeting (or the plugin wiring), tgo no-op.
4. **LSP orchestration + path/edit mapping** — `verter_lsp`: apply manifest before queries; map diagnostics/edits/locations shadow↔real.
5. **Remove the masking** — drop the `did_open` prewarm ordering and the settle loops as *correctness* mechanisms once membership is real; the unprewarmed lane (C) becomes a required regression test.

If the plugin probe succeeds, stages 2–4 collapse onto the plugin path (no shadow config). The decision is made at stage 1.

Whether to ship a shared-program ordering partial for (B)/(C) independently is decided only after stage 1, and only as an explicitly scoped partial — not as the architecture for (A).

### Discriminating REQUIRE-mode tests each piece needs

- **(A) Case V** — from a `.vue` (and `.svelte`) carrier, auto-import completion-resolve for an **unimported workspace sibling** export must return a module `source` and an import edit mapped into `<script setup>` at the correct specifier. node_modules imports do not count (they pass even in the inferred project). This is the currently-missing discriminating carrier test.
- **Project membership** — `projectInfo` (or an equivalent membership assertion) for an opened carrier identifies the **generated configured project**, not `/dev/null/inferredProject*`, and the project file list contains the physical sibling, the parent IDE carrier, the child public-API carrier, and the third imported `.ts` file.
- **(B) imported-type rename** — parent prop-usage rename reaches the child carrier and the third imported `.ts` interface file, rewritten deterministically (no settle loop that can accidentally repair ordering).
- **(C) closed-child rename** — the same rename through `handle_rename`'s own sync with the child initially closed and the prewarm **disabled** (`suppress_imported_carrier_prewarm(true)`). This becomes the **primary** required regression test, no longer `#[ignore]`'d.
- **Specifier-correctness suite** — the acceptance-gate cases above (relative, nested, `baseUrl`, `paths`, monorepo, Windows).
- **Churn test** — keystroke content updates do not rewrite the generated config; membership changes do.

## Interim handling of the currently-failing/fragile tests

- `rename_cross_file_imported_prop_tsserver` (B) is a **real REQUIRE-mode-fragile** test: it passes on an idle machine but races under load. The standard `nextest --workspace` hides this (it skips vacuously without `VERTER_REQUIRE_TSSERVER=1`). Recommended interim handling: **rewrite it to be deterministically discriminating** (parent open, child closed, prewarm disabled, no settle loop that can accidentally repair ordering) and mark it `#[ignore]` with a tracked reason pointing to this block until the membership fix lands — so the REQUIRE-mode suite is honest and the standard-gate vacuous-skip is not presented as a pass. The implementation block un-ignores it (green) when membership lands.
- `rename_cross_file_prop_child_closed_unprewarmed_tsserver` (C) is already `#[ignore]`'d on this exact gap. Keep it `#[ignore]`'d (its reason already names the membership block) and make it the **primary** required regression test that this block un-ignores. After membership lands, leaving it ignored would hide regressions.
- The Case V (A) gap is currently **uncovered**. The block must add the discriminating carrier auto-import test (above); until it lands, there is nothing dishonest to mark — the gap is simply untested, and the block closes it.

Recommendation summary: do not leave `rename_cross_file_imported_prop_tsserver` as a silently-load-fragile test in the REQUIRE suite. Either `#[ignore]` it with a tracked reason now (honest), or let the membership block own taking it deterministically green. The block owns the final disposition.
