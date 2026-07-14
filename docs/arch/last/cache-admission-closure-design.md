# Closing the shared-cache admission poison class — implementer-ready design

## 0. STOP — the landed commit ships a cache-poison REGRESSION, and fixing it is job one

**This work introduced a cache-poisoning regression that is present in the landed code. The base did
not have it. This branch must not be merged onward until it is fixed.** That distinction — regression,
not inherited debt — is the whole point of this section, and it is the first thing you should act on.
Not the type change in §5. This.

**The mechanism, exactly.** The fallthrough resolver's admission funnel,
`store_node` in `crates/verter_session/src/resolver_core/fallthrough_resolver.rs` (around line 193),
gates admission on exactly three things:

1. `key.is_cacheable()`,
2. `crate::request_context::current_cold_compute_completeness().is_partial()`, and
3. `!result.facts.is_empty()` (or an intrinsic-surface / consumed-bindings value).

**It has no non-cacheability rail at all.** Verify it in one command — the file contains **zero**
occurrences of any of them:

```bash
grep -cE "non_cacheable|CacheabilityProbe|with_cacheability_scope" \
  crates/verter_session/src/resolver_core/fallthrough_resolver.rs   # ⇒ 0
```

**And the file never changed.** It is byte-identical to its base. What changed is *underneath* it:
this lineage deleted the roughly **31 call sites that folded non-cacheability into cold-compute
completeness**. Decoupling those two concerns was architecturally **correct** — a fenced serve should
not make a result *partial* — but `store_node`'s only safety gate was that very completeness signal.
Removing the fold **rendered its gate toothless**. Its comment still claims a "single no-poison rail
shared with the component-meta materialiser"; that rail no longer carries non-cacheability.

**The consequence:** a fallthrough node computed through a fenced serve or a lease miss, carrying
non-empty **live-rooted** facts, is admitted and served warm **indefinitely**. Live-rooted is the
sting — the facts validate against the current view on every warm hit, so the read-side rail can never
reject it (see §2 for why "it roots on a live hash, therefore it is safe" is a category error).

**The honest caveat, and you must hold both halves of it.** The *rail* is **proven absent** — that is
a blob-hash fact, not an argument. But **nobody constructed an end-to-end poisoning trace through
`store_node`.** So what is established is a **proven-missing safety rail**, not a demonstrated
exploit. Do not overstate it, and do not let anyone talk you out of it either: **settle it with a
discriminating test, not with another opinion.** Static "this path is safe" reasoning has been wrong
**three times** in this work — including from a read-only diagnostic and from an adversarial reviewer
told to attack that exact claim (§2, §9). Force a fallthrough node through a fenced serve or a lease
miss, assert the entry is refused admission, and watch the test go **red** against the tree as it
stands.

---

**The rest of this document is the decided design for work that has not been done.** It is why the
landed checkpoint is a checkpoint rather than a completed fix. The class described here is **open and
reachable in the landed code** — the regression above is the most urgent instance, not the whole of it.

Everything in this document was decided by an architecture consult run against the source: two
independent unprimed legs plus a code-verifying decider on the contested points. Where the legs
converged independently, it says so — that convergence is the strongest evidence available for any of
these rulings.

**The consult's own transcripts no longer exist**, along with every other scratch artefact from this
effort. That is precisely why this document reproduces the substance in full rather than citing it:
a pointer to a wiped path is worth nothing, so there are no pointers here. Everything you need to
implement this is in the prose below, and every source citation resolves against the committed
repository in front of you.

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

## 5. Make non-cacheability INTRINSIC TO THE TYPE — full specification

This is the single highest-value change in this document, and it is small. It kills the root-cause
*shape* of the entire class with the compiler, rather than closing one more site.

**You will build this from this specification.** An independent implementation of exactly this design
was written and verified — it passed the full workspace gate (all tests, zero failures, both surfaces),
added **zero** new clippy errors, and its headline test **survived mutation** (reverting the predicate
turned the test red; restoring it turned it green). So this is **known-viable, not speculative**. But
that implementation was never pushed and **no longer exists** — there is no diff to copy, no branch to
fetch. Rebuild it from the prose. It is roughly a day's work and it is worth it.

### The defect, visible in the committed code today

Go and read `crates/verter_session/src/resolver_core/fact_read_set.rs`. The finalise result is a
two-variant enum:

```rust
pub enum FactReadSetFinalise {
    Ok(Arc<[FactVersionRef]>),   // sealed, sorted, deduplicated signature
    Overflow,                     // exceeded FACT_SIGNATURE_CAP; refuse admission
}
```

Now read `install_fact_tracer` in `crates/verter_session/src/fact_signature_helpers.rs`. Its signature
is the bug:

```rust
pub(crate) fn install_fact_tracer<F, R>(host: &VerterHost, f: F)
    -> (R, FactReadSetFinalise, bool)   // ← the third element is `fenced_serve_observed`
```

**The facts and the non-cacheability verdict travel as separate values, and the verdict is a bare
`bool` a caller can simply not bind.** Then read `SignatureAdmission::from_finalise` in
`crates/verter_session/src/cache_runtime/admission.rs`:

```rust
pub(crate) fn from_finalise(finalise: FactReadSetFinalise) -> Self {
    match finalise {
        FactReadSetFinalise::Ok(facts) => SignatureAdmission::Cacheable(ReadSetSignature::new(facts)),
        FactReadSetFinalise::Overflow  => SignatureAdmission::NonCacheable(NonAdmissionReason::SignatureOverflow),
    }
}
```

