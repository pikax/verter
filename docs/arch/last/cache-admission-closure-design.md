# Closing the shared-cache admission poison class — implementer-ready design

**Read this first, and read all of it before you write code.** This is not a summary of a fix that
happened. It is the decided design for work that has **not** been done, and it is the reason the
landed checkpoint is a checkpoint rather than a completed fix. The class described here is **open
and reachable in the landed code**.

Everything in this document was decided by an architecture consult run against the source: two
independent unprimed legs plus a code-verifying decider on the contested points. Where the legs
converged independently, it says so — that convergence is the strongest evidence available for any
of these rulings. The consult's own artefacts lived in an ephemeral scratch directory that will be
destroyed; the substance is reproduced here in full, because a citation to a wiped path is worth
nothing.

## 1. The invariant, and the violation

**The invariant.** A result computed from a read that is *not valid reuse evidence* must never be
admitted to, or served warm from, a shared cache.

**The violation.** Several shared caches admit exactly such results. A concrete, empirically
reproduced instance: on the most ordinary shape in the repository — `defineProps<{ msg: MyStr }>()`
— a member-shape entry derived from an artifact that was **served but never published** lands in
`ShapeCacheDb` and is then served warm. Forcing the exact conditions (an unpublished serve while a
content hash *is* available) turned three separate admission arms red pre-fix and green post-fix.

## 2. Why this class kept coming back — the root-cause account you must not inherit

Three review rounds each closed one admission site, and the next review found another. That is not
thrash; each was a real reachable hole. But the *reason* they kept appearing is that the mental
model everyone was working from — including two written safety rationales in the source — **was
false**, and it is important that you do not inherit it.

**The false account** was: *"non-cacheability folds four reasons, but only the fenced-serve reason
moves the content hash; the other three are content-neutral."* From that, people repeatedly reasoned
"this entry roots on a content hash, therefore a stale read would move the hash, therefore it is
safe."

**Why it cannot be true.** The fact tracer **stores only a boolean and discards the reason**. No
movement information survives the trace at all, so the taxonomy cannot be a *content-movement*
taxonomy in the first place. The consult found this independently on both legs and called it
decisive. The partition also fails on its own terms: a fenced serve fences on content *generation*
or *project* generation and can fire with byte-identical content; the "unobservable source" reason
includes rekey cases that **do** move content; the file-source environment hash (parser version,
language identity) moves with byte-identical content. Only the lease-miss reason is genuinely
content-neutral, which was the one accurate member of the claim.

**The correct model — use this text when you rewrite the rationales:**

> A non-cacheable-read reason means **the consumed value's complete validating basis cannot be
> represented by the enclosing cache entry's recorded facts.** Whether any particular content hash,
> environment hash, generation, or route dimension moved is **incidental**. Content, environment,
> generation, project identity, overlay identity and store-view movement are **orthogonal**
> dimensions. **Rooting alone can never prove admission sound.**

Keep individual statements of the form "*this concrete* lease miss is content-neutral" where the
specific execution establishes it. Delete the taxonomy-wide partition wherever it appears — it was
written into rationale comments in the registry cache producers, the imported-root DB, the route DB
(two places), and the imported-type-root path in `host_manage`. **Every "safe because it roots on a
hash" argument in this area is a category error.** It has now been refuted three times, twice by an
outside reviewer, once by a test — including once from a read-only diagnostic and once from an
adversarial reviewer who had been told to attack precisely that claim and concluded it was safe. It
was not safe.

## 3. What the current mechanism does, and exactly where it stops

The checkpoint introduces `CacheabilityProbe`: a token with a **private field**, a **single
constructor** (reached only from inside `with_cacheability_scope`), and an HRTB that prevents it
escaping its scope. Both review legs confirmed it is genuinely **unforgeable**, and several shared
cache funnels now **require** it as a parameter — so an untraced producer at those funnels is a
**type error**, not a review miss. That much is real and worth keeping.

**It is necessary but not sufficient, and the block must not be read as if it closed the class:**

