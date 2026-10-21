# VSC1 evidence index

Selected case IDs: `VSC1-AC1`, `VSC1-AC2`, `VSC1-AC3`, `VSC1-AC-OWNER`, `VSC1-AC-BASIS`, `VSC1-AC-RESOURCE`, `VSC1-AC-EXPOSURE`.

Commands (run on the candidate; this file is not an execution transcript):

- `pnpm --filter verter-vscode test` (unit: `vitest run --dir src`, the e2e lib and acceptance unit suites)
- `pnpm --filter verter-vscode run prepare:e2e`, then `node out-test/e2e/runTests.js --fixture=single-project --only=lifecycle` from `packages/vue-vscode` (the real extension host; `lifecycle.test.ts` is a legacy suite and runs on every legacy fixture route)

Retired routes: the language client's own restart-on-close; failing the start attempt on a failed first start; the second `verter.typeProvider` restart listener; the start attempt without an owning lifetime.

AC1: three restarts and one killed server each leave exactly one Verter LSP under the extension host, with every replaced process gone and the unsaved edit's diagnostic count unchanged. A type-provider change restarts a running server once.

AC2: a server that cannot start, and one that keeps crashing, end after the bounded restarts in a stopped status that restarts on click, with an error notification and nothing left armed.

AC3: activation spawns nothing without a framework document and no setting change spawns a server; one in-flight start serves concurrent demand. Startup latency belongs to VSC8's bench run.

AC-OWNER: the restart supervisor recovers; the language client never restarts the server itself; `DesktopLanguageLifecycle` owns the epoch and the shown state.

AC-BASIS: a replaced client's reports, including its engine status, are stale and never shown as current.

AC-RESOURCE: process and start counts only; timing and memory rows are predeclared for VSC8 and unavailable here.

AC-EXPOSURE: no new operation.
