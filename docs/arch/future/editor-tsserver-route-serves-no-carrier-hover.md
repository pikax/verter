# The `editor-tsserver` route serves no carrier hover on a real workspace

Opened 2026-07-22 by the VS Code acceptance lane. **Root-caused and partly
fixed** the same day; this revision records what the causes actually were, what
landed, and the two residual blockers that keep the tier from serving.

## Symptom (original)

A real Vue project in real VS Code, Verter at default settings. The status bar
reported a connected TypeScript engine. Hovering anything in a `.vue` file
showed **nothing at all** — not a wrong type, not a Verter-native summary,
nothing. Go-to-definition did nothing.

Private corpus F (a workspace with ~77 SFCs, four configured TypeScript projects
reached through **project references** from per-package solution `tsconfig.json`
files, TypeScript installed per package):

| route | resolved provider | carrier hovers carrying TypeScript | carrier definitions |
| --- | --- | --- | --- |
| `auto` (default) | `editor-tsserver` | **0 / 51** (51 empty) | **0 / 33** (33 empty) |
| `off` (control) | `none` | 0 / 51 (27 Verter-native) | 0 / 33 |

Connecting the engine was strictly WORSE than disabling it: `off` produced 27
Verter-native hovers, the connected route produced 51 empties.

## Mechanism — three causes, found by reading the editor's own tsserver log

The lane can now capture that log (`VERTER_ACCEPTANCE_TSSERVER_LOG=1`), which is
the only place the hand-off is visible. It showed the editor DOES send
`quickinfo` / `definitionAndBoundSpan` / `references` / `completionInfo` for
`.vue` documents — the plugin's `typescriptServerPlugins.languages`
contribution works — and that all of them failed. Three independent causes:

### 1. A closed tsserver project crashed every carrier request — FIXED

Every carrier request failed as a tsserver COMMAND:

```
"command":"quickinfo","success":false,
"message":"Error processing request. Cannot read properties of undefined (reading 'get')
TypeError: Cannot read properties of undefined (reading 'get')
    at ConfiguredProject.getOrCreateScriptInfoAndAttachToProject (typescript.js:188093)
    at ConfiguredProject.getScriptSnapshot (typescript.js:188116)
    at readOriginalSource (typescript-plugin/dist/index.js)
    at CarrierMapper.readSourceText (language-shared/dist/carrier/remap.js)
    at mapCarrierSourceOffsetToGenerated (language-shared/dist/carrier/remap.js)
    at editorCarrierPosition (typescript-plugin/dist/index.js)
    at languageService.getQuickInfoAtPosition (typescript-plugin/dist/index.js)
```

`Project.close()` sets `rootFilesMap = undefined`, and `Project.isClosed()` is
exactly that test. The plugin's process-wide owner registry
(`processEditorProjectRuntimes`) was never pruned, so a carrier whose owning
configured project had been closed was routed at a dead `LanguageServiceHost`.

In a solution-style workspace tsserver does this constantly: it creates each
referenced project "to find possible configured project for <file> to open",
then removes the ones that do not contain the file. In one 40-second session:
**124** carrier requests failed this way, in the exact 23-second window between
the owning project being removed and being re-created.

FIXED — owner selection now skips closed projects and drops them from the
registry when observed.

### 2. The LSP stood down and supplied nothing — FIXED

`editor_owns_carrier_source_features()` made hover, definition, references and
rename all return early on this topology, on the reasoning that the LSP "must
not register a competing partial answer". That holds only for **rename**: VS
Code selects a single rename provider. Hover, definition and references are
MERGED across providers, so withholding Verter's answer there could only remove
information — and the plugin had none to add.

It was also inconsistent: completion was never gated, so the route answered
completion while returning nothing for hover and definition, which reads as
"IntelliSense is partly there" rather than as a broken route.

FIXED — the gate is now rename-only.

### 3. The automatic policy selected a tier it cannot verify — FIXED

`typeProviderRoutesEditorTsserver` routed `auto`, `shared-tsgo` AND `tsserver`
here, so the setting documented as "use workspace TypeScript version (tsserver)"
did not, and no setting escaped the tier. FIXED — the tier is now reachable only
through an explicit `editor-tsserver` policy.

## Residual blockers — why the tier still does not serve

With 1–3 fixed, corpus F on `editor-tsserver` produces 27 Verter-native hovers,
33/36 references and 15/15 completions — but still **0 / 51 hovers carrying
TypeScript**. Instrumenting the plugin's routing showed why: every carrier query
reports **no owner**, i.e. no live project has a READY companion for the file
under the cursor. Two causes, neither of them benign to fix:

### A. Verter publishes no carrier for the file the user just opened

On the editor-owned topology the LSP does not await IDE sync at `didOpen`
(`did_open_provider_sync_policy(EditorTsserver)` sets `await_ide_sync = false`).
In a 70-second measured session the plugin's `getExternalFiles` reported **72
companion roots** for one configured project and **0** for the project that owns
the opened carrier; the LSP log shows the opened carrier's IDE sync flushing
**66 seconds** after `carrierStoreReady`. Until then the plugin has nothing to
serve, so the editor gets nothing.

### B. VS Code routes carrier semantics to its SYNTAX server while projects churn

VS Code runs a syntax server and a semantic server. While a project is loading,
`quickinfo` / `definitionAndBoundSpan` / `references` / `completionInfo` are
routed to the **syntax** server, which has no program. In the measured session
the semantic server received the first carrier's requests, while two later
carriers' `quickinfo` and `definitionAndBoundSpan` went to the syntax server
(15 and 18 quickinfo requests respectively) and answered nothing — because
project churn from the solution-style references kept `projectLoading` true.