> The probe proves that **a tracer was active when the funnel was called.** It does **not** prove
> that **the value's compute ran inside that scope.** A caller can compute the value first, and
> *then* open a trivial empty scope purely to obtain a probe, and pass the pre-computed value in.
> The type connects **admission** to a scope; it does not connect the **computation** to that scope.

The closure-shaped funnels are structurally safe because the closure genuinely runs inside the
scope. The funnels that take an **already-computed value** are safe only by *discipline*. And
several funnels take no probe at all.

## 4. The ruled mechanism — invert scope ownership

**The cache OWNER opens the tracing scope. Not the caller.**

Per funnel, the sequence is:

1. Validated warm lookup. (A warm hit pays nothing new — no scope is opened.)
2. Singleflight leadership. (A follower pays nothing new.)
3. **Cold winner only:** open the fact-tracing scope.
4. Run the caller's cold closure **inside** that scope. The caller supplies *only* the computation.
5. Combine, inside the scope: the observed facts; overflow; completeness and provenance;
   generation and store-view revalidation; and the family's signature policy.
6. Mint a **private, sealed, by-value** admission token — call it `Publishable<V>` /
   `AdmissionDecision<T>`, the name is yours.
7. **The storage write consumes only that token.** The closure cannot reach the raw write.

This closes the forge-by-sequencing gap by construction: there is no longer a way to compute
outside the scope, because the owner is what opens the scope and the owner is what calls the
closure.

**Carry the reason by value, typed.** Delete the thread-local refusal-reason bridge (the
set-reason-guard convention and its release-build fallback to "signature overflow"), and widen the
`ReturnOnly` admission arm to carry a typed non-admission reason directly. Stop erasing every
distinct cause into an unstructured boolean — that erasure is the shape of the whole bug.

**Classify propagation.** Distinguish a **local-only refusal** (this layer cannot retain the entry,
but an enclosing compute may still independently and completely root *its own* result) from a
**transitive derivation hazard** (any enclosing compute that consumes this value must also refuse
admission). The four non-cacheable read classes, unrooted results, unresolved provenance, partials,
and torn or superseded values normally require **transitive** propagation. A small enum or bitset in
the scope state; no allocation.

**The two funnels to convert first**, because they already trace and therefore *move* work rather
than adding it: the materialize-structure DB's compute-and-admit funnel and the ref-cycle-result
DB's. Today their tracer bracket lives in the **caller**; move it **into** the funnel. Avoid nested
per-funnel scopes — one owner-controlled outer scope.

**Delete outright (verified zero callers):** `ImportedRootDb::insert_with_facts`
(`crates/verter_session/src/resolver_core/imported_root_db.rs:234`) and the production
`RouteDb::get_or_compute_effective_export_set` funnel. Do not harden a dead API; remove it.

**Compile-confine the raw mutators behind the EXISTING `test-support` cargo feature** — it is
already declared at `crates/verter_session/Cargo.toml:33`, and that file's own comments already
identify `debug_assertions` as an unacceptable production hole:

> **`debug_assertions` is categorically NOT a cache-mutator boundary. An ordinary debug build is a
> production build.**

The surface to confine: `RouteDb::insert_route_with_facts` (`route_db.rs:627`),
`RouteDb::insert_barrel_surface`, the app-config proof funnel, and the `mod tests` / `mod for_tests`
exposures in `lib.rs`. Extend the same treatment to the wider mutation surface both legs enumerated:
the component-meta result DB's `insert`, the owner-import-surface DB's `insert`, the app-config
no-override proof publisher, the semantic graph store's relation insert, and the resolved-import
facts DB's insert-if-absent. And **eliminate the production unvalidated reads**:
`ImportedRootDb::get_any` (`imported_root_db.rs:81`), `RouteDb::get_route_any` (`route_db.rs:330`),
the semantic graph store's unvalidated get, and the component-meta result / owner-import-surface
raw gets.

