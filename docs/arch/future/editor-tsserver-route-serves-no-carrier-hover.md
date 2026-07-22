# The `editor-tsserver` route serves no carrier hover or definition on real projects

Recorded by the VS Code end-to-end acceptance lane, 2026-07-22.

## Symptom

Open a real Vue project in real VS Code with Verter at its **default settings**
(`verter.typeProvider = "auto"`). The status bar reports a connected TypeScript
engine. Hovering anything in a `.vue` file shows **nothing at all** — not a wrong
type, not a Verter-native summary, nothing. Go-to-definition does nothing.

Measured on a private corpus (a pnpm monorepo, ~77 SFCs, four configured
TypeScript projects, every carrier explicitly covered by its project's
`include`), driving a real extension host with the built extension and a debug
`verter-lsp`:

| route (`verter.typeProvider`) | resolved provider | carrier hovers carrying real TypeScript | carrier definitions |
| --- | --- | --- | --- |
| `auto` (default) | `editor-tsserver` | **0 / 51** (51 empty) | **0 / 33** (33 empty) |
| `tsserver` | `editor-tsserver` | **0 / 51** (51 empty) | **0 / 33** (33 empty) |
| `tsgo` | `tsgo` | **20 / 51** | 15 / 33 resolved, 18 empty |
| `off` (control) | `none` | 0 / 51 (27 Verter-native, correct) | 0 / 33 |

On the same runs, plain `.ts` files in the same editor session answered normally
(12 / 39 hovers carrying quickinfo on the `auto` run, p50 6 ms), so the editor,
the host and the machine were all healthy. The engine was attested: the log
records `[editor-tsserver] armed: pid=… projects=[4 projects]` and
`Type provider status: editor-tsserver (attested editor tsserver process … across
4 project(s))`.

Two independent aggravating facts:

1. **`auto` is the default**, so this is what a user gets out of the box.
2. **`tsserver` does not escape it.** `typeProviderRoutesEditorTsserver`
   (`packages/vue-vscode/src/editorTsserverBootstrap.ts:37-39`) routes `auto`,
   `shared-tsgo` AND `tsserver` to the editor-owned tier, so an operator who
   explicitly selects `tsserver` still lands on the same non-answering route.
   Only `tsgo`, `extension` and `off` leave it.

With the type provider disabled entirely (`off`), the same 51 probes produced 27
**Verter-native** hovers. So connecting the engine did not merely fail to add
TypeScript answers — it **removed** the answers the user would otherwise have
seen. The connected state is strictly worse than the disconnected one.

## Mechanism

On the `EditorTsserver` topology the LSP deliberately stands down and lets
Verter's TypeScript plugin, running inside VS Code's own tsserver, own carrier
source features:

- `crates/verter_lsp/src/server/mod.rs:1104-1114` — `editor_owns_carrier_source_features()`
  is `true` whenever `type_provider_kind == TypeProviderKind::EditorTsserver`.
  Its doc comment states the intent: *"The attested editor tsserver plugin owns
  carrier hover, navigation, and rename directly in VS Code. The LSP has no local
  TypeProvider in this topology and must not register a competing partial answer
  for the same request."*
- `crates/verter_lsp/src/server/nav_features.rs:150-166` — `handle_hover` returns
  **only** the CSS leg and exits.
- `crates/verter_lsp/src/server/nav_features_navigation.rs:83`, `:674`, `:943`,
  `:990` — the same stand-down for definition, references and rename.

The stand-down is unconditional on the route. It is not conditional on the plugin
actually being able to answer for the file under the cursor. When the plugin does
not answer, nobody does, and the request completes successfully with no content —
so it presents as "nothing to show" rather than as a failure. The LSP log makes
the shape unmistakable:

```
INFO verter_lsp::server::nav_features: hover ENTER file:///…/App.vue at 34:39
INFO verter_lsp::server::handler_guard: HANDLER_EXIT hover active=0 elapsed=171.9µs
```

