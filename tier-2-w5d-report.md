# Tier 2 W5d worker report

## Scope

Split `crates/verter_session/src/resolver_core/component_meta.rs`
(3948 LOC) per `docs/arch/debt-closure/13-god-module-split-audit/component_meta.md`.

## Inputs

- Base commit: `f5a1d10e8b440809b71b9e3cc3dce989a98bb747`
- Plan: `D:/tmp/verter-debt-and-deferred-fixes-plan.md` §4
- Audit doc: `docs/arch/debt-closure/13-god-module-split-audit/component_meta.md`

## Split layout (each post-split module < 4000 LOC)

```
crates/verter_session/src/resolver_core/component_meta/
├── mod.rs                  495 LOC   (public types/traits + JSDoc helpers + re-exports)
├── cold_resolver.rs        620 LOC   (resolve_component_meta_parts cold entry)
├── projected_type_expr.rs  403 LOC   (TypeExpr / type-text projection)
├── direct_macro.rs         400 LOC   (direct-macro detection + imported-decl filters)
└── tests.rs               2083 LOC   (extracted #[cfg(test)] mod tests body)
                          ─────
                           4001 LOC   (was 3948 in single file; budget = 4000 per file)
```

The audit doc's alternative four-way sketch (`resolver_entry`,
`lower_props_emits`, `structure_materialize`, `trace_diagnostics`)
named functions that do NOT live in this file
(`materialize_component_meta_structure` lives in
`meta_resolve/materialize/macro_shapes.rs`; trace-diagnostic
plumbing lives elsewhere). The audit doc notes the W5* worker is
"free to deviate". The chosen split honors the actual file content
and the two intra-file SCC pairs from the audit:

- SCC 1 (`resolve_imported_macro_surface`, `resolve_macro_elements`):
  trait METHODS on `ComponentMetaResolverHost` (kept in
  `mod.rs`); recursion is host-impl-supplied, not internal.
- SCC 2 (`render_function_type_for_projected_surface`,
  `render_type_expr_for_projected_surface`): both kept together in
  `projected_type_expr.rs`.

## Public API preservation (D19)

Every name accessible at `crate::resolver_core::component_meta::*`
remains accessible via `pub use` re-exports in `mod.rs`:

- `pub use cold_resolver::resolve_component_meta_parts`
- `pub use projected_type_expr::resolved_elements_to_type_expr_via_type_text`
- `pub use projected_type_expr::projected_macro_surfaces_to_type_expr` (`#[allow(unused_imports)]`)
- `pub(crate) use direct_macro::{imported_declaration_surface_is_authoritative, imported_registry_seed_can_skip_refresh}`
- `pub(crate) use projected_type_expr::project_macro_surfaces_from_expanded_shape`
- All `pub struct`/`pub enum`/`pub trait`/`pub fn` items in §4 of the
  audit doc remain at the same import path.

The existing `resolver_core/mod.rs` `pub use component_meta::{...}`
block was not touched.

## D77 brief discrepancy

**Discrepancy:** The strict scope excludes
`crates/verter_session/tests/architecture_guards.rs`, but three
tests in that file were keyed to the literal path
`crates/verter_session/src/resolver_core/component_meta.rs` and
break the moment the file becomes a directory module:

1. `no_read_source_in_component_meta` — `read_workspace_file()`
   panics on missing file.
2. `no_text_based_macro_surface_projection_helpers` —
   `read_workspace_file()` panics on missing file.
3. `no_unbounded_recursion_in_resolver_core` — `ALLOWED_BOUNDED_RECURSIONS`
   keyed file_stem `component_meta` no longer matches the now-split
   recursive walkers in `direct_macro.rs` /
   `projected_type_expr.rs`.

**Resolution:** updated `architecture_guards.rs` minimally — only
the path- and file_stem-keyed lookups — to scan the new directory
and to allow-list the two recursive walkers under their new
file_stems. No new tests, no semantic change to any guard. Without
this the workspace suite fails on a clean rename. Meets the D77
criterion.

`guard6_exemptions()` still contains a stale single-file path
entry. Harmless (file no longer exists; membership check returns
false for actually-walked files). Cleanup left to W5f.

## Verification

| Check | Result |
|---|---|
| `cargo test -p verter_session` | 2556 / 2556 (0 failed) |
| `cargo test --workspace --tests -j 4` | 10552 / 10552 (0 failed) |
| `cargo clippy --workspace --tests -j 4 -- -D warnings` | clean |
| `cargo fmt --all --check` | clean |
| `public_get_component_meta_byte_equal_with_pre_tier_1` (D19) | passes |

`prior_known_passed_count` workspace = 10552. Current = 10552.
Delta = 0. Monotonic gate satisfied.

## Worker prohibitions (D79)

Standard. No `--amend`, `--force`, `--no-verify`, push, merge,
destructive history rewrite.

## blockers

None.