**Keep two legitimate, non-interchangeable test seams and do not let them blur.** Tests of *real
admission* open a real cacheability scope and go through the real owner funnel. *Synthetic
cache-state fixtures* use an explicitly-named test-only seeding capability that **does not
masquerade as a valid traced admission**. If one seam can do both jobs, a fixture will eventually be
mistaken for a proof.

### The acceptance bar, in one sentence

> Every host-owned shared-cache mutation accepts only **owner-minted admission evidence created
> around the complete cold computation**; all raw value/fact/carrier mutations and all unvalidated
> production reads are absent from production-visible types; test seeds are compile-confined.

### A debt row here was explicitly ruled NOT justified

Both consult legs refused one, and the reason generalises: the project's own deferral record
requires a justification of the form "no compiler or structural mechanism can express this
invariant", and here a structural mechanism **demonstrably does** exist — so no honest justification
can be written.

If some component is nonetheless deferred, the deferral is valid **only** with all three of:

1. a **typed executable refusal arm** selected **before** any cache, reverse-index, backfill, or
   retained-singleflight write, incapable of calling any mutator, returning the value with a typed
   reason;
2. a **discriminating test** proving the caller still receives the value, that no warm entry is
   created, that no reverse-index entry is created, that a second call recomputes, and that a
   follower cannot adopt the unadmitted value;
3. a **compile-fail test** proving foreign production code cannot reach a raw write.

A prose row, a debug assertion, or "currently zero callers" is **not** a fail-closed mechanism. A
debt row that leaves an existing public insert operational has no fail-closed at all.

## 5. Port the stronger type — `codex/bugb-independent` @ `4cc13cfbb`

An independent solve of this same bug, written from the same base (`44d2a7528`) without sight of the
first attempt, converged on the same probe (same name, same signature position — the probe is *not*
the novel part) but is **strictly better on the one axis that matters**. Branch and worktree are
preserved. Verified first-hand: `crates/verter_session/src/resolver_core/fact_read_set.rs:267` on
that branch declares

```rust
pub enum FactReadSetFinalise {
    Ok(Arc<[FactVersionRef]>),
    NonCacheable(Arc<[FactVersionRef]>),
    Overflow,
}
```

The boolean is **removed** from tracer installation, and an exhaustive, wildcard-free match forces
every consumer to route `NonCacheable` into an unresolved-provenance refusal and `Overflow` into a
signature-overflow refusal. The consequence is the point:

> **A consumer cannot obtain clean facts from a non-cacheable compute.** Non-cacheability becomes
> **intrinsic to the type**, not a flag travelling beside it.

The checkpoint's own enum (`fact_read_set.rs:262` on the implementation lane) has only `Ok` and
`Overflow`; non-cacheability rides **alongside** the facts as a separate boolean. **That droppable
boolean is the root-cause shape of this entire class** — every hole in the register below is some
layer dropping exactly that kind of signal. Being fair to the checkpoint: its three production
callers *do* bind the boolean today, so it is not broken by this. The defect is that **the type
permits dropping it.**

**Port the three-variant type.** It is a compiler-enforced kill of the root cause and it costs
almost nothing.

## 6. The headline deliverable: a systematic AUDIT, not more patches

**Patching sites has now failed three times.** Each round closed the site the last review found, and
each next review found another — including two live caches (`ImportedRegistryDb`,
`DeclarationLookupDb`) that no brief had named, and one proven live poison
(`OwnerCollectionDb`) that a "safe by rooting" argument had cleared.

The decider stated plainly, having found two further live exposures nobody had asked it about:
**the hole set is NOT exhaustive**, and exhaustiveness cannot be proven while mutation remains
decentralised.

So the remaining deliverable is not a list of fixes. It is:

> **Enumerate EVERY shared-cache producer, and prove for each one that it either (a) takes the
> owner-minted admission capability, or (b) is structurally incapable of admitting.** Then remove
> every raw shared-cache publisher from production-visible types, so that (b) is decidable by the
> compiler rather than by reading.

Do not accept a per-site fix as closing this. Do not accept "these are all of them" from any source,
including this document.

