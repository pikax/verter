//! Server-level integration tests with real type providers (tsserver + TSGO).
//!
//! Each test uses `real_provider_test!` to generate two variants — one per provider.
//! Tests skip gracefully when binaries are not found.
//!
//! # Known Limitations & Canary Failures
//!
//! ## Harness vs. VS Code behavioral differences
//!
//! The test harness calls LSP methods directly (e.g. `server.goto_definition()`), which
//! returns the **raw LSP response** — a single hop. VS Code's `executeDefinitionProvider`
//! follows through imports automatically (multi-hop). This means:
//!
//! - **Component tag go-to-def** (`<MyComp` +1): The LSP resolves to the import binding
//!   in the same file, while VS Code follows through to the source `.vue` file. Tests
//!   asserting "should go to MyComp.vue" may fail at server level when the intermediate
//!   import is in the same file — this is expected and not a bug.
//!
//! - **Import specifier go-to-def**: Works correctly — the provider resolves the module
//!   specifier directly to the target file (single hop).
//!
//! ## TSGO-specific limitations (canary failures)
//!
//! These tests use identical assertions for both providers. Failures flag real TSGO
//! limitations that should be tracked and resolved:
//!
//! - **`definition_path_aliases`** (both providers): `import MyComp from '@/components/MyComp.vue'`
//!   go-to-def on the import binding resolves to the same file instead of following through the
//!   `@/` path alias to `MyComp.vue`. The `IdeProjectConfig` tsconfig path in the harness
//!   points at `{fixture}/tsconfig.json` but the path-aliases fixture uses `paths: { "@/*": ["./src/*"] }`
//!   which may not be picked up without a full project sync. Affects both providers equally.
//!
//! - **`rename_single_project_tsgo`**: Cross-file prop rename (`foo="literal"` → "fooRenamed")
//!   returns edits in 1 file (only App.vue) instead of 2 files (App.vue + MyComp.vue).
//!   tsserver correctly propagates the rename to the child component's `defineProps` type.
//!   TSGO does not yet support cross-file rename propagation for Vue prop attributes.
//!
//! - **`completion_secondary_files_tsgo`**: JS SFC (`<script setup>` without `lang="ts"`)
//!   member access on a JSDoc-typed variable (`state.label` where `state` has
//!   `@type {{ label: string, done: boolean }}`) returns component-scope completions
//!   instead of member completions. TSGO does not yet resolve JSDoc `@type` annotations
//!   for member access in JavaScript Vue SFCs.
//!
//! ## Fixture-specific notes
//!
//! - **no-config / single-file**: These fixtures have no `tsconfig.json`. The harness
//!   `install_resolver_snapshot` points at a non-existent `{root}/tsconfig.json`, but
//!   the type provider still works because it falls back to default config. Hover and
//!   completions work; cross-file features (definition, rename) are limited.
//!
//! - **monorepo**: The harness uses the monorepo root as the workspace root with a
//!   single `IdeProjectConfig`. The actual VS Code workspace would have multiple roots
//!   (`packages/app`, `packages/shared`). Cross-package go-to-def works via the root
//!   tsconfig's `references` field, but multi-root features are not fully exercised.

mod completion;
mod definition;
mod document_symbols;
mod hover;
mod multi_fixture;
mod references;
mod rename;
