# C1 thirteenth deviation — F13: F10's audited-boundary is wider than F10's own text anticipated

Found while running the F10-required "genuinely open" follow-up audit ("the
exact file/function boundary beyond the two named entry points") that F10
itself deferred. Dispositioned via a fresh Codex xhigh consult. Full consult
prompt/output: `/tmp/c1-deviation7-consult-prompt.md` /
`/tmp/c1-deviation7-consult-output.md` (not committed — ephemeral scratch;
this file is the durable record).

## Finding

A scoped read-heavy sweep of `typeinfo/framework_surface/vue_exec/{mod.rs,
normalize.rs, normalize_slots.rs}`, `typeinfo/framework_surface/{scope.rs,
svelte_callable_role.rs, resolved_surface_access.rs, executor.rs, ...}`,
`typeinfo/shallow_surface.rs`, and `structural_carrier_producer/
macro_arg_producer.rs` found the framework-surface query operation family is
wider than F10's originally-named `resolve_vue_macro_surface_with_ctx`/
`vue_macro_dtos_with_ctx` pair, and that a few of F10's implicit "stays"
classifications for files living under `framework_surface/` needed
per-file correction, not directory-based assumption.

## Disposition: ADOPT-NOW, correction/expansion of F10, no reopen

**Verdict, per question:**

1. **`shallow_surface.rs`'s wider fan-out (component-meta, meta_resolve,
   executor, codegen, Vue, Svelte all call it)** — F10 stands, but only the
   NARROW graph-only projector relocates, not the whole three-function
   family wholesale:
   - `resolve_shallow_surface_for` (current-view acquisition +
     `HostResolverContext` construction) — **stays**, session lifecycle.
   - `project_shallow_surface_from_base` (delegates to the graph projector,
     then does source-backed JSDoc enrichment through
     `ctx.ensure_indexed_ready_serve`) — **stays as a session enrichment
     wrapper** over the relocated projector.
   - `project_shallow_surface_graph_only` — **relocates** as
     `attempt_project_shallow_surface_graph(...)`, subject to also
     relocating/splitting its `read_positive_surface_members` dependency.
   The wider callers may all call the SAME relocated projector — that's the
   sanctioned session→kernel direction, and does NOT make their entire
   modules "thin callers"; it removes only their ownership of
   shallow-projection POLICY specifically.
2. **`scope.rs`/`svelte_callable_role.rs`/`resolved_surface_access.rs`** —
   NOT one disposition, corrected per-file:
   - `scope.rs`: **STAYS for now.** Every current production use is output
     NORMALIZATION (constructing `TypeExprScope` alongside materialized
     DTO fields); its `&VerterHost` param should narrow to the graph/
     context, but purity alone doesn't move it — F10 explicitly keeps
     normalization session-side. May travel later if the output-fence work
     relocates the terminal normalizer it feeds.
   - `svelte_callable_role.rs`: **RELOCATES.** `classify_svelte_callable_role`
     executes `demand_symbol_identity` and decides `Complete`/`Partial`/
     exact Svelte `Snippet` identity semantics — genuine semantic query
     policy, not DTO formatting. Belongs with the relocated Svelte query
     slice.
   - `resolved_surface_access.rs`: **STAYS with the session normalizers.**
     The premise that `ResolvedVueSurface`/`SvelteResolvedSurface` must
     relocate is NOT established — they are session-minted
     normalization-authority wrapper tokens, not the raw semantic result
     types; the sealed trait protects the session normalizer input
     boundary. Rehome only if a later output-capability rehome moves the
     terminal normalizers atomically — not merely because the raw resolver
     moves.
3. **`resolve_vue_public_type`** — **same operation family, distinct kernel
   entry point; extend F10's named-entry-point list to include it. No
   separate deviation round needed.** Genuine query policy (synthesized-
   default gate, `Instantiate` execution, shallow projection) splits into a
   semantic-owned `attempt_resolve_vue_public_type(...)` plus a retained
   session-side `VerterHost::resolve_vue_public_type` wrapper for
   current-view acquisition/load-retry/`Option` adaptation.
4. **`define_shapes.rs`'s direct raw-query call**
   (`macro_surface_resolves`) — **no architecture change, but F10's
   addendum text needs a factual correction**: it's an additional direct
   raw-kernel caller (not routed through `vue_macro_dtos_with_ctx`'s cache
   wrapper as previously described). Conversion must preserve
   `Complete(Some)`/`Complete(None)`/`NeedInputs` distinctly — the current
   `.is_some()` collapse must NOT survive verbatim, or a missing
   observation could misreport as a proven-unresolved macro.
