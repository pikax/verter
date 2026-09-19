# DX0 evidence index

Selected case IDs: `DX0-AC1`, `DX0-AC2`, `DX0-AC3`, `DX0-ratification`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node tests/product-experience/DX0/verify.mjs`
- `node --test tests/product-experience/DX0/dx0.test.mjs`
- `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
- the remaining `docs-domain` final profile commands (see `catalogs/gate-profiles.toml`)

Deletion population this node: empty. No route is displaced or retired by DX0.

DX0.1 grounding: all five editor clients (VS Code, Neovim, Helix, Lapce, Zed) were inventoried from their shipped sources before this constitution proposed anything; no replacement is proposed. Playground gaps (tsgo project check, project-wide lint, component-meta, formatting, flow facts, native editor hover/diagnostics/rename, LSP-channel lint and component-usage diagnostics, TypeInfo queries, VS Code source-map commands) are catalogued with promotion blocked. NativeOnly editor operations are catalogued as gaps on their own rows; their browser TS-worker analogues are separate Portable rows (playground.hover/completion/diagnostics/rename), and the playground.hover, playground.completion and playground.diagnostics rows record the live unlabelled merges (hover contents, completion suggestions, and the Diagnostics tab plus its badge mixing compiler+lint with TypeScript) as implicit-comparison gaps with promotion blocked.

AC-BASIS is bound to DX2/DX8 as downstream runtime-test owners; AC-RESOURCE is not applicable (no hot paths, no UI, 0 production LOC); AC-EXPOSURE registration is owned by DX1.

This file does not claim the cases executed. Copying the charter here is not evidence.
