---
name: signature-kernel
description: "Verter semantic signature kernel — signature records/descriptors, epoch-safe interned storage and retirement, request-pinned borrowed reads, positional matching, call substitution, ordered union/intersection reduction, VerterStableV1 deterministic ordering, the observation corpus and the determinism matrix"
---

# Semantic Signature Kernel

The one authority for **what a callable is**, **how two types are compared**, and
**what order semantic composites are published in**. Every signature set, every
intersection/union reduction, and every deterministic ordering decision in the
session crate comes from here; there is no second signature producer and no
per-consumer ordering rule.

Normative contract: [`docs/arch/signature-kernel.md`](../../../docs/arch/signature-kernel.md)
(revision 4.1, byte-locked by `docs/evidence/signature-kernel/manifest.json`).
Where this skill and the contract disagree, the contract wins.

---

## 1. Module map

`crates/verter_session/src/signature_kernel/` (crate-private; its single
consumer is `project_semantic_dispatch/signature_discovery.rs`):

| Module | Owns |
|---|---|
| `records.rs` | The record vocabulary and its handles: `SignatureDescriptor`, `SignatureCandidate`, `SignatureTemplate`, `SignatureInputShape`, `ParameterLayout`/`ParameterSlot`/`RestSlot`, `BinderSpace`/`BinderDeclaration`, `SignatureResultRecipe`, `PredicateEffect` (the effect half of a result read: a declared result's type predicate / assertion, the predicate the checker infers from a body, or a union signature's composite predicate), `AppliedResult`, `TypeToken`, `GraphEpoch`. Every id is epoch-qualified. |
| `storage.rs` | `AppendInterner<T>` — the private append-only interner over `boxcar::Vec` with `DEDUP_SHARDS` (16) hash shards. Hash is computed outside the shard lock; equality decides collisions. A record is fully initialised before its handle is published, so `boxcar`'s `count()` is never a published-handle range. |
| `lifetime.rs` | `SignatureStore`: interning entry points, `replace_epoch()`, `retain_result`/`drain_retained`/`retained_len`, `live_reader_count()`, `StoreError`. |
| `read_view.rs` | `SemanticReadView::pin(&SignatureStore)` — the request-pinned borrowed read. `BorrowedSet::{Empty, One, Many}`, `ReadError`, plus the `descriptor_chain_walks` / `shard_lock_acquires` probes the performance gates assert on. |
| `positional.rs` | The shared positional model: `PositionalShape`, `PositionalMode`, `TypeAt`, `MinArityFlags`, `ProjectedTuple`/`ProjectedElement`, `SlotTypeFacts`. Parameter matching, rest/receiver layout and arity are computed **once** here for every consumer. |
| `provenance.rs` | `SignatureProvenance`, `OverloadOrder`, `ArmIdentity`, `ConstituentSequence`, `DeclarationGroupId`, `MappedConstituent`, `OriginRelation`, `SourceLocatorId` — where a candidate came from and in what authored order. |
| `substitution.rs` | `CallSubstitution`, `SubstTerm`, `compose_canonical`, `MAX_SUBSTITUTION_CHAIN_DEPTH`. The two substitution stages (declared/outer map and frozen call-site map) compose canonically; a hand-built chain past the bound flattens rather than growing. |
| `discovery.rs` | `publish_signature`, `set_from_candidates`, `append_signatures`, `union_signatures`, `intersection_signatures`, `heritage_signatures`, `signatures_identical`, `DiscoveryError`. |
| `result.rs` | Demand-driven results: `ResultDemand`, `ReadSignatureResultKey`, `SignatureSetValue`, `SignatureResultValue`, `SignatureCandidateNodes`. |

Ordering and composite reduction live beside the kernel, in the dispatch crate:

