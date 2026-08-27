# C1 ninth deviation — F10: framework macro-surface query ownership and output-capability closure

Found while classifying `project_semantic_dispatch`'s
`host_for_fact_tracer_install()` call sites per F8's step 4. This is the
exact "watch-for" trigger F8 itself named: "`resolve_vue_macro_surface_with_ctx`
(or another escape) turning out to be independent query semantics that
cannot relocate into the single kernel." Dispositioned via a fresh Codex
xhigh consult. Full consult prompt/output:
`/tmp/c1-deviation4-consult-prompt.md` / `/tmp/c1-deviation4-consult-output.md`
(not committed — ephemeral scratch; this file is the durable record).

## Finding

`project_semantic_dispatch/semantic_source.rs`'s
`replay_vue_macro_type_argument_surface` (genuine, non-test, production
`impl ProjectSemanticDispatch` method, three production callers at lines
701/819/1062 — member-path, callable-occurrence, index-position replay)
escapes through `self.ctx.host_for_fact_tracer_install()` into
`VerterHost::resolve_vue_macro_surface_with_ctx`
(`typeinfo/framework_surface/vue_exec/mod.rs:517`), a session-side
inherent method belonging to CLAUDE.md's "Framework Adapter Substrate
(CRITICAL)" — a separate, already-CRITICAL-guarded architecture area
C1's charter never mentions.

`resolve_vue_macro_surface_with_ctx`'s own doc comment: "Every
view-sensitive read... flows through `ctx`... The dispatcher is
`ctx.dispatch()`, keeping the surface inside the single resolution
engine" — so it is not itself an independent second resolver, but it does
real per-macro-kind query POLICY (macro-kind shortcuts, provenance choice,
carrier lowering, indexed-access decomposition, shallow projection) that
is semantic query semantics, not lifecycle glue.

A second, independent coupling in the same audit:
`project_semantic_dispatch/output_materialization.rs` centrally implements
the private sealed `OutputProjector` capability for the session-owned Vue
(`typeinfo::framework_surface::vue_exec::TypeinfoVueSurfaceOutputCap`) and
Svelte (`svelte_exec::TypeinfoSvelteSurfaceOutputCap`) terminal output-sink
types — this file IS the module CLAUDE.md's "Carrier IDE TS Surface
Principle" output-fence machinery lives in (the sealed reverse
`SemanticNodeId -> TypeExpr` boundary). Once `output_materialization.rs`
relocates, it cannot name those downstream session-owned capability types.

## Disposition: ADOPT-NOW (recorded as F10)

**Verdict: ADOPT-NOW.** The F8 procedural stop was correctly triggered,
but the charter's Abort bar is NOT met — `resolve_vue_macro_surface_with_ctx`
is a wrongly-homed query-time projection operation that already delegates
to the single dispatcher, not an independent resolver that cannot
relocate.

### Corrected rule

- Extract the RAW exact-macro payload-surface operation into
  `verter_semantic`, owned by the relocated semantic kernel: macro lookup
  observations, runtime-object/model handling, projection
  context/provenance selection, macro-hot-carrier demand, indexed-access
  decomposition, shallow projection, unresolved-arm diagnostics. It takes
  the new `ResolverObservation` interface and returns `AttemptOutcome`
  (preserving the distinction between a proven semantic `Complete(None)`
  and a missing observation needing `NeedInputs(LoadSet)`).
- `ProjectSemanticDispatch::replay_vue_macro_type_argument_surface` calls
  that relocated operation DIRECTLY — never `VerterHost`,
  `ExecutorResolveCtx`, `PlannedDemand`, the framework registry, a
  framework cache, or a session DTO producer.
- Session-side `VerterHost` public methods and the framework EXECUTOR
  become thin callers of the SAME kernel operation — they may
  capture/load/retry, install fact tracing, perform cache admission,
  normalize resolved output, encode wire results; they own no duplicate
  macro-surface resolution policy.
- The framework-adapter contract (CLAUDE.md's Framework Adapter Substrate
  section) is preserved as-is: `FrameworkAdapterCtx` stays facts/
  carrier-only; adapters plan through the closed `PlannedDemand`
  vocabulary; the session executor exhaustively maps demands onto PUBLIC
  kernel entry points. `PlannedDemand` is never a callback path FROM the
  kernel INTO session (this is the reason disposition-2 from my consult
  prompt — "redirect the kernel's internal replay through
  `PlannedDemand`" — is REJECTED: `PlannedDemand::MacroPayload`'s executor
  deliberately IGNORES `MacroPayloadSelector::macro_index` and aggregates
  every contributing macro for the requested wire kind
  (`executor.rs:526`) — routing exact semantic replay through it would
  change meaning AND create a kernel→session dependency in the wrong
  direction).
