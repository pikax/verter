# IDE parity E2E contract

Every applicable behavioral test must pass on tsserver, managed tsgo, and shared
editor-owned tsgo. Compiling the harness alone does not satisfy the release gate.

## Enforced inventory

- Every parity suite file and literal test discovered from the authored TypeScript tree.
- Every declarative Vue/Svelte matrix case discovered from the matrix source.
- The 22-case framework contract remains a separate focused surface; it is not a
  substitute for the parity inventory.
- Run summaries attest the exact fixture, loaded compiled files, complete applicable ID
  inventory, failures, and pending IDs. Missing, duplicate, stale, pending, or unexpected
  evidence fails closed.
- A content-hash build manifest binds every authored suite source to the compiled JavaScript
  loaded by the extension host.

## Test-defect policy

- `failParityGap` never skips. It classifies a red as `TEST_DEFECT` or `PRODUCT_GAP` and throws.
- Framework-specific tests are registered only for their applicable fixture. They are not
  represented as N/A passes or artificial failures on the other framework.
- Vue and Svelte public contracts follow their frameworks: Vue uses its public instance;
  Svelte 5 uses official `ComponentProps` and callable-component exports.
- Negative type assertions use live diagnostics and active `@ts-expect-error` controls.
  Documentation-token `.contains()` checks are not type evidence.
- Navigation fails if generated carriers, including `Comp.d.vue.ts` or
  `Comp.d.svelte.ts`, escape to the editor.

## Required local gates

```sh
pnpm --filter verter-vscode exec tsc -p tsconfig.test.json --noEmit
pnpm --filter verter-vscode test:e2e:lib:unit
pnpm --filter verter-vscode test:e2e:dx:unit
```

Every standard live fixture runs on `tsserver` and `tsgo`; every configured-project fixture
also runs on `shared-tsgo`. The shared route must prove an actual editor-owned carrier result
and zero managed-fallback activation. Project-less fixtures cannot establish a project-bound
shared carrier and are excluded from that route by the canonical inventory.
A red test
is triaged as a fixture/test defect or a product gap; assertions are not weakened to make
the release green.