~200 µs, no engine round-trip, no result — repeated for all 51 probes.

Completion is NOT gated by `editor_owns_carrier_source_features()`, so it is still
served by the LSP on this route (`ensure_current_file_synced: flushing IDE sync`
appears in the same log). The route therefore answers completion while returning
nothing for hover and definition, which is why the failure reads as "IntelliSense
is partly there" rather than "the route is not serving".

The arming gate accepts the route on evidence that does not imply serving.
`establishEditorTsserverPlugin` (`packages/vue-vscode/src/extension.ts:1248-1281`)
accepts the attestation when `receiptIncludesConfiguredProject(receipt, workspaceRoot)`
holds — i.e. the editor's tsserver reported *a* configured project under the
workspace root. It bootstraps by opening one arbitrary carrier
(`prepareEditorTsserverConfiguredProject`, `extension.ts:1300-1314`) and never
verifies that a carrier source feature actually returns anything. A receipt
listing four projects is therefore sufficient to disable the LSP's own hover path
for every carrier in the workspace.

## Why this was not caught

`packages/vue-vscode/e2e/suite/editor-owned-project.test.ts` covers exactly this
route (`{ fixture: "editor-owned-project", typeProvider: "tsserver" }`,
`e2e/lib/routeInventory.ts:32-35`) and asserts a carrier hover exposes the real
component surface. It **passes**, 4/4, on the same machine, same VS Code build,
same LSP binary, verified during this investigation.

So the hand-off works for that fixture — a single project, one tsconfig, a flat
`npm install` — and does not work for the measured real project. The fixture's
shape is the thing that differs, which is why a green fixture suite coexisted
with a completely non-answering editor.

## Reproduction

Requires a project whose shape the vendored fixtures do not have. No synthetic
equivalent exists in-tree yet; producing one is the first step of any fix.

1. Build the extension and LSP: `pnpm --filter verter-vscode run prepare:e2e`.
2. Point the committed acceptance lane at a Vue project that is a **pnpm
   workspace with multiple configured TypeScript projects** (the measured one had
   four, including an inferred project), where each project's `include`
   explicitly covers `*.vue`:

   ```
   VERTER_ACCEPTANCE_WORKSPACE=<project> \
   VERTER_ACCEPTANCE_LABEL=<letter> \
   VERTER_ACCEPTANCE_PROVIDER=auto \
   node packages/vue-vscode/out-test/e2e/acceptance/launch.js
   ```

3. The lane fails `a workspace with a connected engine answers hovers with real
   TypeScript` and the receipt shows `carrier.hover.typescript = 0` with
   `provider.kind = "editor-tsserver"`.
4. Re-run with `VERTER_ACCEPTANCE_PROVIDER=tsgo`. The lane passes and
   `carrier.hover.typescript > 0`. That pair is the discriminator: same host,
   same project, same binary, only the route differs.

The `off` control run (`VERTER_ACCEPTANCE_PROVIDER=off`) is worth running too —
it shows the Verter-native hovers the `auto` route suppresses.

## Evidence

All numbers inline above. Additional detail from the `auto` run on the measured
corpus, debug profile, shared machine:

- `openToShownMs` 50; `timeToFirstTypeScriptHoverMs` **null** after 90 s of
  polling at a member position that the `tsgo` route answers in 2054 ms.
- `carrier.hover` p50 4 ms / p95 18 ms — fast *because* it is empty.
- `carrier.references` 24 of 36 empty; `carrier.completion` 15/15 resolved.
- `typescript.hover` (native TypeScript yardstick, same session) p50 6 ms,
  12 of 39 carrying quickinfo.
- Enabling all extensions (`VERTER_ACCEPTANCE_KEEP_EXTENSIONS=1`) changes
  nothing: still 0 / 51. It is not a `--disable-extensions` artifact.

