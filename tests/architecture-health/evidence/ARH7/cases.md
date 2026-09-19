# ARH7 evidence cases

The sole owning interface is `node tests/architecture-health/ARH7/verify.mjs` (verify) plus `node --test tests/architecture-health/ARH7/arh7.test.mjs` (dirty twins), wired into `test:scripts` and the CI `architecture-health` lane. The verifier joins the shipped ARH2 predecessor (imported `validate()`, never re-implemented) and the live VS Code activation composition. No DAG copy is read from this tree.

`packages/vue-vscode/src/extension.ts` is the composition root (`activate`/`deactivate`) over `createActivationRoot`. `ActivationSession` owns MCP handles and the activation lifetime bag. `StartAttemptScope` is the shared lifetime type for activation and language-server start attempts. The module-level client/MCP locator and the in-file `StartAttemptScope` class are deleted.

## ARH7-ratification (accept)

Clean products validate. Manifest cases, products and verify/test commands match the verifier. The candidate is a 40-hex commit; `--provenance` (full-history architecture-health lane) proves it is an ancestor of HEAD. Shallow `test:scripts` checkouts skip `--provenance` when the pin object is absent. `test:scripts` and the CI architecture-health job include this node; the arch filter includes `packages/vue-vscode/**`.

## ARH7-cutover (reject) — AC1

- `locator-still-present`: displaced module bindings are exactly `getClient`, `stopHeartbeat`, `activationContext`, `currentMcpEndpoint`, `retryMcpLifecycleSync`.
- `start-attempt-still-in-extension`: `class StartAttemptScope` must not remain in `extension.ts`.
- `owner-path-missing` / `owner-export-missing`: CUT-1 survives on `createActivationRoot`; CUT-2/CUT-3 on `StartAttemptScope`.
- `activate-extension-still-uses-context-subscriptions`: `activateExtension` registers on the session lifetime.
- `forbidden-service-locator`: no generic container/mega-context replacement.

## ARH7-authority (reject) — AC2

Reload or repeated activation duplicating listeners is rejected by `activationSession.spec.ts` (`reload or repeated activation does not duplicate registrations`), with the existing activation gate and start-attempt lifetime tests retained. Dirty twin drops the session spec.

## ARH7-work (reject) — AC3

Fresh/incremental, edit/revert, stale/partial and scheduling order are not applicable (no state/query/map mutation). Cancellation binds the existing start-attempt failure-disposal witness. Dirty twins empty cancellation evidence or invent a concern.

## ARH7-delivery (reject) — AC4

`examples/reference` must not name the displaced locator. VSC0 start-attempt-scope evidence and the desktop/web shared boundary must name the new lifetime modules. AC4 rationale records NativeOnly `vscode.extension` and the DOC1 non-duplication.

## ARH7-cost (reject) — AC5

No committed wall-clock/RSS/speedup. Required work is not removed; the locator is retired. Dirty twin plants `wallNs`.
