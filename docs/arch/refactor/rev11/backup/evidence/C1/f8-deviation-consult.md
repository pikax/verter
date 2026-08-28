# C1 seventh deviation — F8: wire before moving, not after

Found at the start of phase 4 implementation (scoping-spec.md §4 step 4).
Dispositioned via a fresh Codex xhigh consult per the sixth-deviation
protocol. Full consult prompt/output:
`/tmp/c1-deviation2-consult-prompt.md` / `/tmp/c1-deviation2-consult-output.md`
(not committed — ephemeral scratch; this file is the durable record).

## Finding

Before relocating `project_semantic_dispatch` (146,416 lines / 73 files),
audited it for anything that would block a clean move (the same audit that
caught F7). Two findings:

1. **The choke-point struct's own field type is `ResolverContext`.**
   `ProjectSemanticDispatch<'a> { pub(super) ctx: &'a dyn ResolverContext,
   ... }` (`project_semantic_dispatch/mod.rs:309-311`) — `ResolverContext`
   is F3's carve-out trait that stays in `verter_session`. 94 references
   across 22/73 files. Relocating the struct without first retyping this
   field is impossible: `verter_semantic` cannot name a `verter_session`
   type (that edge doesn't exist and must not), so there is no
   "temporary re-export" that bridges this the way step 3 bridged the
   forward `StoreViewValidationToken` re-export.
2. **A second, deeper leak**: one `ResolverContext` method,
   `host_for_fact_tracer_install(&self) -> &crate::VerterHost`, returns
   `&VerterHost` directly (not an abstracted accessor). 28 call sites across
   10 non-test production files call arbitrary further `VerterHost` methods
   through it (env hashes, project identity, fact-tracer install, and
   more) — a scattered escape hatch, not a single coherent capability.

The spec's own phase order (§4) sequences the `ResolverContext` →
`ResolverObservation` retyping as step 6, AFTER step 4 (relocate
`project_semantic_dispatch`) and step 5 (relocate `resolver_core`). That
produces no compiling intermediate state: steps 4-5 as literally sequenced
first cannot land independently.

## Disposition: ADOPT-NOW

**Verdict: ADOPT-NOW — a phase-order and boundary correction, not a
Fork-4/Abort event.** The host escape proves the existing interface is
unsuitable; it does not prove the required observations are impossible to
express in `AttemptOutcome` terms.

### Corrected steps 4-6 (supersedes scoping-spec.md §4 steps 4-6)

4. **Wire before moving.** Audit the complete relocation closure/SCC
   containing `ProjectSemanticDispatch`; grow `ResolverObservation` to its
   real dependency-neutral surface (per the classification table below —
   NOT a mechanical mirror of `ResolverContext`'s methods); add the
   exhaustive test double and an I/O-free `ResolverAttemptView`; retype the
   to-be-relocated kernel IN PLACE (still physically in `verter_session`)
   from `&dyn ResolverContext` to `&dyn ResolverObservation`; thread
   `AttemptOutcome` through it; eliminate every host/session/workspace
   capability escape (`host_for_fact_tracer_install` disappears entirely —
   see below). Add the session-owned blocking driver that captures an
   attempt view, invokes the kernel, and drives `NeedInputs` before
   retrying.
5. **Relocate already-clean dependency leaves.** Move dependency-neutral
   `resolver_core`/F2 algorithmic slices that already depend only on
   `verter_semantic` types and `ResolverObservation`. No moved production
   function may name `ResolverContext`, `VerterHost`, `HostStoreView`,
   scheduler types, or session lifecycle modules.
6. **Atomically relocate the choke-point SCC.** Move
   `project_semantic_dispatch` and any remaining mutually dependent kernel
   files, repoint callers, delete the old module, and remove temporary
   re-exports in the same cutover. Verify exactly one production
   `ProjectSemanticDispatch`/`SemanticQueryApi` authority remains.

Then continue with the existing step 7 (`ProjectResolver` -> `ModuleResolverCore`).

If the module SCC makes pre-retyping impractical for some piece, steps 4-6
may collapse into one atomic move-and-retype cutover for that piece — they
may not remain independently landable in their original order regardless.

### Concrete dispatcher mechanism

`ProjectSemanticDispatch` keeps dynamic dispatch — `&'a dyn
ResolverObservation`, NOT a generic `<C: ResolverObservation>` parameter and
NOT a blanket adapter over `ResolverContext` (a blanket adapter would
preserve exactly the blocking host-capable escape F3 rejects, and a generic
parameter would spread through the struct's large surface for no benefit).

The session side gets a thin blocking driver/facade: holds `&dyn
ResolverContext`/`HostStoreView`, captures or refreshes a
`ResolverAttemptView`, invokes the one relocated dispatcher, and on
`NeedInputs` performs the permitted blocking load and retries. It owns no
query semantics, graph, memo, or independent resolution branching.

### F7 correction (important)

F7's phrasing — "a distinct session-side driver type... implements it, and
that driver is the one permitted to hold the host reference" — is WRONG as
written: `ResolverObservation`'s seal is private to `verter_semantic`
(`mod sealed { pub trait Sealed {} }`, module-private), so nothing defined
in `verter_session` can implement it at all, host-holding or not. That is
correct per C1-AC-7 ("the interface... cannot be built holding a
host/scheduler reference") but F7 mis-stated the mechanism.

Corrected: **the session blocking driver does NOT implement
`ResolverObservation`.** It constructs and drives a semantic-owned,
dependency-neutral `ResolverAttemptView` type (defined in
`verter_semantic`, implementing the sealed trait) built from data the
driver captured off the host — the driver itself never crosses the seal.

### The real trait surface — classification, not mirroring

| Existing `ResolverContext` access | Correct `ResolverObservation` disposition |
|---|---|
| indexed/prepared/shallow/artifact/hash facts | Narrow observation methods returning `AttemptOutcome<T>` |
| `ensure_indexed_ready_serve` | Nonblocking peek; miss becomes `NeedInputs` |
| env hashes, project identity, config | Immutable attempt inputs or narrow DTO observations |
| `project_type_store()` | Explicit semantic-owned store handles; never the whole session store |
| `resolve_*` operations | Move the resolution ALGORITHM into the kernel; expose the route/probe FACTS it needs, not a semantic operation implemented by the environment |
| fact-tracer installation/observation | Semantic-owned tracing or a returned sidecar; no host accessor |
| ambient dependency recording/telemetry | Attempt output or lifecycle publication, not an immutable observation read |
| workspace enumeration/reverse dependencies | Captured finite snapshot or `NeedInputs` |
| test knobs | Explicit test-only attempt inputs, not a host escape |

`host_for_fact_tracer_install` must DISAPPEAR rather than become an
observation method: its current uses reach env identity, workspace state,
Vue macro resolution, route witnesses, provenance, relation knobs, and
augmentation enumeration — not one coherent observation capability. Each of
its 28 call sites needs its own disposition against the table above.

### Corrected audit counts (supersede my first-pass grep)

- 23 distinct direct `ResolverContext` methods used through confirmed
  resolver receivers across 23 non-test file paths (not my first-pass 19 —
  missing: `observe_materialize_scope`, `prepared_type_decl`,
  `prepared_type_decl_return_only`, `record_ambient_dependency`).
- `host_for_fact_tracer_install()`: 28 call sites across **10** file paths
  (I originally said 13 from an incomplete grep pass — the correct 10 are
  the ones actually enumerated: `mod.rs`, `carrier.rs`, `apparent_type.rs`,
  `call_resolve.rs`, `build.rs`, `cycle_gate.rs`, `flow_return.rs`,
  `locator_shape.rs`, `relation.rs`, `semantic_source.rs`).

### Watch-for triggers for a FURTHER stop (not yet tripped)

- `resolve_vue_macro_surface_with_ctx` (or another escape) turning out to
  be independent query semantics that cannot relocate into the single
  kernel.
- A required path that must block or perform I/O inside
  `ResolverObservation`.
- A miss that cannot name a finite `LoadSet`.
- Session-side lifecycle/publication code needing to retain semantic
  branching rather than merely capture/load/retry.

### Authoritative audit method

Grepping `self.ctx.` is a first-pass inventory, not the final authority —
it misses helper parameters, alternate receiver spellings,
`self.dispatch.ctx`, passed-through context values, and second-order
capability calls after `host_for_fact_tracer_install()`. Order:

1. Reference-search `ProjectSemanticDispatch::ctx` (rust-analyzer
   find-all-references where available; otherwise wide grep sweeps as a
   proxy, cross-checked against compiler errors after retyping).
2. Reference-search each `ResolverContext` trait method, filtered to the
   relocation closure and production `cfg`.
3. Audit every place the context is passed BY VALUE, not just
   method-called.
4. Follow every returned capability transitively — especially host,
   whole-store, session-view, and workspace accessors.
5. Record a call-site disposition table: required fact, semantic owner,
   DTO, missing `InputKey`, side-effect/output disposition, `cfg`.
6. Grow `ResolverObservation` and its exhaustive in-crate test double.
7. Retype in place and let compiler errors expose missed operations.
8. Treat `cargo check` (crate-scoped, per testing policy) as the final
   proof; grep/reference reports are implementation evidence, not a new
   landed scanner.