The tier is not fundamentally incapable: in the same sessions a carrier whose
owning project stayed alive and whose requests reached the semantic server
returned **12 / 12** references correctly. It is unreliable, and its
unreliability is dominated by tsserver project lifecycle that Verter does not
control.

## Reproduction

Needs a workspace with **project references** (per-package solution
`tsconfig.json` files referencing `tsconfig.app.json` / `tsconfig.components.json`)
and TypeScript installed per package. No in-tree fixture has that shape;
building one is the first step of any further work.

1. `pnpm --filter verter-vscode run prepare:e2e`
2. ```
   VERTER_ACCEPTANCE_WORKSPACE=<project> VERTER_ACCEPTANCE_LABEL=<letter> \
   VERTER_ACCEPTANCE_PROVIDER=editor-tsserver VERTER_ACCEPTANCE_TSSERVER_LOG=1 \
   node packages/vue-vscode/out-test/e2e/acceptance/launch.js
   ```
3. The receipt shows `carrier.hover.typescript = 0`.
4. In the profile's
   `logs/*/window1/exthost/vscode.typescript-language-features/tsserver-log-*/tsserver.log`,
   the plugin's `getExternalFiles(<project>)` lines show 0 companion roots for
   the project that owns the probed carrier.

The `off` control (`VERTER_ACCEPTANCE_PROVIDER=off`) is the sharpest comparison:
on corpus F it now produces **the same** carrier hover / definition / references
numbers as the editor-owned tier, differing only in completion. That is the
measurement that says the tier is contributing almost nothing on this workspace.

## Evidence

All numbers inline. Corpus F, debug `verter-lsp`, shared machine, 3 repeats per
probe:

| | `auto` before | `editor-tsserver` after 1–3 | `off` control after 1–3 |
| --- | --- | --- | --- |
| carrier hover | 0 TS / 0 native / 51 empty | 0 TS / 27 native / 9 empty | 0 TS / 27 native / 9 empty |
| carrier definition | 0 resolved / 33 empty | 6 resolved / 27 empty | 6 resolved / 27 empty |
| carrier references | 12 resolved / 24 empty | 33 resolved / 3 empty | 33 resolved / 3 empty |
| carrier completion | 15 resolved | 15 resolved | 0 resolved (15 unresolved) |
| carrier hover p50 | 5 ms | 6 ms | 6 ms |

## Why deferred

`SCOPE.md` permits shipping only benign fixes. Blocker A is a sync-policy change
on the editor-owned topology — awaiting IDE sync at `didOpen` there is exactly
the trade-off the deadline/sync workstream owns, and doing it here would collide.
Blocker B is not Verter's code at all: it is VS Code's syntax/semantic routing
reacting to tsserver project churn, and the only lever Verter has is to stop
producing churn, which means changing what the plugin advertises as project
membership — an architectural change to the tier.

## Proposed fix and falsifiable prediction

1. **Publish the opened carrier eagerly on this topology.** The tier's entire
   value is the plugin serving from the store; a store with no entry for the
   file under the cursor cannot serve. Await IDE sync for the opened carrier
   (only the opened one) before reporting the tier ready.
2. **Give tsserver a stable home for the carrier.** While the raw `.vue` belongs
   to no configured project, tsserver keeps creating and removing referenced
   projects around it and VS Code keeps routing to the syntax server. Advertising
   the carrier SOURCE identity in `editorOwnsMembership` mode (as the non-editor
   surface already does) is the candidate, but it interacts with VS Code owning
   the open raw text and needs its own design.

Falsifiable prediction: with (1), the plugin's `getExternalFiles` for the project
owning the opened carrier reports a non-zero companion count within a second of
open, and `carrier.hover.typescript` on corpus F at `editor-tsserver` goes from
**0 / 51** to non-zero. If it stays at zero after (1), the cause is (2) and this
document should be reopened against it.

Discriminating test: the acceptance lane on a project-references workspace at
`VERTER_ACCEPTANCE_PROVIDER=editor-tsserver`, before and after. A fix that does
not move `carrier.hover.typescript` off zero has not worked.

## Blast radius

**If fixed:** the editor-owned tier becomes a genuinely serving tier and is a
candidate for the automatic policy again — it is by far the fastest measured
option on corpus F (hover p50 6 ms, definition p50 3 ms, references p50 2 ms,
versus 68 ms / 2362 ms / 3 ms for the managed tier on the same workspace).

**If left alone:** the tier keeps behaving like `off` plus completion on
workspaces of this shape. Since the automatic policy no longer selects it, the
user-visible cost is bounded to operators who set `editor-tsserver` explicitly.

**Interaction:** the automatic policy now falls through to the managed tier,
whose own defects are recorded in
`managed-tsserver-hangs-and-answers-no-carrier-hover.md` and
`managed-tsserver-serves-a-workspace-with-verters-own-typescript.md`. On corpus F
today that fallthrough is SLOWER and loses carrier completion. The automatic
policy follows the ratified provisioning order deliberately, so that when the
managed tier's defects are fixed the workspace sees the fix instead of staying
parked on a tier that cannot serve. If the owner prefers the fast-but-silent
tier in the meantime, `verter.typeProvider = "editor-tsserver"` selects it, and
re-adding `"auto"` to `typeProviderRoutesEditorTsserver`
(`packages/vue-vscode/src/editorTsserverBootstrap.ts`) plus
`route_consumes_editor_tsserver_attestation` (`crates/verter_lsp/src/main.rs`)
restores the old default in two lines.
