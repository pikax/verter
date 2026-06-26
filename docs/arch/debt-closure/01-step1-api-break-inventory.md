# Step 1 — Sub-task 1.0 working notes: API-break inventory

Source plan: `<scratch>/architectural-debt-closure.md` (revision 10), Step 1.

This document captures the four `rg` inventories the plan's sub-task 1.0
demands BEFORE adding the additive type extensions or rewriting any
construction sites. The matrix at the end of this file is what sub-task
1.1 applies mechanically.

## `rg "FieldKind\b"`

5 hits, all in-tree:

| File | Note |
|---|---|
| `crates/verter_semantic/src/analysis/type_eval_build.rs` | OWNS `FieldKind` (definition at line 1332) and the closure boundary that consumes it (line 1351). |
| `crates/verter_session/src/host_manage.rs` | Reads `FieldKind` inside the `compute_evaluated_types_from_owner_context` closure to dispatch surface-id capture per kind. Sub-task 1.2 rewrites this closure. |
| `crates/verter_session/src/meta_resolve.rs` | Re-exports / forwards `FieldKind` for downstream consumers (verify before touching). |
| `crates/verter_session/src/resolver_core/component_meta.rs` | `FieldKind` consumer at the resolver-core boundary. |
| `crates/verter_session/src/resolver_core/component_meta_query_engine.rs` | `FieldKind` consumer at the engine boundary. |

**No non-`verter_session` / non-`verter_semantic` consumers.** STOP CONDITION 3 does not fire.

`FieldExpansionContext` is added next to `FieldKind` (per D1.1). The
existing `FieldKind` enum stays exactly as today; the closure parameter
changes from `FieldKind` to `FieldExpansionContext { kind, macro_index, output_path }`.

## `rg "AnalyzedMacro\b"`

~50 hits across 8+ crates. Categorized:

- **Producer (1 site):** `crates/verter_semantic/src/analysis/macros.rs:1472` — `extract_define_*` builds `AnalyzedMacro { ... }`. Sub-task 1.1 adds `parsed_type_argument: Option<Arc<TypeExpr>>` here (parsed from the first `call.type_arguments.params` once during analysis).
- **Owner (1 site):** `crates/verter_semantic/src/analysis/types.rs:1230` — struct definition + `Serialize` (1266) + `Deserialize` (1314) impls (manual; `#[serde(default)]` does NOT apply to the struct fields, only to the `Wire` deserialization helper at 1318-1345).
- **Reader-only consumers (~50 files):** `verter_lsp`, `verter_mcp`, `verter_diagnostics`, `verter_actions` rules, etc. They read existing fields (`kind`, `prop_fields`, `emit_fields`, `model_name`, `expose_fields`, `binding_name`, `is_type_based`, etc.). The new `parsed_type_argument` field is opt-in for new consumers; existing reader code stays unchanged.

**Manual-serde back-compat requirement (D1.2):** the `Wire` struct at `types.rs:1318` already uses `#[serde(default)]` on every optional field — the same `#[serde(default)]` strategy applies to `parsed_type_argument`. Old payloads (no `parsed_type_argument` key) deserialize with `None`. The `Serialize` impl at line 1266 must:

1. Bump the field count tally in the `count = 7 + ...` expression to include `parsed_type_argument` when `Some(...)`.
2. Insert `s.serialize_field("parsedTypeArgument", &self.parsed_type_argument)?;` (camelCase per the existing `rename_all = "camelCase"` convention on the Wire struct).
3. Verify the `Serialize` and `Deserialize` halves agree on field ordering (serde tolerates field-order mismatch on objects, but the order must agree for human readability).

The `From<Wire> for AnalyzedMacro` constructor at line 1346 maps the new field through.

## `rg "SemanticQueryKey::Instantiate\b"` (12 files)