## 7. The four known LIVE-PRODUCTION holes

These are the ones the decider adjudicated line-by-line against source. They are **anchors, not an
inventory** — see §6. Line numbers are as cited at the implementation-lane tip and will drift; the
paths and symbols are the durable part, and I verified each path and symbol exists.

**(i) The symbol cache is served through an accept-all store view, on a live path, and canonical
invalidation never clears it.** `SymbolResolverState::resolve_node`
(`crates/verter_session/src/resolver_core/symbol_resolver.rs:115`) treats "the facts are non-empty"
as the *entire* admission decision — no probe, no completeness, no supersession — and its key
(`canonical#name`) carries no content version, no view identity and no environment generation.
`PermissiveStoreView::validates()` returns `true` **unconditionally**
(`crates/verter_session/src/resolver_core/mod.rs:463`). The production JSDoc caller
(`crates/verter_session/src/host_manage/jsdoc_resolve.rs:572` mints exactly that view) is **not**
cfg-gated and is reachable from the component-meta resolver, the host and session resolver contexts,
the external-type fallback, and the component-meta registry producers. The eviction paths
(`evict_canonical` / `hard_evict` / `invalidate_canonical` in the resolver runtime) **omit this
cache**. Failure mode: after a same-name declaration edit, the stale resolved declaration (old span,
kind, decl-id, text) validates **forever** and is **laundered into outer fact-tracked caches** — the
outer request observes the *current* file hash while the inner cache hands back the *stale*
declaration.

**(ii) The fallthrough store validates forever, because its key carries no content hash.**
`resolver_core/fallthrough_resolver.rs::store_node` (`:193`) checks key cacheability and partiality,
but not taint, overflow, provenance or supersession. Its writes happen **during** the cold compute,
and the top-level stability fence runs later and **does not retract nested nodes already inserted**.
Worse than first alleged: consumed-bindings are stored with **empty facts**, under a key with **no
content hash and no generation** (the branch key is just the branch index) — and the validated fact
cache validates by "all facts still valid", so **empty facts validate vacuously, forever**. The
decider gave a concrete reproducing edit: change a spread binding's resolved keys without changing
the branch index, and the old consumed-bindings node is served for ever.

> **This hole is also where the checkpoint introduced a regression of its own, and it is the one
> thing the checkpoint had to fix.** A single function used to do two things at once — mark a
> request cache-suppressed **and** fold the result to partial. Decoupling those is architecturally
> **correct** (a fenced serve should not make a result *partial*), and both independent attempts
> deleted all of its call sites. But the replacement fan-out marks only the thread-local tracers,
> and **no fallthrough file takes a probe** — so `store_node`, whose admission gate reads only "is
> the cold compute partial?", started seeing `Complete` and **admitting the poison**, with a comment
> still describing a rail that no longer existed. Both implementations' `store_node` were
> byte-identical here: a **shared blind spot, not a differentiator.** Whatever the checkpoint did
> about this, re-derive it — and mutation-verify it.

**(iii) A scalar lane deliberately publishes partial results, against its own no-poison contract.**
In `crates/verter_session/src/resolver_core/component_meta_request.rs`, partial results are refused
**only** when a fixed store view is supplied; the scalar lane deliberately falls through (its own
comments say so), and the host's `resolve_component_meta` passes no view. The store-cached
resolved-meta write never inspects completeness, and **unconditionally mirrors into the legacy cache
even when strict-cache admission declined the candidate**. Failure mode: a budget-exhausted,
fatal, or incomplete component-meta result is **warm-replayed instead of retried and healed** —
which directly contradicts the no-poison contract stated in the resolved-state module a few files
away.

