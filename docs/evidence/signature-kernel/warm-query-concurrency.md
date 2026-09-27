# Concurrent queries on one host — evidence, fixes and proposals

What the signature-kernel benchmark's scalability sections and the
candidate-only contention and cancellation probes found about concurrent
work on one `VerterHost`, what was fixed on this branch, and what is left,
with a proposed design for each remaining point. Numbers are machine-bound
evidence for a decision, not a gate: every gate stays as recorded in
[`performance-gates.md`](performance-gates.md).

## Method

- **Probes.** `signature_kernel_bench` (sections `concurrent_queries`,
  `scheduler_scaling`, `full_check`), `signature_kernel_contention_probe`
  (A0 public audited query, A1 dispatch steps inside one request, A2 memo
  read alone, A3 request setup and teardown alone; A0 and A2 also on one
  shared witness) and `signature_kernel_cancel_probe` (stop phases).
- **Profiles.** samply 0.13 on Linux (WSL2, 32 logical CPUs,
  `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only`), recording one point at
  a time (`--only A3:disjoint --callers 8`), the full-check point at eight
  callers by its caller threads, and the cancellation probe restricted to
  the post-poll windows the probe logged.
- **Timings.** Windows 11, 32 logical CPUs, release build, three
  interleaved before/after invocations on an otherwise idle machine; each
  cell is the median over invocations of each invocation's median, with
  the spread between invocations. "Before" is the tree before the fixes
  below (the contention probe's own commit), "after" the head after them.

## Hot points found and fixed

1. **The audit records store** (`component_meta_audit/audit_records_store.rs`).
   A3 at eight callers spent 82.5% of its time in
   `ActiveRegistration::finalize`, mostly `memmove` and futex waits.
   Every audited request — flow-return requests too, with audit capture
   off, because the consumer filter, not `audit_enabled`, decides whether
   a record is kept — inserts its record into one host-wide mutex store
   of 256 entries, and once full each insert evicted the oldest with
   `IndexMap::shift_remove_index(0)`, moving every retained entry under
   the lock. The store now orders records by an insertion sequence in an
   ordered index beside a hash map, boxes records before the lock and
   drops leaving records after it.
2. **The warm memo read's second lock turn**
   (`semantic_query_memo/mod.rs`). A2 at eight callers spent 41.5% in
   `entries_lock_diagnosed`: a warm hit took the memo's single `entries`
   mutex twice, the second time to move the hit to the back of its slot's
   LRU order even when it already was the back. A hit on the snapshot's
   freshest candidate skips that turn; a test holds one acquisition per
   warm query at one and at four concurrent callers.
3. **The retention ledger's trim** (`bounded_query_retention.rs`). The
   full-check point at eight callers spent 6.6% in the memo's family
   admission, nearly all of it `memmove` inside
   `GlobalRetentionBudget::trim_locked`: past the memo's 4096-family cap
   every admission removed its one victim with a `retain` over the whole
   ledger while the `entries` mutex was held. Victims that are the
   ledger's oldest records now leave by a front drain that touches only
   them; a test holds that the removal touches only the victim.
4. **The flow-return schedule's discovery walk after a trip**
   (`project_semantic_dispatch/flow_return_schedule.rs`). After the first
   poll observed a cancellation, a fifth of the remaining time was the
   callee schedule discovering every remaining sibling chain, because
   discovery charges no work and the trip was read only at a component
   close. The walk now reads the trip and the cancellation before each
   callee it discovers.

Every answer is unchanged: the fixes change which locks are taken and how
records leave containers, never what is computed or kept. The contention
probe asserts that every witness answers the same when read by 1/2/4/8/16
concurrent callers as when read by one.

## Before and after

`concurrent_queries` (one warm host, 4 host workers):

| Callers | q/s before | q/s after | query p95 before | query p95 after |
|---|---|---|---|---|
| 1 | 104.8k (20%) | 156.3k (3%) | 13.1 µs | 8.3 µs |
| 2 | 154.8k (6%) | 286.7k (1%) | 15.7 µs | 9.1 µs |
| 4 | 109.6k (75%) | 531.2k (1%) | 90.9 µs | 9.8 µs |
| 8 | 84.4k (43%) | 567.0k (3%) | 242.9 µs | 23.8 µs |

Contention probe, thousands of operations per second, disjoint keys unless
noted:

| Variant | 1 caller | 2 | 4 | 8 | 16 |
|---|---|---|---|---|---|
| A0 before → after | 108 → 163 | 163 → 291 | 119 → 535 | 117 → 563 | 117 → 449 |
| A0 same key | 91 → 128 | 146 → 233 | 126 → 421 | 122 → 410 | 121 → 450 |
| A1 | 216 → 221 | 398 → 417 | 724 → 774 | 766 → 736 | 452 → 818 |
| A2 | 489 → 536 | 853 → 930 | 1366 → 1598 | 771 → 1294 | 354 → 653 |
| A2 same key | 318 → 331 | 518 → 536 | 913 → 915 | 706 → 683 | 482 → 594 |
| A3 | 214 → 668 | 180 → 1014 | 125 → 1776 | 134 → 1651 | 139 → 704 |

The same-key rows scale with the disjoint ones: nothing collapses to
single flight on one warm key.

`scheduler_scaling` (one caller, 576 cold roots) and `full_check` (96
files, one caller per worker):

| Workers | B wall before | B wall after | C files/s before | C files/s after | C CPU utilisation before → after |
|---|---|---|---|---|---|
| 1 | 822 ms | 707 ms | 119.9 | 137.6 | 1.00 → 0.99 |
| 2 | 839 ms | 722 ms | 215.3 | 254.0 | 0.88 → 0.92 |
| 4 | 823 ms | 721 ms | 321.6 | 395.6 | 0.72 → 0.76 |
| 8 | 814 ms | 721 ms | 362.2 | 421.2 | 0.58 → 0.62 |

B is flat in the worker count before and after (CPU time equals wall
time: one caller's semantic work runs on the caller's thread); the ledger
trim made each cold publish cheaper, about 13%. C still flattens past four
workers with falling utilisation: see the memo's `entries` mutex below.

Cancellation (ms, p50 / p95 / p99, three invocations pooled):

| Point | stop before | stop after | poll delay after | unwind after |
|---|---|---|---|---|
| 10% | 1.27 / 1.68 / 1.91 | 0.76 / 1.33 / 2.42 | 0.03 / 0.18 / 0.23 | 0.72 / 1.31 / 2.39 |
| 30% | 3.15 / 4.76 / 5.63 | 2.55 / 5.27 / 5.77 | 0.03 / 0.14 / 0.29 | 2.49 / 5.21 / 5.72 |
| 50% | 5.30 / 8.15 / 8.83 | 4.50 / 5.51 / 7.16 | 0.03 / 0.14 / 1.09 | 4.42 / 5.31 / 7.14 |
| 70% | 6.84 / 10.33 / 11.04 | 6.50 / 9.33 / 9.66 | 0.04 / 2.41 / 2.89 | 6.39 / 9.22 / 9.61 |

The poll delay is tens of microseconds at the median; the stop is the
unwind, and it grows with how far the request had progressed.

## Proposals (not implemented)

### Shard the semantic memo's `entries` map

Every memo read, publish, invalidation and retention step takes one
host-wide `parking_lot::Mutex<FxHashMap<FamilyKey, FamilySlots>>` (79
acquisition sites). After the fixes a warm read takes it once, but every
cold publish takes it, holds it for the family admission and the
eviction plan, and the full check's remaining serialisation is there
(`record_family_admission_locked`, `plan_family_slot_eviction` and lock
waits under `warm_publish_one`).

Design: a fixed number of shards (a power of two, e.g. 64), each its own
mutex over its families, selected by the `FamilyKey`'s existing hash.
Single-family operations lock one shard. Multi-family operations — the
batched SCC member publish, invalidation sweeps, `clear`, the reverse
canonical index — lock the shards they touch in ascending index order
(never two orders), so the fenced publication stays atomic. The retention
ledger stays one global FIFO (its own small mutex, already separate), so
eviction order is unchanged; victims are removed shard by shard after the
ledger step. Resident structure: none new — the same map split, owned by
the store, released with it. Size: large (every acquisition site and the
multi-family paths), so a separate task.

### Keep the audit registry off the request path when nothing samples it

`HostAuditRuntime::active_requests` is an `RwLock<FxHashMap>` written twice
per request (register and finalize) to feed the peak-RSS sampler, which
runs only with `audit_timing_capture`. Proposal: register a request only
when the sampler exists (`audit_timing_capture`), keeping the public
`snapshot()` contract for the audit-enabled configurations that use it.
Separately, flow-return requests publish a record into the records store
even with `audit_enabled = false`, because the consumer filter decides it
(a documented contract,
`tests/cases/g_type/flow_return_audit_contract.rs`); gating publication on
`audit_enabled` is a behaviour change that needs that decision first.
Size: small, but a contract change.

### Read the store view without the manager's mutex on a warm hit

`StoreViewManager::base_view` takes a host-wide mutex on every request to
compare the live validation token (nine generations and a fold of the
workspace env hashes) with the cached view's. Proposal: publish the
current `(token, view)` pair through an `ArcSwap` and serve a warm hit by
a lock-free load and compare, falling back to the mutex only to build or
join a build. The live-token re-read that makes a warm hit current stays
the same comparison. Size: medium (the build/join protocol is unchanged).

### Bound the cancelled request's unwind

After the first poll the cancelled root still (a) finalises its build's
fact read set — 35% of the post-poll time, sorting every fact the build
read (`FactReadSet::finalise` → `canonicalise`) for a result that is then
discarded — and (b) tears down the dispatch transaction, dropping every
obligation record and completed member it accumulated. Both grow with
the work done before the cancellation. Proposal: (a) a cancelled cold
build skips read-set canonicalisation and hands the tracer back
unfinalised (the result can never be admitted, and every enclosing build
is cancelled with it); (b) the teardown moves the transaction's
accumulated state to a host-owned release worker instead of dropping it
on the caller's thread. Size: (a) medium, touching the fact-tracer
protocol; (b) small but adds a resident worker, which needs the WSP6
owner/lifetime review.

### One caller, many roots

`scheduler_scaling` shows no internal parallelism for one caller; the
design for a coarse-grained batch entry is
[`check-root-batch.md`](check-root-batch.md).