- **Do NOT relocate `typeinfo/framework_surface/**` or `vue_exec/**`
  wholesale** — this is an F2-style AUTHORITY split, not an F9-style whole
  directory relocation. Relocate only the query-time algorithm slices
  proven necessary; keep `FrameworkAdapterRegistry`, `FrameworkAdapterCtx`,
  `PlannedDemand`, `ExecutorResolveCtx`, adapter planning, framework
  caches, the audited wire entry, graph export, and wire/status
  normalization in `verter_session`. Audit/split `typeinfo/{surface,
  shallow_surface,types}` and `structural_carrier_producer` only to the
  extent their query/value cores are required by the relocated operation.
- `output_materialization.rs`'s sink registration rehomes ATOMICALLY with
  the dispatcher move: every terminal sink currently defining a registered
  `OutputProjector` capability — including the Vue and Svelte framework
  sinks — must be split or relocated enough that the private sealed
  capability + payload-vault guarantees remain COMPILER-ENFORCED across
  the new crate boundary. No public raw-materialization escape, no
  unsealed downstream implementation bridge.
- **New Abort/rescope trigger, narrower than F8's**: if the completed
  dependency audit proves the raw macro-surface operation cannot return
  `AttemptOutcome` with a finite `LoadSet`, or that preserving the output
  fence necessarily requires exposing raw materialization authority across
  crates, stop and reopen Fork 4. The evidence gathered so far does not
  show this.

### Relocation-extent finding (why not "wholesale")

At `vue_exec/mod.rs:517` the macro-kind shortcuts, provenance choice,
carrier lowering, indexed-access decomposition, and shallow projection are
genuine semantic query policy — the `VerterHost` receiver on
`resolve_vue_macro_surface_with_ctx` is "effectively artificial" (it barely
touches `self`, mostly threads `ctx`). But `vue_exec/normalize*.rs`'s DTO
normalization responsibility is NOT automatically a resolver-SCC member —
it may need a physical split once the output-fence closure
(`output_materialization.rs`'s atomic rehome, above) is worked out, not
before.

### Entanglement sweep — three MORE files found, not yet audited

The consult's own direct-name sweep for `resolve_vue_macro_surface_with_ctx`/
`framework_surface` usage from the relocation-adjacent directories found,
beyond the two I already knew about:

- `resolver_core/component_meta/mod.rs`
- `meta_resolve/projectors/define_shapes.rs`
- `meta_resolve/slot_binding_graph.rs`

None of these three have been read or classified yet — this is concrete
next-round work, not resolved by this disposition. The consult confirmed
NO direct production references from the named relocation directories to
`typeinfo::vue_macro_codegen`, `semantic_query_memo`, or `semantic_query`
for THIS specific coupling (framework-surface), and that reverse
session→kernel references remain legal (expected — that's the "thin
caller" direction the corrected rule requires).

## Addendum — the three swept files confirmed as more callers, not a new deviation

Read the three files the consult's sweep found (`resolver_core/
component_meta/mod.rs`, `meta_resolve/projectors/define_shapes.rs`,
`meta_resolve/slot_binding_graph.rs`) — all three call
`typeinfo::framework_surface::vue_exec::vue_macro_dtos_with_ctx`, a
sibling entry point to `resolve_vue_macro_surface_with_ctx` in the SAME
file. Its signature: `pub(crate) fn vue_macro_dtos_with_ctx(ctx: &dyn
crate::resolver_core::ResolverContext, request: &VueMacroSurfaceRequest)
-> MacroDtosRead` — takes `ctx` directly (not `&VerterHost`), calls
`ctx.host_for_fact_tracer_install()` internally, and is a CACHING WRAPPER
around the same raw macro-surface query (its own doc comment: "Load the
CURRENT... `IndexedReady` BEFORE touching the cache", "keeps the cache free
of entries keyed on an unvalidated identity").

This is the SAME operation family F10 already dispositions, not a new
deviation: the cache-admission half (matches F10's "may... perform cache
admission") stays in `verter_session`; the raw query algorithm underneath
relocates per F10's corrected rule. `resolver_core/component_meta/mod.rs`
additionally calls `svelte_exec::resolve_svelte_surface` — the Svelte
sibling of the same pattern; F10's corrected rule already covers this
generically ("the framework-adapter contract is preserved... adapters
continue to plan through the closed `PlannedDemand` vocabulary" — Svelte
and Vue are structurally symmetric per the Framework Adapter Substrate's
own design), so no separate Svelte-specific consult round was needed here,
but the Svelte side of the eventual split still needs its own
implementation-time verification once code is written.

`resolver_core/component_meta/` being one of the three callers is
expected and unsurprising — it's already part of F2's split (phase 5), and
F2's own original text already named `typeinfo::{surface,raise,
framework_surface}` as one of the modules whose algorithmic core relocates
"where a currently-imported session module is ITSELF query-time algorithm
rather than lifecycle glue." F10 is confirming and concretizing that
general F2 rule for this specific subsystem, not contradicting it.

## Explicit instruction, again

"Stop adding `ResolverObservation` methods and finish the file-level plus
transitive entanglement audit first" — repeated from F9, now covering the
three newly-found files above in addition to `semantic_query_memo`'s
per-site classification and the remaining ~20 `ResolverContext` method
return-type audits from F8/F9.
