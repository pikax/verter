# Phase 05f worker report

**Worker:** Phase 5f (fallthrough + indexed-paths + package-backed)
**Branch:** `wt/phase-05f-fallthrough-and-indexed`
**Base:** `034dfc70` (Phase 5e integrated post-rebase)
**Sub-plan:** `<scratch>/verter-architecture-cutover-phase-05.md`

## Commits

| SHA | Message |
|---|---|
| `86e485cb` | `refactor(meta): close inherited-emits seed via Conditional empty-path distribution` |
| `76ebc1d4` | `refactor(meta): close indexed-paths seed via decomposed ProjectPath dispatch` |
| `cd170849` | `docs(meta): defer package-backed + slot-shapes + mapped-types seeds to 5g` |

## Migration counts

| Commit | Migration count | Seed flipped at this boundary |
|---|---|---|
| 7 | 1 dispatch arm added (`SemanticNodeData::Conditional` in `expand_terminal_step`, gated on TypeParam/Infer check) | `resolver_coverage_inherited_emits` |
| 8 | 1 dispatch arm + 1 caller migration (`produce_one_macro_object_shape` → `project_expr_class_a_via_dispatch_threaded`) + 1 helper (`decompose_indexed_access_chain`) | `resolver_coverage_indexed_paths` |
| 9 | 0 production migrations; 3 ignore-message rewrites (deferral rationale) | (none — 3 seeds deferred to 5g) |

## Seed flip status

| Seed | Status | Closes at |
|---|---|---|
| `resolver_coverage_inherited_emits_branch_merged_surface` | **GREEN** | commit 7 |
| `resolver_coverage_indexed_paths_deep_chain` | **GREEN** | commit 8 |
| `resolver_coverage_package_backed_function_property_gate` | **DEFERRED to 5g** | (rationale below) |
| `resolver_coverage_slot_shapes_typed_bindings_lower_to_primitive` | **DEFERRED to 5g** | (deferred from 5b; rationale below) |
| `resolver_coverage_mapped_types_exclude_distributes` | **DEFERRED to 5g** | (deferred from 5e; rationale below) |

## Class A parity

`class_a_invisibility_mapped_pick_two_keys_unchanged` (from 5b) — **GREEN** throughout 5f.
Verified at line 9337 of `/tmp/p05f-workspace.txt`:
`test project_semantic_dispatch::tests::class_a_invisibility_mapped_pick_two_keys_unchanged ... ok`

## Test counts (measured)

`/tmp/p05f-workspace.txt`:
- **Block count:** 44 (>= 40 per §0.4 r11)
- **Pass:** 10211
- **Fail:** 0
- **Ignored:** 10
- **0 `test result: FAILED` blocks**

Counts:
```
$ awk '/^test result: ok/ { p+=$4; i+=$8 } /^test result: FAILED/ { f+=$4 } END {print "Pass:", p, "Fail:", f, "Ignored:", i}' /tmp/p05f-workspace.txt
Pass: 10211 Fail:  Ignored: 10
```

`grep -c "test result: ok\." /tmp/p05f-workspace.txt` → 44
`grep -c "test result: FAILED" /tmp/p05f-workspace.txt` → 0

## Verification gates

- `cargo test --workspace --tests --verbose` → all 44 blocks `ok`, 0 failed
- `cargo clippy --workspace --tests -- -D warnings` → clean
- `cargo fmt --all --check` → clean
- `pnpm install --frozen-lockfile` → done in 19.8s

## Deferred items (§0.5.1)

### `resolver_coverage_package_backed` — deferred to 5g

The seed's hermetic harness places the consumer SFC at `/c.vue`
(workspace root). The unowned-resolver's `resolve_node_modules_package`
walks ancestor directories of the importer; a root-level path has no
parents, so resolution returns `None` before the `materialize_surface`
gate ever runs. Pre-fix output `prop_names=[]` therefore fails the
positive assertion (`callback must surface`) for the WRONG reason —
not gate enforcement, but resolution. The fixture also makes the
negative assertion vacuous: `event` is a function PARAMETER inside
`InnerHandler`'s call signature, never a top-level prop in the
component-meta extraction path regardless of gate enforcement.

Phase 5f's commits 7+8 already apply the package-backed gate via
the DeclPlaceholder check in `expand_terminal_step` (`walk.rs:751`)
for any case where lowering DOES produce a package-backed DeclRef;
the dispatch helper's `materialize_surface` route is wired and
ready in `project_semantic_dispatch/mod.rs:642`. Closes in 5g
alongside the engine deletion + the 7 Class A fixture authoring
task — that lands a discriminating fixture (function-typed nested
member with sibling object members that WOULD leak without the
gate) plus the harness fix to seat the consumer deep enough for
the unowned node_modules walk to find `pkg-types`.

### `resolver_coverage_slot_shapes` — deferred to 5g

The `ResolveMacroPayload::DefineSlots` arm dispatches
`ProjectPath{type_args[0], [], mode}` (`build.rs:1993`) but the
slot-bindings extractor at `meta_resolve.rs::DefineSlots` arm still
walks the lowered Object's call_signatures via the raw TypeExpr (it
does not consult the dispatch result for binding parameter types).
Phase 5f's commits 7+8 enable Conditional/IndexedAccess empty-path
materialisation for the macro shape extractor (closes inherited-emits
+ indexed-paths), but the slot-binding-parameter lowering is a
distinct path that requires its own migration in 5g (alongside the
engine deletion that retires the engine-internal slot resolver).

### `resolver_coverage_mapped_types` — deferred to 5g

`Exclude<>` is a 'deferred utility' in `dispatch's build.rs:962-966`
— its body lowers to `T extends U ? never : T` but the conditional
reduction depends on the relation engine's ability to decide
string-literal-extends-string-literal assignability. Phase 5f's
commits 7+8 add open-Conditional empty-path distribution +
IndexedAccess empty-path materialisation, but neither closes the
`Exclude<'a'|'b'|'c', 'b'>` reduction because the conditional check
(`'a'`, `'b'`, `'c'`) is bound to concrete string literals, not
unbound, so distribution does NOT trigger (and would be wrong if it
did — `Exclude` requires CONCRETE reduction to drop the matching
literal, not Union both branches). Closes in 5g where the engine
deletion + 7 fixture authoring lands a discriminating `Exclude`
evaluation path that routes through the relation engine's
literal-equality check.

## Worker honesty integrity check (§0.4 r11)

- Tee'd `cargo test --workspace --tests --verbose` output to `/tmp/p05f-workspace.txt` — cited above.
- Block count 44 >= 40.
- Counts measured by this worker; orchestrator re-run expected within ±5.
- Used EXACTLY `cargo test --workspace --tests --verbose` (no `--no-fail-fast` extras).

## §0 binding amendment compliance

Phase 5f introduced **ZERO** new `SemanticQueryKey` variants. The
single Phase 5 variant (`ResolveMacroPayload`) was added in 5b; my
work uses the existing variant set:
- Commit 7: extends `expand_terminal_step` to handle `Conditional`
  via the existing `SemanticQueryKey::ProjectPath` re-dispatch path
  (mirrors the existing `Conditional` arm in `advance_step`).
- Commit 8: decomposes `IndexedAccess` chains at the caller boundary
  into `(base_expr, [PathSegment::Index(...)])` for dispatch via
  the existing `SemanticQueryKey::ProjectPath` variant.
- Commit 9: docs only.

## Marker

Marker JSON path: `crates/verter_session/.phase-markers/phase-05f-complete`
(written by next commit).
