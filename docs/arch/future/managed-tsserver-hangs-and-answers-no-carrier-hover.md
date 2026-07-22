# The managed-tsserver route hangs and answers no carrier hover on a large project

Recorded by the VS Code end-to-end acceptance lane, 2026-07-22.

Distinct from `editor-tsserver-route-serves-no-carrier-hover.md`: that one is a
routing/stand-down defect on the editor-owned tier. This one is the **managed
tsserver** tier, where Verter does drive the engine itself, and the engine wedges.

## Symptom

Open a large real Vue project (private corpus: ~245 SFCs, ~408 `.ts`, ten
tsconfigs, pnpm workspace) in real VS Code at default settings. The editor-owned
tier correctly declines, `auto` falls back to the workspace tsserver, the status
bar reports a connected engine with an honest reason — and then `.vue` hover
returns **nothing**, slowly.

Measured over 3 repeats per probe, debug `verter-lsp`, shared machine:

| operation | carrier (`.vue`) | native TypeScript yardstick (`.ts`, same session) | ratio (p50) |
| --- | --- | --- | --- |
| hover | **0 / 18 carrying TypeScript**; 6 Verter-native, **12 empty**; p50 **1365 ms** | 9 / 18 carrying TypeScript; p50 1366 ms | 1.00 |
| definition | **12 / 12 empty**; p50 **2364 ms**, p95 **11 821 ms** | 12 / 12 resolved; p50 10 021 ms | 0.24 |
| completion | 6 / 6 unresolved; p50 **11 053 ms** | 6 / 6 resolved; p50 5864 ms | 1.88 |
| references | 6 / 12 empty; p50 **2863 ms** | 12 / 12 resolved; p50 2866 ms | 1.00 |

`timeToFirstTypeScriptHoverMs` is **null** — no correct hover ever arrived, after
90 s of polling.

The absolute numbers are the story as much as the ratios: an **11-second
completion** and a **2.4-second go-to-definition that returns nothing** are not
latency regressions, they are an unusable editor. The native yardstick is slow on
this project too (10 s definitions), so part of this is project size — but the
carrier side returns *nothing* for that cost, and the native side returns answers.

## Mechanism

The LSP log shows the engine wedging repeatedly and being restarted:

```
WARN  [editor-tsserver] establish failed; continuing to managed fallback:
      Error: editor tsserver plugin attestation timed out: receipt not written
INFO  Type provider status: tsserver (no supported tsgo engine is available for <ws>,
      falling back to the workspace tsserver <ws>/node_modules/typescript/lib/tsserver.js)
ERROR verter_type_runtime::tsserver::ipc: tsserver appears hung (3 consecutive timeouts) — triggering restart
WARN  verter_type_runtime::resilient: tsserver crash detected - initiating restart sequence
ERROR verter_type_runtime::tsserver::ipc: tsserver appears hung (4 consecutive timeouts) — triggering restart
ERROR verter_type_runtime::tsserver::ipc: tsserver appears hung (5 consecutive timeouts) — triggering restart
ERROR verter_type_runtime::tsserver::ipc: tsserver appears hung (6 consecutive timeouts) — triggering restart
WARN  verter_type_runtime::tsserver::ipc: tsserver quickinfo error for <ws>/…/App.vue.tsx:
      request 'quickinfo' timed out after 1.3489331s
WARN  verter_type_runtime::tsserver::ipc: tsserver quickinfo error for <ws>/…/App.vue.tsx:
      request 'quickinfo' timed out after 50ms
```

Two things stand out and both are consistent with the deadline analysis already
recorded for this effort:

1. **`quickinfo` is cancelled at 1.35 s, then at 50 ms.** The deadline shrinks as
   the restart cycle progresses, so each successive attempt has less time than the
   last, on an engine that has just been restarted and is therefore colder. A cold
   `quickinfo` on a project this size does not complete in 50 ms, so the route
   cannot converge: every cancellation makes the next attempt more likely to be
   cancelled.
2. **The restart counter reaches 6 and keeps climbing** without the route ever
   producing a carrier answer, while the editor's own tsserver — same machine,
   same project, same moment — answers `.ts` hovers.

Producers: `crates/verter_type_runtime/src/tsserver/ipc.rs` (hung detection,
restart trigger, `quickinfo` timeout) and `crates/verter_type_runtime/src/resilient.rs`
(restart sequence).

## Reproduction