5. **`structural_carrier_producer`/`host_manage::eval_env`** — **do NOT
   reclassify `eval_env`, do NOT relocate `macro_type_arg_hot_ref`
   wholesale.** The audit's portability premise for the crate-visible
   producer ENTRY was wrong: `macro_type_arg_hot_ref` itself reaches
   `ctx.ensure_indexed_ready_serve`, its lazy singleflight `MacroHotMirror`
   is a child of session-owned `IndexedReady`, and its cold builder
   hydrates a transient lease from that artifact. What's genuinely
   query-free is only the INNER `lower_type_expr_structural` graph lowerer
   (the file's own doc comment already names this narrower property).
   Relocate/extract only that pure lowerer + semantic product construction;
   keep artifact lookup, mirror storage, singleflight, lease handling, and
   admission in `verter_session`; expose the resulting macro hot product to
   the kernel as an observation or semantic-owned input. `host_manage::
   eval_env` keeps calling the session mirror accessor as today — neither
   "thin relocated-producer caller" status nor a separate deviation is
   needed for it. (Also noted: the file's doc-commented "four production
   sites" is stale/non-exhaustive — re-verify caller count before
   implementation, don't trust the comment.)
6. **JSDoc/raw-source slicing and F12** — **confirmed SHARED F12
   dependency for source acquisition, but NOT a blocker for the entire
   F10/normalizer split.** The chain is real: `member_jsdoc_from_spans`/
   `signature_jsdoc_from_spans` → `slice_canonical_span` →
   `ensure_indexed_ready_serve` → `normalized_analysis_canonical`'s
   fast-path pre-check → `resolve_for_persistent_state` on the
   runtime-JS slow path — the exact F12 chokepoint. A narrower valid seam
   IS available now: a finite `RawSource(canonical)`-shaped observation
   (pure normalization consumes supplied immutable source/slices, absence
   -> `NeedInputs`, session driver loads/retries) — but that observation
   does NOT eliminate F12's requirement for cold runtime-JS input (correct
   satisfaction may still need `.d.ts` companion selection; reading the raw
   requested file directly would violate current normalization semantics).
   Record: (a) complete removal of the source-supply/session edge is gated
   on F12; (b) pure normalizer/value slices may be separated NOW; (c) the
   raw Vue/Svelte query relocation is NOT blocked; (d) `normalize.rs`/
   `normalize_slots.rs` are NOT proven portable as whole files — they have
   OTHER `ensure_indexed_ready_serve` calls reading analyzer facts beyond
   just the JSDoc-slicing ones (`normalize.rs:96`, `normalize_slots.rs:175`
   — not yet independently classified).
7. **Overall: ADOPT-NOW.** Do not stay artificially confined to F10's two
   named functions. Nothing here reaches F10's reject/reopen threshold —
   every newly found dependency still admits a finite observation or a
   session wrapper; no raw materialization authority needs exposing across
   the crate boundary.

### Corrected disposition text (F13, folds into F10)

> **F10 audited-boundary addendum — ADOPT-NOW.** The framework-surface
> relocation covers the semantic query operation FAMILY, not merely the two
> originally named Vue macro entry points. It includes the raw exact Vue
> macro-surface attempt, a distinct Vue public-type attempt
> (`resolve_vue_public_type`), the corresponding Svelte semantic-query
> slices, Svelte callable-role identity classification
> (`svelte_callable_role.rs`), and the shared graph-only shallow-surface
> projector (`project_shallow_surface_graph_only`). Session-owned public
> methods and the framework executor retain current-view capture, input
> loading/retry, fact tracing, cache admission, DTO/wire normalization, and
> publication.
>
> `resolve_shallow_surface_for` remains a session query-returner wrapper.
> `project_shallow_surface_from_base` remains/becomes a session enrichment
> wrapper over the relocated graph-only shallow projector, because it
> hydrates JSDoc from cache-owned raw source. Its component-meta,
> meta-resolve, executor, codegen, Vue, and Svelte callers may call the SAME
> relocated projector; this does NOT relocate those caller modules or their
> lifecycle/normalization responsibilities.
>
> `framework_surface/scope.rs` remains with session output normalization
> (receiver narrows to graph/context authority, but the file stays).
> `svelte_callable_role.rs`'s identity-demand policy relocates.
> `resolved_surface_access.rs` and the `ResolvedVueSurface`/
> `SvelteResolvedSurface` normalization-authority tokens remain session-side
> unless a later output-capability rehome moves the terminal normalizers
> atomically.
>
> `define_shapes::macro_surface_resolves` is an additional direct
> raw-kernel caller (correcting F10's addendum, which described all three
> swept files as using only the cache-wrapped entry point). Its conversion
> must preserve `Complete(Some)`/`Complete(None)`/`NeedInputs` distinctly —
> never collapse a missing observation through a boolean `.is_some()`
> adaptation.
>
> Only the query-free structural-lowering/value core of
> `structural_carrier_producer` relocates, as required.
> `macro_type_arg_hot_ref`, its `IndexedReady`-owned `MacroHotMirror`,
> singleflight/lease behavior, and session artifact loading stay in
> `verter_session`; the kernel receives the hot product through a finite
> observation or semantic-owned input. `host_manage::eval_env` is NOT
> reclassified or relocated.
>
> JSDoc/source hydration is a session source-supply seam. Pure
> normalization may consume a finite raw-source or pre-sliced-text
> observation now, but correct COLD satisfaction for runtime-JS canonicals
> remains dependent on F12's phase-4/7 companion-resolution cutover. This
> dependency does NOT block the raw framework query split.

## Explicit instruction, followed

Record the corrected/expanded scope now; do not implement any of it this
round (consistent with F10's own "record the ownership ruling, defer trait/
relocation implementation" pattern and F12's "continue every genuinely
independent disposition-table row now" instruction — this consult closes
out that specific independent row).