| Path | Owns |
|---|---|
| `semantic_query/stable_key.rs` | `VerterStableV1` encoding and `StableKey::cmp`. |
| `project_semantic_dispatch/canonical_algebra.rs` | `intern_ordered_union` / `intern_ordered_intersection` — the two crate-private canonical builders, plus `compare_structural` and `CanonicalEvidence`. |
| `project_semantic_dispatch/build.rs` | `build_reduce_union` / `build_reduce_intersection` — the query builders behind `SemanticQueryKey::ReduceUnion` / `ReduceIntersection`. |
| `project_semantic_dispatch/signature_discovery.rs` | The cutover consumer: it drives discovery, positional matching and instantiation through the kernel records. |

---

## 2. Ordered reduction is ONE pair of queries

Union and intersection construction is closed over **exactly two** query keys
and **exactly two** crate-private builders:

```text
SemanticQueryKey::ReduceUnion { members, nullability }      → build_reduce_union
SemanticQueryKey::ReduceIntersection { input, purpose, ctx } → build_reduce_intersection
        ↓                                                           ↓
canonical_algebra::intern_ordered_union          canonical_algebra::intern_ordered_intersection
```

Both builders perform recursive same-kind flattening, the lattice absorption
laws, literal subsumption on unions, structural `T | T = T` / `T & T = T`
through `compare_structural`, and proven-disjoint scalar collapse to `never` —
an undecided relation is never guessed. Both thread `CanonicalEvidence` to
`deposit_canonical_evidence`; an incomplete comparison sets `cache_suppress`
(`ReturnOnly`, never a warm canonical result).

A union is built under an explicit `NullabilityPolicy` — `strictNullChecks`
as construction input. `Erased` (the option off) drops `null` / `undefined`
beside any other member, as the checker's `getUnionType` does, and a
nullable-only list becomes `null` if it names `null`, else `undefined`. The
policy is family identity on `ReduceUnion` and is recorded on the canonical
stamp (`CompositeOriginCategory::Canonical(NullabilityPolicy)`), so the two
settings never share a memo entry, a node, or the pre-seal skip.
Intersections run the strict algebra.

`ProjectSemanticDispatch::intern_normalized_union_or_intersection` is the
dispatch-level funnel every flow/meta-resolve/locator producer reaches these
through; its unions run the strict algebra. The flow-return evaluator, which
answers under its function's own project policy, constructs through
`intern_normalized_union` with the frame's policy instead. Two composite
constructions are deliberately outside the funnel, both
`CompositeList::ordered_carrier` mints, and both for the same reason — an
ordered carrier is an authored sequence, not a commutative intersection, and
routing it through the reducer would change the origin category and therefore
node identity:

1. same-name method **overload groups** (`walk.rs`, `build.rs`);
2. a **possibly-callable member-value intersection** (`walk.rs`
   `merge_value_nodes_recursive`). Call resolution over an intersection tries
   arms in declaration order, so a commutative sort would break overload
   precedence. The classification (`value_may_contribute_call_signatures`)
   fails CLOSED on anything undecidable from the graph alone, and callable
   merging itself belongs to `SignaturesOfType` →
   `signature_kernel::discovery::intersection_signatures`, never to a local
   concatenation of signature nodes (§15).

An interface/class declaration body with heritage is a third construction
outside the funnel, the `CompositeList::heritage` mint (§6, rule 8): it is a
declaration, not a commutative intersection, and is never re-decided.

**Retired spellings.** `NormalizeUnion`, `NormalizeIntersection`,
`SemanticMeet`, `canonical_intersection`,
`build_normalize_union` and `UnionSelected` are retired names. Each is an entry
in `RETIRED_SYMBOLS`
(`crates/verter_session/tests/cases/g_misc0/no_legacy_walker.rs`), enforced by
`retired_symbols_absent_from_production_source`. Re-introducing one resurrects a
second composite-construction authority beside the ordered reduction pair, which
is exactly how two producers start ordering arms differently.

---

## 3. Determinism: `VerterStableV1`

