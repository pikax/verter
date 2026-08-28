# Retiring `@verter/core`

`@verter/core` is not part of Verter's current compiler architecture or
release surface. It has been removed rather than carried as a misleading
second compiler package.

## Decision basis

The retirement audit used the committed integration tree at
`978c3ea1019a5977749fdf85c5ccd7efd608b8e5` as its baseline.

At that baseline:

- no production workspace package declared `@verter/core` as a dependency;
- the only root-level operational reference was the watch-script filter;
- the package contained 365 tracked files (1,259,344 bytes), implementing an
  old TypeScript parser/transformer superseded by the Rust compiler;
- IDE source generation and runtime code generation were already owned by
  `verter_compiler`, with host/session queries owned by the Rust semantic
  dispatch;
- public guides that described `@verter/core` as the LSP's TSX producer were
  therefore incorrect.

There is no compatibility package, deprecation stub, or publication alias.
Keeping one would imply a supported compiler API and preserve a false dual-
engine architecture.

## Test preservation

Seven current Svelte client-runtime specifications and their 36 emitted
fixtures had been stored under `packages/core/test` even though they did not
import or execute the old compiler. They now live in the private
`packages/svelte-runtime-tests` workspace package. The package remains
test-only and is not published.

The Rust Svelte emitter tests continue to compare emitted modules with those
fixtures. Moving the tests changes ownership and discovery only; it does not
weaken their behavioral assertions.

## Verification

The retirement boundary was checked through the current owners rather than
through the deleted package:

- `cargo test -p verter_compiler --lib`: 5,515 passed, 4 intentional generator
  ignores, 0 failed;
- projection stack-safety module: 11 passed, including the 2 MiB-stack
  subprocess cases, distinct connected-work/query-depth partials, recursive
  carriers, unresolved carriers, diagnostic deduplication, and recomputation;
- Svelte runtime package: 7 files and 35 behavioral tests passed;
- `@verter/types`: 16 files and 612 typed tests passed with no type errors;
- `@verter/typescript-plugin`: 15 files and 269 tests passed;
- warning-denied all-target Clippy passed for `verter_compiler` and
  `verter_session`; `cargo fmt --all --check` passed;
- the integration-test layout guard and dry-run pack verification for every
  publishable package passed;
- the frozen offline pnpm install and TypeScript workspace build passed;
- VitePress built every page with strict dead-link checking. The build exposed
  and prompted repair of pre-existing malformed inline code and stale source
  links instead of weakening the documentation gate;
- all 3,914 existing tracked or new JSON, JSONC, and YAML files parsed under
  their proper syntax.

No remaining production source, workspace configuration, workflow, or package
manifest references `@verter/core`, `packages/core`, or the retired v5-process
tracker. Historical references in this decision record and the release audit
are deliberate.

## Retired parity tracker

The old v5-process parity manifest contained 769 rows (681 specification
rows and 88 fixture rows) pointing at 62 Rust test names. It did not execute
those tests or compare behavior; its Rust checks asserted only that rows had
non-empty names and the string status `ported`.

Its Node validator was already non-executable: it hard-coded the nonexistent
paths `crates/verter_core/tests/parity/v5_process_manifest.toml` and
`crates/verter_core/src`. The manifest and validator have therefore been
removed. This is not a test waiver. The real Rust compiler behavior tests
remain in their owning modules and are run directly by Cargo.

## Current ownership

| Concern | Owner |
|---|---|
| Vue/Svelte parsing and lowering | Rust parser/compiler crates |
| IDE-oriented TypeScript/JS source generation | `verter_compiler` `CompileTarget::IDE` |
| Runtime module generation | `verter_compiler` runtime targets |
| Cross-file semantic queries | `ProjectSemanticDispatch` and `SemanticGraphStore` |
| Native and WASM JavaScript APIs | `@verter/native` and `@verter/wasm` |
| Bundler integration | `@verter/unplugin` |
| Editor TypeScript integration | `@verter/typescript-plugin`, language-shared, and VS Code client packages |

Any future JavaScript API must wrap an owned Rust compiler surface and carry
its real support and diagnostic contract. It must not reintroduce a separate
TypeScript compiler implementation.