| File | Construction sites (rg `SemanticQueryKey::Instantiate\s*\{`) | Mode in scope | Decision per matrix at D1.4 |
|---|---|---|---|
| `crates/verter_session/src/semantic_query_memo.rs` | 3 sites: `family_and_slot` projector (line 900, USE site — destructures the key into `(FamilyKey, ModeSlot)`) + 2 test-only construction sites (lines 2419, 2453). | N/A — projector. | Add `body_mode` to the destructured pattern at 900; project body_mode into `mode_to_slot(body_mode)` (parallels how `ProjectMember` does it today). Test sites pass `Expanded` — test intent. |
| `crates/verter_session/src/semantic_query.rs` | 5 sites: `instantiate` trait helper (line 1485) + 4 test-only sites (1565, 1569, 1723, 1730). | The trait helper today has no `mode` parameter. Per D1.4, the helper grows `body_mode: ProjectionMode`. | Trait helper signature becomes `fn instantiate(&self, base, args, body_mode)`. Tests pass `Expanded` (legacy structural intent). |
| `crates/verter_session/src/project_semantic_dispatch/mod.rs` | 2 sites: pattern-match destructure at 426 (recursive-ref guard); pattern-match at 438 (build dispatch). | Match-arms — destructures only. | Add `body_mode` to destructured patterns; pass to `build_instantiate(base, args, *body_mode)` at 438. |
| `crates/verter_session/src/project_semantic_dispatch/build.rs` | The `build_instantiate` signature itself; arg-id loop at 319-345 calls `shallow_lower_type_expr` with hardcoded `Expanded`; body lowering at 376-384 same. | `body_mode` parameter is added to `build_instantiate`. | `build_instantiate` grows `body_mode: ProjectionMode` parameter; arg-id loop AND body lowering thread `body_mode` instead of hardcoded `Expanded`. |
| `crates/verter_session/src/project_semantic_dispatch/lower.rs` | 2 sites: 299 (built-in utility re-route), 457 (user decl re-route). Both inside `shallow_lower_type_expr`, which takes a `mode: ProjectionMode` parameter. | YES — `mode`. | Propagate `mode` (the lower's `mode` param). |
| `crates/verter_session/src/project_semantic_dispatch/walk.rs` | 3 sites: 464, 532, 733. The walker has its own `self.mode` (verify). | YES — `self.mode`. | Propagate `self.mode`. |
| `crates/verter_session/src/project_semantic_dispatch/raise.rs` | 3 sites: 431 (recursive-ref destructure), 443 (raise dispatch destructure → `build_instantiate`), 734 (operator dispatch). | Site 443 destructures only; passes through to `build_instantiate(base, args, *body_mode)`. Site 734 has `mode` in scope. | 431/443 add `body_mode` to destructure pattern; 443 passes to `build_instantiate`; 734 propagates `mode`. |
| `crates/verter_session/src/project_semantic_dispatch/evaluate.rs` | 1 site: 140 (typeof unwrap inside `evaluate_deferred_semantic_node`'s fix-point loop). | NO — no outer mode. | **Decision: hardcode `ProjectionMode::Expanded`.** Rationale: the fix-point loop walks through unwrapped body layers (mappers, conditionals, object surfaces). Navigate would leave the body as a lazy Ref shell and the next iteration would have nothing to inspect. Verified empirically — `meta::meta_tests::resolve_component_meta_*` regress under Navigate. |
| `crates/verter_session/src/project_semantic_dispatch/relation.rs` | 2 sites: 192 + 270. Identity-carrier unwrap + object-vs-record relation. | NO — no outer mode. | **Decision: hardcode `ProjectionMode::Expanded`.** Rationale: site 192 feeds `evaluate_deferred_semantic_node` (same body-walk argument as evaluate.rs:140); site 270 reads `view.members` / index signatures off the unwrapped body. Navigate yields a lazy shell with no surface view. |
| `crates/verter_session/src/project_semantic_dispatch/enumerate.rs` | 1 site: 182 (DeclPlaceholder arm in `expand_keyof`). | NO — no outer mode. | **Decision: hardcode `ProjectionMode::Expanded`.** Rationale: the comment immediately above the call states "expand via Instantiate before enumerating keys" — the next `KeyNamesFrame::Expand` reads keys off the body's Object members / Union arms / surface view. Navigate keeps the body as a lazy shell so the enumeration finds nothing. Verified empirically — `meta::meta_tests::*_materializes_*_variant_and_slot_helpers` regress under Navigate. |
| `crates/verter_session/src/project_semantic_dispatch/tests.rs` | ~30 test sites. | N/A — test intent. | All sites pass `body_mode: ProjectionMode::Expanded` for legacy structural correctness. The new `instantiate_memo_splits_per_body_mode` test (FAIL-FIRST #5) is the only site that passes `Navigate` deliberately. |
| `crates/verter_session/src/resolver_core/component_meta_query_engine.rs` | 1 site: 2032 (`dispatch_root_instantiated` — feeds `dispatch_projected_surface` → `projected_surface_from_semantic_node`). | NO — no outer mode. | **Decision: hardcode `ProjectionMode::Expanded`.** Rationale: the consumer reads surface members / call signatures off the Object body. Navigate would return a lazy Ref shell and `projected_surface_from_semantic_node` would have no view to project. Verified empirically — `project_expr_surface_expr_materializes_*` regresses under Navigate. |

## `rg "DeclarationScopePayload\b"` (7 files)

All in `verter_session`. Auxiliary type — used by the dispatch lower for prepared-decl scope binding. Not directly affected by Step 1 changes; listed here for completeness per the gate.

| File | Role |
|---|---|
| `crates/verter_session/src/resolver_core/bare_name_resolve.rs` | OWNS `DeclarationScopePayload`. |
| `crates/verter_session/src/project_semantic_dispatch/build.rs` | Consumer (passes through to lower). |
| `crates/verter_session/src/project_semantic_dispatch/lower.rs` | Consumer (parameter on `shallow_lower_type_expr`). |
| `crates/verter_session/src/project_semantic_dispatch/mod.rs` | Re-exports / forwards. |
| `crates/verter_session/src/meta_resolve.rs` | Constructs payload before calling lower. |
| `crates/verter_session/src/meta_resolve_tests.rs` | Test fixtures. |
| `crates/verter_session/src/resolver_core/component_meta_query_engine.rs` | Engine consumer. |

## Migration matrix summary (D1.4 final, applied in sub-task 1.1)

| Site | Decision |
|---|---|
| `lower.rs:299` | propagate `mode` (caller's lower mode) |
| `lower.rs:457` | propagate `mode` |
| `walk.rs:464` | propagate `self.mode` |
| `walk.rs:532` | propagate `self.mode` |
| `walk.rs:733` | propagate `self.mode` |
| `raise.rs:443` | propagate `body_mode` from destructured key into `build_instantiate` |
| `raise.rs:734` | propagate `mode` (operator dispatch's outer mode) |
| `evaluate.rs:140` | hardcode `Expanded` (body fix-point walk requires inspectable surface) |
| `enumerate.rs:182` | hardcode `Expanded` (key enumeration reads body surface) |
| `relation.rs:192` | hardcode `Expanded` (`evaluate_deferred_semantic_node` walks body) |
| `relation.rs:270` | hardcode `Expanded` (object-vs-record reads `view.members`) |
| `engine.rs:2032` | hardcode `Expanded` (surface projection reads body view) |
| `semantic_query.rs::SemanticQueryApi::instantiate` | API change — grows `body_mode` param |
| `semantic_query_memo.rs:900` (projector) | destructure `body_mode`, project to `mode_to_slot(body_mode)` |
| `mod.rs:438` (build dispatch) | destructure `body_mode`, pass to `build_instantiate(base, args, *body_mode)` |
| `mod.rs:426` (sentinel) | destructure `body_mode: _` (unused) |
| `raise.rs:431` (sentinel) | destructure `body_mode: _` |
| `build.rs::build_instantiate` | grows `body_mode: ProjectionMode` parameter; threads through arg-id loop AND body lowering |
| `meta_resolve.rs::materialize…` thin wrapper (sub-task 1.2) | propagate caller's `mode` |
| `host_manage.rs::compute_evaluated_types_from_owner_context` closure (sub-task 1.2) | hardcode `Expanded` (consumer wants reduced output) |
| Test fixtures (`tests.rs`, `semantic_query.rs`, `semantic_query_memo.rs` test sites) | pass `Expanded` for legacy intent; FAIL-FIRST #5 passes `Navigate` deliberately |

## STOP CONDITION 3 check

Plan STOP CONDITION 3: "Step 1 sub-task 1.0 grep finds non-`verter_session` consumers of `FieldKind` you cannot migrate in scope."

`FieldKind` consumers (5 files) are all in `verter_semantic` (1: owns the type) or `verter_session` (4: consumers in scope of this PR). **STOP CONDITION 3 does not fire — proceed to sub-task 1.1.**

## Sub-task 1.0 commit

Per the plan, sub-task 1.0 lands no production code; it only documents
the inventory in working notes. This file IS the working notes; the
inventory commits with sub-task 1.1's additive type extensions.