Caveat on the `tsgo` column: the acceptance launcher supplies `VERTER_TSGO_BIN`
from the repository's pinned `@typescript/typescript-<platform>` package, so that
run used an engine a user of this project would have to install. It demonstrates
that the engine and the carrier projection are capable of answering these exact
probes — which is what isolates the defect to the route — not that switching the
setting is a fix a user can apply unaided.

## Why deferred

`SCOPE.md` permits shipping only **benign** fixes: small, self-contained, no new
abstraction or flag, no behaviour change beyond removing the defect. Every
candidate fix here fails that bar:

- Making the LSP serve carrier hover on this route contradicts the stated design
  ("must not register a competing partial answer") and would introduce exactly
  the duplicate-provider condition the comment was written to prevent.
- Removing `"tsserver"` from `typeProviderRoutesEditorTsserver` would break the
  ratified route inventory (`e2e/lib/routeInventory.ts:32-35`) and its passing
  fixture suite.
- Strengthening the arming predicate is the right fix, but it changes tier
  acceptance semantics, which `engine-provisioning-spec.md` owns.

This is a routing/provisioning decision, not a local defect, so it is recorded
rather than patched by the acceptance workstream.

## Proposed fix and falsifiable prediction

**Make the editor-owned tier prove it can serve before the LSP stands down.**
`establishEditorTsserverPlugin` already opens a bootstrap carrier to force a
configured project; have it also request one carrier source feature at a known
position in that carrier and accept the tier only if a non-empty result comes
back. On failure, return `NO_EDITOR_TSSERVER` and let `auto` fall through to the
next tier exactly as it does when the editor's TypeScript extension is absent.

The change is confined to the acceptance predicate passed to
`attestEditorTsserverBootstrap` (`extension.ts:1265-1277`); the Rust stand-down
stays untouched, because on a tier that genuinely serves it is correct.

Falsifiable prediction: with that predicate in place, the measured corpus resolves
to `tsgo` (or `tsserver`) under `auto`, and the lane's
`carrier.hover.typescript` goes from **0 / 51 to ≥ 20 / 51**, matching the
explicit-`tsgo` run, with `timeToFirstTypeScriptHoverMs` landing near 2000 ms
instead of never. The `editor-owned-project@tsserver` fixture must stay green,
proving the predicate does not reject a tier that does serve.

Discriminating test: the lane itself, run twice on the same project with
`VERTER_ACCEPTANCE_PROVIDER=auto` before and after. A fix that does not move
`carrier.hover.typescript` off zero has not worked.

A second, independent item worth fixing in the same area: the arming gate should
distinguish "the plugin serves this workspace" from "the editor reported a
configured project", and an operator who explicitly selects `tsserver` should get
an engine that answers — today both `auto` and `tsserver` land on the same
non-answering tier with no way to tell from the UI.

## Blast radius

**If fixed:** `auto` starts resolving to a different tier on workspaces where the
editor plugin cannot serve. Those workspaces gain working hover and definition
and lose the editor-owned topology's benefits (one tsserver process, editor-owned
project semantics). Workspaces where the plugin does serve are unaffected because
the predicate passes. The `editor-owned-project` fixture is the regression guard.
A too-strict predicate would push working setups off the editor-owned tier — the
fixture staying green is what bounds that.

**If left alone:** the default configuration shows no TypeScript IntelliSense in
`.vue` files on projects of this shape, with a status bar that says an engine is
connected, and no setting other than the undocumented `tsgo` recovers it. This is
the exact symptom that opened this effort ("4 hovers entered, 0 answered"), so
leaving it means the headline complaint is unresolved regardless of how much
latency work lands elsewhere.

**Interaction:** `docs/arch/future/` neighbours covering carrier→TSX position
mapping affect the `tsgo` route's remaining empties (18 / 33 definitions empty
even when the route works). They are a different, additive defect — fixing this
routing issue exposes them rather than masking them.
