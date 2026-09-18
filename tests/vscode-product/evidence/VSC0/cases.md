# VSC0 evidence index

Selected case IDs: `VSC0-AC1`, `VSC0-AC2`, `VSC0-AC3`, `VSC0-ratification`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node tests/vscode-product/VSC0/verify.mjs`
- `node --test tests/vscode-product/VSC0/vsc0.test.mjs`
- `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
- the remaining `docs-domain` final profile commands (see `catalogs/gate-profiles.toml`)

Deletion population this node: empty. No route is displaced or retired by VSC0.

VSC0.1 grounding: the extension (`packages/vue-vscode`) was inventoried from its live manifest and source — 16 contributed commands (all registered in non-test source; `verter.openRouteComponent` plus four `verter._*` ids are registered-but-internal, the latter gated on `VERTER_E2E_TEST`), 4 views, 32 settings, 5 languages, 8 grammars, 9 colors, 11 activation events, the 7-mode provider enum, and the activation/recovery lifecycle (activation gate, deferred features on `$/verter/ready`, one `StartAttemptScope` per start attempt, heartbeat watchdog, restart supervisor, attempt-owned `verter-mcp` child). Two open gaps are recorded, not hidden: `languages/vue-postcss-language-configuration.json` and `languages/vue-sugarss-language-configuration.json` are referenced by `contributes.languages` but missing on disk (repair owned by VSC7).

VSC0.2 grounding: all 47 production modules are classified from live imports — desktop 20 (Node builtins, `process.*`, `require()`, or the runtime `vscode-languageclient/node` import in `extension.ts`), presentation 6 (vscode API only), pure 21 (no host imports); 46 spec files plus `extensionTsService.testUtils.ts` are counted as evidence, never classified. The eight view/decoration providers sit in desktop today because their `LanguageClient` import is written as a value import; converting those to `import type` (and the two `path`-only tree providers) belongs to the VSW1 browser cutover, not a silent reclassification here.

AC-BASIS is bound to VSC1 (activation/recovery/teardown runtime tests) and VSW1 (load shared code with Node globals absent) as downstream runtime-test owners; the import boundary is the mechanical proxy this harness enforces. AC-RESOURCE is not applicable (no hot paths, no UI, 0 production LOC). AC-EXPOSURE registration is owned by DX1; this node promotes no operation.

This file does not claim the cases executed. Copying the charter here is not evidence.