`StableKey::cmp` compares `(fingerprint, exact)` — the FNV-1a hash of the exact
key bytes FIRST, the exact bytes only on collision. Consequences worth keeping
in mind:

* Canonical union member order is **fingerprint order**, not authored order and
  not lexicographic order. `Extract<'a'|'b'|'c', 'a'|'b'>` renders `"b" | "a"`.
* An authored-order display pin in an older test is an arena-id-sort
  coincidence (arena id == lowering order), not a contract.
* `intern_ordered_union` interns with ORDER-SENSITIVE identity
  (`CompositeMembers::eq` compares the member slice), so `[a,b]` and `[b,a]` are
  distinct nodes — there is no set-collision first-wins there.

Every `SemanticNodeData` variant has a `VerterStableV1` encoding. The
registration table `STABLE_KEY_TABLE` in
`crates/verter_session/tests/cases/g_block/semantic_determinism_matrix.rs`
enumerates the live variant set exactly (guard:
`stable_key_table_enumerates_every_semantic_node_data_category`) and carries a
`residual` column naming, per variant, any identity input the encoder currently
**approximates** rather than consumes.

---

## 4. Epoch-safe storage and the lifetime contract

* Handles are epoch-qualified. A handle minted in a retired epoch is rejected,
  not silently re-read (`stale_epoch_handle_is_rejected`).
* `replace_epoch()` installs a new graph epoch. A reader pinned before the
  replacement finishes against **its** epoch
  (`old_pinned_reader_finishes_against_its_epoch`,
  `pinned_view_reads_substitution_after_epoch_replacement`).
* Live readers are roots until drop (`live_readers_are_roots_until_drop`,
  `live_reader_count_includes_pinned_retired_epoch`).
* Retained results outlive an epoch replacement until drained
  (`retained_results_outlive_epoch_replacement_until_drained`).
* Interning rejects stale embedded handles across a replacement
  (`intern_rejects_stale_embedded_handles_across_epoch_replacement`), and a
  result lookup MISS never publishes (`lookup_result_does_not_publish_on_miss`).

The kernel store does **not** own the process byte budget: aggregate retention
is `SemanticRetentionAccount` (see `/type-cache-architecture` → Aggregate
retention account). A retained parse snapshot is a `Pinned` charge — charged
unconditionally, never refused.

---

## 5. Evidence

`docs/evidence/signature-kernel/`:

| File | Role |
|---|---|
| `manifest.json` | Contract byte-lock, pinned oracle identity (TypeScript 7.0.2 + per-platform toolchain digests), corpus identity and observation digest, generator versions. |
| `semantic-difference-ledger.md` | The four-class release authority of §5.8. Every recorded difference is classified: exact agreement, presentation-only, `VerterStableV1` order-induced (causal proof required), or independent semantic difference / incompleteness. A row never silently disappears. |
| `determinism-matrix.md` | Per-row status of the §5.9 perturbation matrix and what keeps a row undrivable. |
| `performance-gates.md` | The §12 structural gates, the executable guard for each, and the 5% regression investigation policy. |

Executable homes:

| Home | Role |
|---|---|
| `crates/verter_session/src/signature_corpus_rows_tests.rs` | THE 26-row observation corpus (`verter-signature-corpus-v0@typescript-7.0.2`). Append-only: adding a row is one `Row` literal. Each row records the checker print, the `--declaration --emitDeclarationOnly` bytes, and the implementation's `Verdict`. |
| `crates/verter_session/src/signature_corpus_tests.rs` | The corpus driver and the **flip law**: a `MatchesChecker` row fails when the live answer stops matching, and an owed/degraded row fails when the live answer STARTS matching. Both directions are proven by `signature_corpus_flip_law_fires_in_both_directions`. A verdict can only move by a deliberate re-pin. |
| `crates/verter_session/tests/cases/g_block/semantic_determinism_matrix.rs` | The §5.9 perturbation matrix and the §5.4 stable-key table, each enumerated against its authority and consumed by replay drivers. Every comparison runs on TWO bases: the stable-text completed observation AND the generated-bytes digest. |
| `crates/verter_session/tests/allocator_canaries.rs` | `signature_kernel_warm_positional::warm_positional_read_does_not_allocate_or_lock` — the §12 Empty/One gate, in a separate test binary because it installs a counting `#[global_allocator]`. |
| `crates/verter_session/src/signature_kernel/*_tests.rs` | Per-module unit coverage (lifetime, storage, substitution, positional, provenance, discovery, read view). |