**(iv) Raw cache mutators are `#[doc(hidden)]` only — no cfg gate — and ship in RELEASE.** Verified
first-hand on the landing branch: `crates/verter_session/src/semantic_query_memo/mod.rs:3581-3582`
carries `#[doc(hidden)] pub fn publish_with_carrier_for_tests(...)` with **no `cfg` at all**, and
the live store is reachable through `ProjectTypeStore`'s semantic-graph accessor. Its dispatch and
generation sibling variants and the materialized-result publisher are the same. Separately,
`lib.rs` exposes `pub mod tests` / `pub mod for_tests` under `cfg(any(test, debug_assertions))` —
which, again, **is not a production boundary**. And the public route and imported-root raw surfaces
reachable through `ProjectTypeStore` (`RouteDb::get_route_any` `route_db.rs:330`,
`insert_route_with_facts` `:627`, `ImportedRootDb::get_any` `imported_root_db.rs:81`,
`insert_with_facts` `:234`) all compile in **release**, all operate on the host's shared DBs, and
all permit permissive reads or caller-supplied loose facts with no probe.

**One further hole is LATENT, not live, and the distinction was checked:** a leader that
manufactures empty facts classifies its own result unadmitted and then returns **without** fanning
the refusal, so an enclosing probe stays clean and a parent can warm-admit a result derived from an
unrooted root. Structurally unsound and worth deleting — but the decider enumerated every direct
caller of those facts-less entry points and found them all test-only. The **shape**, however, is
live one level up, in the named-type-export route entry: its normal exit can return **empty facts
with no mark** when every participant fails both hash oracles. Close it at the funnel floor.

## 8. One guard must be deleted, not extended

While hardening this area, an anti-vacuity pin was added to a family-A fact-validation guard: four
literal spelled-out function-name searches. It is strictly *stronger* than what it replaced — and it
is **still wrong to land**, because it **broadens a name-keyed source scanner**, which this
project's landed-guard bar forbids (a landed guard is structural — compiler, type system, privacy,
sealing — never a name/text/grep scanner; pre-existing ones are grandfathered as-is but may not
grow).

The ruling was **delete the guard and replace it with compiler-enforced admission evidence**, and
the load-bearing fact that settled it is worth knowing: **the function the guard's negative half
searches for does not exist anywhere in the repository** — a call to it would already be `E0425`.
The guard's negative half is *already dead*, and what the guard actually decides is that three files
do or do not contain particular character sequences. It never establishes that a producer *calls*
the helper, that the call *resolves* to that item, that the helper's result *reaches the cache
write*, or that the key *matches* the computed value.

The replacement is part of the same work as §4: make the two signature helpers return **opaque,
policy-specific evidence types** with private fields and private constructors, carrying key identity
rather than a bare signature; make the funnels accept **only** that evidence (so "right helper,
wrong canonical/name" is also blocked); expose **no** conversion from a raw fact-ref slice into the
evidence type; and then **delete the scanner entirely**, including its stale reference to the
nonexistent helper. Retain and extend the behavioural rooting suite — stale-versus-current canonical
rooting, syntactic export-set rooting, member-shape changes, unrelated-sibling warm survival,
materialize provenance, mid-compute races — because a structural evidence type cannot prove the
helper's *internal algorithm* picks semantically correct facts, nor race behaviour. Those are
behavioural and stay behavioural. The residue left uncovered by any landed scanner — exact function
naming, file placement, literal spelling — is **acceptable uncovered**: both legs agreed it is not
an architectural invariant at all.

## 9. How to know you have actually fixed something

Two rules, both learned the hard way in this area, both non-negotiable:

- **Settle reachability empirically, never by opinion.** A static "this is safe because it roots on
  a hash" argument has been wrong **three times** here, including once from a read-only diagnostic
  and once from an adversarial reviewer specifically told to attack it. The only thing that ever
  settled these questions was a test that goes **red** against the pre-fix tree. When two reviewers
  disagree about whether a path is reachable, **do not get a third opinion — write the test.**
- **Mutate your fix and watch the test go red.** A zero-coverage "fix" was caught in this very area
  by exactly this check: reverting the headline predicate left the whole suite **green**, because
  the test pinned a neighbouring loop rather than the retention it claimed to pin — while its doc
  comment explicitly claimed otherwise. A passing suite is not proof that a fix does anything.
