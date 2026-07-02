# Hot-Materialize Tripwire — Residual Syntactic-Gap Deferral (codex-DEFER debt row)

**Status**: DEFERRED (codex-DEFER) — the residual hot-path reverse-materialization fence
`hot_path_never_calls_materialize_type_expr`
(`crates/verter_session/tests/cases/output_projector_residual_guards.rs`) is a FROZEN syntactic
tripwire, not the universal authority. The universal "no hot materialize-then-decide" invariant is
carried by the STRUCTURAL rail (the `NoTypeExpr` marker trait, the sealed `OutputProjector`
capability, and the production conversions that move hot decisions onto node-domain
`RaisedShapeFacts` / `RaisedShapeKey` by construction). This row records the named syntactic residual
surfaces and the path that closes each. It is a **durable** debt home — it is independent of
`docs/arch/stage9-materialize-fence-deferral.md` and SURVIVES that file's deletion at fence-GREEN,
because one named residual (FN5's Unknown-fence typed-degradation end-state) lands in Stage 10, after
the fence doc is gone.

**Ruling source (codex-DEFER, binding)**: `mechanism_ruling: codex-reconciliation-hot-materialize-sc-first-2026-06-27`
— the Structural-Confinement-First reconciliation that froze the name-based tripwire after the
generic-minter-name (`inner`) laundering escape. Under that ruling, false-positive NARROWINGS of the
tripwire stay welcome, but false-negative BROADENINGS (new reader rails that try to make the
syntactic scanner universal) are REFUSED — the gaps below are closed structurally by the later
conversions, not by widening the scanner.

## The three named residuals

### FN4 — arbitrary expression-macro body blindness

The frozen tripwire deeply scans only the explicitly-handled macro forms (`matches!`, `vec!`). It
does NOT scan arbitrary expression-macro bodies. A materialize-then-decide hidden inside an
unhandled user macro body (e.g. a custom `select!`-style or assertion macro that branches on a
materialized `TypeExpr` variant) is therefore not syntactically caught. This is a syntactic gap, not
a hole in the invariant.

### FN5 — Unknown-control-flow fence typed-degradation end-state (trait-default scan reconciled)

The trait-default facet is now RECONCILED; one residual remains.

1. **Trait-default scan (reconciled)**: the global Unknown-control-flow fence
   (`no_new_semantic_unknown_control_flow_outside_owner`) scans free / impl / module AND
   trait-DEFAULT (provided) method bodies for a non-owner `TypeExpr::Unknown { raw: <sentinel> }`
   construction, with the SAME `#[cfg(test)]` exclusion, per-fn raw-taint frame, and fn attribution
   as a free / impl fn within the same scanner (`UnknownSentinelScanner::visit_trait_item_fn`).
   A sentinel-`Unknown` fabricated inside a NON-test trait default body IS caught (direct
   construction, `TypeExpr` alias, bare-variant import, and the field-shorthand raw-taint form); a
   `#[cfg(test)]`-gated trait default is skipped. This facet is no longer a syntactic gap — the
   trait-default scan carries only the same FN4-class macro-body residual the free / impl scan does.
2. **Typed-degradation end-state**: the fence's intended END-STATE is a typed degradation / typed
   state carrier rather than `TypeExpr::Unknown` control flow. The fence today recognises the
   `TypeExpr::Unknown { raw: <sentinel> }` control-flow shape; the downstream refinement replaces
   that control sentinel with a typed degradation, at which point the control-flow shape itself
   disappears.

### FN6 — inert detector-name spellings in the gate ident lists (deferred-from-§5a, closes-at-§5c, codex-sanctioned)

- **Exact item**: the detector-name spellings `slot_callable_param_and_return`,
  `callable_arm_from_raised`, and `snippet_callable_positional_bindings` in
  `HOT_DECIDE_TAINTED_GATE_IDENTS` / `HOT_EXTRACTING_GATE_IDENTS`
  (`crates/verter_session/tests/cases/output_projector_residual_guards.rs`) are now INERT: the
  production fns they spell were deleted by the node-domain slots conversion (the legacy `TypeExpr`
  callable-helper deletion), so the spellings can no longer match any production code. (The §5c
  sweep covers both lists wholesale, which also catches the sibling stale spelling
  `slot_callable_param_and_return_from_arms`.)
- **Why not now (deferred from §5a)**: §5a was the codex-scoped orphaned-sink deletion
  (`raise_realized_callable_member_value` + its guard authorizations). Under the frozen
  Structural-Confinement-First ruling (above), these name-lists are a SUPPLEMENTARY syntactic
  tripwire — the structural `NoTypeExpr` / sealed-capability rails are the primary defense — so an
  inert supplementary spelling is not a correctness exposure, and sweeping the lists carries its own
  non-vacuity (hollow-proof) obligation that is outside §5a's mechanical scope.
- **Owning future block**: §5c.
- **Temporary behavior**: the inert spellings stay in the two gate ident lists. The (T)
  taint-through-extracting-gate self-test keeps exercising taint propagation through the retained
  `slot_callable_param_and_return` GATE spelling, driven by the live mat-direct taint source
  `raise_member_value`.
- **Fail-closed guard/test preventing silent loss**: an inert gate spelling can only FAIL to match
  production code — it cannot false-pass anything (unlike `HOT_TERMINAL_SINKS`, these lists carry no
  per-entry located-fn accounting requirement); the hot-materialize fence and the per-entry
  `HOT_TERMINAL_SINKS` accounting rail stay GREEN; the structural rails carry the invariant.
- **Closing condition**: the §5c ident sweep of both lists (dropping every spelling with no
  production fn) plus a both-rails hollow-proof that the syntactic tripwire and the structural rail
  remain non-vacuous after the sweep.

## Structural-closure path (why the residual is a syntactic gap, not an invariant hole)

FN4 (macro-body blindness) is closed by REMOVING the materialized `TypeExpr` from hot inputs. Once
the production conversions (B1 / B2 / C) move hot decisions onto node-domain facts
(`RaisedShapeFacts` / `RaisedShapeKey`) by construction, there is no hot materialize-then-decide left
for a macro body (FN4) to hide — the syntactic gap becomes moot because the thing it could miss no
longer exists on the hot path. The FN5 trait-default scan facet (FN5.1) is closed DIRECTLY: the
Unknown fence scans trait-default bodies at parity with free / impl fns (above), so there is no
trait-default-specific gap. The FN5 typed-degradation end-state (FN5.2) is a downstream refinement of
the Unknown control-flow shape, not a hot-input exposure.

## Closing stage

- **FN4** (macro-body blindness) closes WITHIN **Stage 9**, via the B1 / B2 / C node-domain
  conversions — the same conversions that turn the residual fence GREEN.
- **FN5.1** (trait-default scan) is already CLOSED: the Unknown fence scans trait-default bodies at
  parity with free / impl fns, with the same cfg-test exclusion, raw-taint frame, and fn attribution.
- **FN5.2** (Unknown-fence typed-degradation end-state) is **Stage 10** — a downstream typed-state
  refinement that lands after the fence doc is deleted, which is why this debt row lives in a durable
  home.
- **FN6** (inert detector-name spellings) closes at **§5c** — the ident sweep of both gate lists
  plus the both-rails hollow-proof; codex-sanctioned deferred-from-§5a.

## The global Unknown fence STAYS ENABLED in Stage 9

The global Unknown-as-control-flow fence `no_new_semantic_unknown_control_flow_outside_owner` is
**ENABLED and GREEN in Stage 9** and stays so. FN5's deferral is now ONLY the downstream
typed-degradation refinement (FN5.2); the trait-default scan facet (FN5.1) is already reconciled (the
fence scans trait-default bodies at parity with free / impl fns) — it is NOT a blocker to the Unknown
fence being enabled now. The fence guards the real invariant today; this row records only the residual
syntactic surface (FN4 macro-body blindness, FN5.2 typed-degradation end-state) and its close.

## Closure criterion

This debt row is cleared when (a) the B1 / B2 / C node-domain conversions land (removing materialized
`TypeExpr` from hot inputs, mooting FN4) and (b) the Stage-10 typed-degradation end-state replaces the
`TypeExpr::Unknown` control-flow shape (closing FN5.2) — at which point the frozen tripwire's named
residuals are empty and this file is deleted. (FN5.1, the trait-default scan, is already reconciled.
FN6 clears independently and earlier, at §5c, via the ident sweep + both-rails hollow-proof; its
clearance removes the FN6 section but does not gate this file's deletion.)