**Re-locking the contract digest.** `docs/arch/signature-kernel.md` is byte-locked
by `manifest.json` → `contract.sha256`, checked by
`typeinfo::oracle_core::identity::tests::evidence_manifest_digests_reproduce_from_checked_in_inputs`.
If the contract bytes change intentionally, re-lock the digest in the same
change; never weaken the test.

---

## 6. Working rules

1. **One signature producer.** A new consumer that needs callable shape goes
   through `signature_discovery`, never through its own walker over
   `SemanticNodeData::Signature`.
2. **One ordering rule.** Rendering, display, and comparison all read the same
   `VerterStableV1` order. A consumer that sorts arms itself is a defect.
3. **Two builders, both private.** Raw interning stays crate-private behind
   `ReduceUnion` / `ReduceIntersection`. A thin adapter may remain only when it
   makes no semantic decision.
4. **Never game a determinism test.** Serialising node allocation to make a
   replay agree is explicitly forbidden by §12. Fix the ordering input instead.
5. **A typed gap is not a fast success.** Do not compare a partial Verter query
   to a complete TypeScript project check and label the ratio a speedup.
6. **A corpus verdict moves by re-pin only.** Change the row literal in the same
   change that changes the answer, and move the matching ledger row with it.
7. **Semantic decisions read `SignaturesOfType`; representation may read the
   surface.** A reader whose answer depends on which signatures a type HAS —
   callability, callable anchoring, runtime classification, overload choice,
   applicability — asks discovery (`shared_signature_nodes` /
   `shared_signature_buckets`), as the apparent-type anchor and the broad
   runtime classifier do. Rendering, serialization, hashing, traversal and
   surface carriage read an object's `call_signatures` / `construct_signatures`
   directly. That split is sound only because every list on an interned object
   is either the object's own authored list or discovery's answer for the
   composite it was merged from: the shallow intersection merge keeps arm
   members, but takes its call/construct entries from `SignaturesOfType` for
   the intersection (`with_discovered_signatures`), never from its own
   identity-deduplicated concatenation. A new merge that interns an object
   carrying signatures must do the same, or it creates a second signature
   authority.
8. **A declaration body with heritage is not an intersection type.** An
   interface/class body with `extends` is an intersection NODE (bases in
   clause order, own body LAST) minted `CompositeList::heritage`
   (`CompositeOriginCategory::Heritage`; the single-declaration projection
   and the merged-declaration reducer mint it, and every order-preserving
   rebuild keeps it through `CompositeList::rebuilt_from`). `SignaturesOfType`
   reads the category and answers it with
   `signature_kernel::discovery::heritage_signatures` — own signatures first,
   then each base's in clause order, no identical-signature dedup, no mixin
   composition (TypeScript's `resolveObjectTypeMembers`) — so call
   resolution, the signature utilities, the relation engine and the shallow
   walker's heritage flush all agree, through an alias of the declaration and
   after instantiation too. Because the category is part of node identity,
   the body never shares a node with the authored `Base & { … }` over the
   same arms.

## Related skills

`/type-resolution` (query modes, macro traversal, the five-mode dispatch),
`/type-cache-architecture` (key composition, candidate substrate, retention
account), `/component-meta` (publication surface), `/audit-infrastructure`
(`ReduceUnion` / `ReduceIntersection` work sites and origin-graph kinds).