**It never sees the boolean.** It cannot: the boolean is not part of its input type. So a compute that
consumed a non-cacheable read finalises as `Ok(facts)` and lifts to `Cacheable` — clean facts from a
dirty compute, and every caller of `from_finalise` inherits the hole for free. That is not a missing
check at one site. **That is the class**, expressed as a type, and every individual hole in §7 is some
layer dropping exactly this kind of signal.

**Scale, on the landed tree — this is why the change is not cosmetic.** Roughly **twenty admission
sites can structurally obtain clean facts from a non-cacheable compute simply by not reading the third
tuple element.** They mostly do read it today; nothing makes them. And the forward-looking half is
worse than the backward-looking half:

> **Every future admission site added to this code is one dropped `_` away from re-introducing the
> entire bug class.**

A reviewer cannot reliably catch a *missing* binding — three review rounds in this very area each
missed one. A compiler catches it every time, for free, forever. That asymmetry is the entire argument
for doing this.

### The fix

**Fold the verdict into the finalise result and delete the boolean.**

```rust
pub enum FactReadSetFinalise {
    /// Sealed and clean: this signature may authorize a shared-cache admission.
    Ok(Arc<[FactVersionRef]>),
    /// The observations are complete and may be bubbled into an enclosing tracer,
    /// but the compute ALSO consumed a non-cacheable read. The value may be returned
    /// to this caller; these facts must NEVER authorize a shared-cache admission.
    NonCacheable(Arc<[FactVersionRef]>),
    /// Signature exceeded FACT_SIGNATURE_CAP. No partial signature is returned.
    Overflow,
}
```

The `NonCacheable` arm **still carries the facts**, and that is deliberate — an enclosing tracer may
legitimately need the observations even though *this* layer may not publish. What it must never do is
be mistakable for `Ok`.

Then:

1. **`FactReadSet::finalise`** returns `NonCacheable(facts)` whenever the read set observed a
   non-cacheable read, `Overflow` on cap exceedance, and `Ok(facts)` otherwise. The precedence is
   `Overflow` > `NonCacheable` > `Ok` (an overflowed signature is unusable regardless).
2. **`install_fact_tracer` drops its third return value**: its type becomes
   `(R, FactReadSetFinalise)`. The `fenced_serve_observed` boolean — and any sibling non-cacheability
   boolean threaded beside the facts — **ceases to exist as a separate value**. This is the load-bearing
   step: there is no longer a flag to forget.
3. **`SignatureAdmission::from_finalise` becomes an exhaustive, WILDCARD-FREE match** over all three
   arms, failing closed:
   - `Ok(facts)` → `Cacheable(ReadSetSignature::new(facts))`
   - `NonCacheable(_)` → `NonCacheable(NonAdmissionReason::UnresolvedProvenance)` — refuse the write,
     **return the value to the caller**
   - `Overflow` → `NonCacheable(NonAdmissionReason::SignatureOverflow)`

   **Never a `_ =>` arm.** The wildcard is what would let a future fourth reason silently become
   cacheable; its absence is what makes the compiler your auditor when someone adds one.
4. **Fix the resulting compile errors — that list IS your audit.** Every site that destructured the old
   3-tuple or matched the old 2-variant enum now fails to compile, and each failure is a place that was
   free to ignore non-cacheability. In the committed tree the production tracer installations are in:
   `component_meta_materialize.rs`, `component_meta_caches.rs` (two sites), `host_manage/prepared_decl.rs`,
   `framework/script_facts.rs`, `project_semantic_dispatch/mod.rs` (two sites),
   `project_semantic_dispatch/relation.rs`, `typeinfo/framework_surface/svelte_exec.rs`, and
   `typeinfo/framework_surface/vue_exec/mod.rs`; the `from_finalise` consumers are in
   `cache_runtime/node.rs`, `framework/script_facts.rs` and `host_resolve/virtual_file_pipeline.rs`.
   Re-derive that list with `grep -rn "install_fact_tracer(\|from_finalise" crates/verter_session/src/`
   rather than trusting it — it will have drifted.
5. **Do not add a convenience accessor** like `fn facts(&self) -> Option<&[FactVersionRef]>` that
   returns the facts for both `Ok` and `NonCacheable`. That would restore the exact hole you are
   closing, in a friendlier costume.

### Why this is the right shape

> **A consumer cannot obtain clean facts from a non-cacheable compute.** Non-cacheability stops being
> a flag that travels *beside* the evidence and becomes a property *of* the evidence.

The checkpoint's probe (§3) connects *admission* to a scope. This connects the *evidence* to its own
validity. They are complementary, and neither substitutes for the other — but this one is what makes
the compiler refuse the mistake, and it is the reason the independent implementation was judged better
on the axis that matters. Being fair to what landed: its production callers *do* bind the boolean
today, so the checkpoint is not broken by this. The defect is that **the type permits dropping it**,
and this class has now demonstrated three times that anything the type permits, someone eventually
does.

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

> **⚠ This is also the site of the REGRESSION THIS WORK INTRODUCED, and it SHIPS UNFIXED in the
> landed commit — see §0, which is your first job.** The pre-existing weaknesses above (no content
> hash in the key, vacuous empty-fact validation) are inherited debt. The *regression* is separate and
> newer: this lineage deleted the ~31 call sites that folded non-cacheability into cold-compute
> completeness, which **rendered `store_node`'s completeness gate toothless** — so a node computed
> through a fenced serve or a lease miss, carrying non-empty live-rooted facts, is now admitted and
> served warm. The base did not have this. Fix §0 before you touch anything else in this document.

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