Needs a project of this scale; no in-tree fixture is large enough to wedge
tsserver, and building one is the first step of any fix.

1. `pnpm --filter verter-vscode run prepare:e2e`
2. Point the committed acceptance lane at a Vue workspace of roughly this shape
   (hundreds of SFCs, hundreds of `.ts`, several tsconfigs, pnpm workspace), with
   no tsgo engine installed so `auto` falls back to the workspace tsserver:

   ```
   VERTER_ACCEPTANCE_WORKSPACE=<project> VERTER_ACCEPTANCE_LABEL=<letter> \
   VERTER_ACCEPTANCE_PROVIDER=auto \
   node packages/vue-vscode/out-test/e2e/acceptance/launch.js
   ```

3. The receipt shows `carrier.hover.typescript = 0` with
   `provider.kind = "tsserver"`, and the log carries the `appears hung` sequence.
4. Re-run with `VERTER_ACCEPTANCE_PROVIDER=tsgo` (the launcher supplies the
   repository's pinned tsgo binary). Carrier hovers go to **28 / 33 carrying
   TypeScript**, first correct hover at **1953 ms**, and no wedge appears.

## Evidence

Inline above. The `tsgo` comparison on the same project, same session shape:

| operation | carrier @ `auto` (tsserver) | carrier @ `tsgo` |
| --- | --- | --- |
| hover | 0 / 18 TypeScript, 12 empty, p50 1365 ms | **28 / 33 TypeScript, 0 empty**, p50 16 ms, warm p50 11 ms |
| definition | 12 / 12 empty, p50 2364 ms | 24 / 24 resolved, p50 7 ms |
| completion | 6 unresolved, p50 11 053 ms | 9 / 12 resolved, p50 102 ms |
| references | 6 / 12 empty, p50 2863 ms | 21 / 21 resolved, p50 33 ms |

Same project, same machine, same binary, minutes apart. The route is the variable.

Caveat on the `tsgo` column: the acceptance launcher supplies
`VERTER_TSGO_BIN` from the repository's pinned `@typescript/typescript-<platform>`
package. A user of this project would not have that binary unless they installed
it, which is exactly why `auto` reported "no supported tsgo engine is available".
The comparison shows the engine is capable, not that the user can reach it today.

## Why deferred

`SCOPE.md` restricts this workstream to benign fixes. Correcting this means
changing deadline policy on a cold engine and the interaction between
cancellation and restart — precisely the "distinguish cold from warm deadlines"
work that `deadline-and-file-set-spec.md` already owns and that another
workstream is implementing. Landing a competing change here would collide with it.

## Proposed fix and falsifiable prediction

Two independent changes, in the owning workstreams:

1. **Do not shrink the deadline across a restart.** A `quickinfo` deadline of
   50 ms on a just-restarted engine cannot succeed; the cold path needs a cold
   budget. This is the deadline spec's cold/warm split.
2. **A restart cycle that never produces an answer should be surfaced, not
   retried silently.** After N restarts with zero successful carrier requests,
   the status should degrade to something the user can act on — the same honest
   `none`-with-a-reason treatment the no-tsconfig case already gets — rather than
   continuing to report a connected engine that answers nothing.

Falsifiable prediction: with the cold/warm deadline split, this project's
`carrier.hover.typescript` goes from **0 / 18** to non-zero and
`timeToFirstTypeScriptHoverMs` becomes a number, at a first-hover cost bounded by
the engine's genuine cold `quickinfo` time (seconds, not never). If hover counts
stay at zero after the deadline work lands, the cause is not the deadline and this
document should be reopened.

Discriminating test: the acceptance lane on this corpus at `auto`, before and
after. A fix that leaves `carrier.hover.typescript` at zero has not worked.

## Blast radius

**If fixed:** longer cold requests on large projects — the user waits instead of
getting nothing. That is the intended trade and matches "never fail closed on work
that would have succeeded". Small projects are unaffected because they never hit
the cold budget.

**If left alone:** on projects of this size the default configuration shows no
TypeScript IntelliSense in `.vue` files, spends 2–11 seconds per interaction doing
it, and reports a connected engine throughout. Users will read this as Verter
being broken and slow simultaneously, which is the worst possible combination for
adoption.

**Interaction:** this compounds with the editor-tsserver stand-down defect. A
project that attests the editor tier gets silence with no engine work at all; a
project that falls through to managed tsserver gets silence with a great deal of
engine work. Both present identically to the user.
