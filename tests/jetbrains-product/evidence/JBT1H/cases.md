# JBT1H evidence index

Selected case IDs: `JBT1H-AC1`, `JBT1H-AC2`, `JBT1H-AC3`, `JBT1H-AC-OWNER`,
`JBT1H-AC-BASIS`, `JBT1H-AC-RESOURCE`, `JBT1H-AC-EXPOSURE`.

Commands (run on the candidate; do not treat this file as an execution
transcript):

- `pnpm --filter @verter/dx-harness exec vitest --run test/jetbrainsHarness.test.ts`
  — AC1/AC2/AC3, fixture/workflow pin, receipt-basis, labelled-unknown metrics
  (no JVM).
- `pnpm --filter @verter/dx-harness test:unit` — typecheck plus the hermetic
  suite that includes the harness tests.
- `extensions/jetbrains/gradlew test` — Platform test-framework capture hooks
  against the pinned WebStorm SDK (`RealIdeCaptureTest`).
- `node scripts/jetbrains-gate.mjs` — jetbrains-product gate (Gradle `test`
  already includes the new JVM capture tests).

JBT1H.1 grounding: the pinned IDE is driven through the IntelliJ Platform
test-framework already on the JBT1 Gradle build (not a mock LSP client). Each
capture records IDE product/build, plugin version, and engine identity
(unknown on the skeleton, with reason).

JBT1H.2 grounding: official and Verter sides run the same JBT0 workflow ids;
UI application/paint is a measured EDT cycle distinct from RPC duration.
Verter skeleton workflows are `unsupported`; official semantic workflows on
the platform-only test IDE are `pending`. Completeness states stay distinct.

JBT1H.3 grounding: the process-tree snapshot must include the TypeScript
provider for a claimed retained-memory number; exclusion/missing/out-of-tree
pids invalidate the memory row. Unavailable metrics are `{status:unknown}`.

AC-OWNER: one final owner (`expansion.jetbrains-product`). The JBT0
adapterState row for `packages/dx-harness/jetbrains` moves from `absent` to
`harness-present`. Retirement: this harness is consumed by JBT9/JBT10 and is
not the JBT1 skeleton scheduled for deletion at JBT2.

AC-BASIS: incremental vs fresh compare only on the same ProductReceiptBasis.
Installed-product pending-capture pins are not filled from the test IDE.

AC-RESOURCE: WSP equal-work corpus pin plus the predeclared metric set;
no superiority claim.

AC-EXPOSURE: comparison operations registered NativeOnly, promotion blocked
on DX1 executable exposure.

Deletion population this node: empty. Anecdotal comparison is replaced by
the recorder; raw failed runs are retained on the pair record.

This file does not claim the cases executed. Copying the charter here is not
evidence.
