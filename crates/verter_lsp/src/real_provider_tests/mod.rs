//! Server-level integration tests with real type providers (tsserver + TSGO).
//!
//! Each test uses `real_provider_test!` to generate two variants — one per provider.
//! Tests skip gracefully when binaries are not found.
//!
//! # Known Limitations & Canary Failures
//!
//! ## TSGO-specific limitations (canary tests)
//!
//! Canary tests use `canary_assert_known_limitation!` to assert the **known-broken behavior
//! still holds**. When a limitation is fixed (the broken condition becomes false), the canary
//! **panics** — signaling the fix should be promoted to a real `assert!`.
//!
//! Cross-file Vue-prop rename reaches parity on BOTH providers: tgo's native rename does not
//! enumerate the child component's synthesized `{carrier}.ts` `defineProps` surface, so Verter
//! synthesizes that child-declaration leg provider-agnostically and maps it back onto the child
//! `.vue` through the generation-pinned snapshot. The `rename_*` lanes therefore assert ≥2-file
//! edits for both providers (no rename canary remains).
//!
//! ## Fixture-specific notes
//!
//! - **no-config / single-file**: These fixtures have no `tsconfig.json`. The harness
//!   builds a `ProjectRegistry` that finds no tsconfigs, so path alias resolution is
//!   unavailable. The type provider still works because it falls back to default config.
//!   Hover and completions work; cross-file features (definition, rename) are limited.
//!
//! - **monorepo**: The harness uses the monorepo root as the workspace root with a
//!   single `IdeProjectConfig`. The actual VS Code workspace would have multiple roots
//!   (`packages/app`, `packages/shared`). Cross-package go-to-def works via the root
//!   tsconfig's `references` field, but multi-root features are not fully exercised.

mod code_action;
mod completion;
mod completion_detail;
mod definition;
mod diagnostics;
mod document_symbols;
mod external_ts_baseline;
mod hover;
mod import_matrix;
mod multi_fixture;
mod references;
mod rename;
mod signature_help;
