# AMD-017 — the tsgo request deadline covers queue admission, and expiry before service does not tear the engine down

**Status:** RATIFIED, with the two corrections the ratifying seat named — both applied in this
revision. Verdict receipt (lane `architecture-amd-017`, RESULT PASS, 2 x P2 corrections, validated by
`scripts/orchestration/check-results.mjs` as structurally sound):
`~/.claude/briefs/rev11/verify/results/RULING/e90514830e2154f23f5f5c5a0f8473ee494620fc/architecture-amd-017.out`

**Scope sentence.** This amendment binds one public contract change on `block/deterministic-tests`:
`verter_tsgo_api::actor::RequestOptions::deadline` is redefined from a serve-phase response bound into
an absolute request-lifetime bound captured at `ClientHandle::request` entry that also covers queue
reservation and enqueue, and engine teardown on expiry is narrowed from unconditional to only once the
actor has begun serving the request. **It amends no charter, accepts no block, changes no block status,
adds or retires no DAG or ledger row, and authorises no further change to this deadline state machine,
nor any actor, queue or restart redesign.**

## 0. Content anchors, and how to falsify them

This record is anchored to CONTENT, not to a commit sha: a sha citation is invalidated by this
record's own commit and again by any landing squash, whereas the bytes described here are the thing
the argument is about.

| what | path | blob |
|---|---|---|
| contract BEFORE | `crates/verter_tsgo_api/src/actor/mod.rs` at merge-base `abebdec33` | `36068a3a2e55d358e2061b2fc53ad3ca81e7ac7f` |
| contract AFTER | `crates/verter_tsgo_api/src/actor/mod.rs` on the branch | `196ffb6e1d6f9b943fa722e026ab910b921c7b9a` |
| client doc BEFORE | `crates/verter_tsgo_api/src/client.rs` at merge-base `abebdec33` | `41ce234c181845d1f88a205e173e32779716cd36` |
| client doc AFTER | `crates/verter_tsgo_api/src/client.rs` on the branch | `35221d5b62ea05bb9610155ab349809e14c312d6` |

**Falsification test.** Run `git hash-object` on each path in the tree you are reading. If any result
differs from its AFTER blob, this record describes different content and is STALE — re-derive before
relying on any clause. Line numbers are avoided below; they drift for free.

## 1. Why this amendment exists

The branch's ratified scope is five permission-shaped bullets — deterministic test infrastructure;
exact timer tests; direct lost-wake and lifecycle corrections discovered while removing timing
assumptions; production/test topology alignment needed to test those mechanisms; removal of newly
added or directly touched polling and elapsed-time correctness checks — plus four required corrections
from the timing-architecture ruling, none of which concern deadlines.

**No ratified row states a required outcome for this change.** The nearest bullet, "direct lost-wake
and lifecycle corrections", licenses *touching* the request lifecycle but says nothing about what the
deadline must mean afterwards, so it cannot be the criterion this change is judged against. An
authorisation to do work is not an acceptance criterion that covers it, and mapping this change to
that bullet would convert a discoverable hole into an invisible one.

The change is not internal: `RequestOptions` is a public struct with a public field, a documented
contract, and an observable failure behaviour that changed.

## 2. The comparison, recorded rather than asserted

Both quotations are the rustdoc on `RequestOptions::deadline`, read from the two blobs in §0.

**BEFORE:**

> An optional hard deadline for the engine's response, measured from when the actor starts serving
> the request. On expiry the request fails with `TsgoApiError::Timeout` and the ENGINE IS TORN DOWN
> (the single-flight wire cannot recover a wedged request; the transport is terminated with a
> process-tree kill), so a hung engine can never block a caller forever. `None` keeps the legacy
> unbounded wait (interactive lanes).

**AFTER:**

> An optional hard deadline covering queue reservation, enqueue, provider execution, and the reply,
> measured from `ClientHandle::request` entry. On expiry the request fails with
> `TsgoApiError::Timeout`. The teardown boundary is the moment the actor begins serving the request:
> expiry during queue reservation, or while enqueued but before the actor begins serving, never
> starts a fresh timeout and does NOT tear the engine down. Once the actor begins the send/serve
> phase, expiry TERMINATES THE ENGINE (the transport is torn down with a process-tree kill) — the
> write may be partial or complete, and the single-flight wire's state is no longer safely
> recoverable. `None` keeps the unbounded wait (interactive lanes).

| clause | BEFORE | AFTER | changed |
|---|---|---|---|
| what the bound covers | the engine's response only | queue reservation + enqueue + provider execution + reply | **YES** |
| when the clock starts | when the actor starts serving | at `ClientHandle::request` entry | **YES** |
| clock kind | a duration applied at serve time | one absolute instant, reused by every arm | **YES** |
| error on expiry | `TsgoApiError::Timeout` | `TsgoApiError::Timeout` | no |
| engine teardown | unconditional | only once the actor has begun serving | **YES** |
| timeout restart after admission | not applicable | explicitly none — the same instant continues | **YES (new guarantee)** |
| `None` | unbounded wait | unbounded wait | no (wording only) |

Four arms carry the AFTER column:

1. **Reservation.** Queue reservation races cancellation and the absolute instant. Expiry returns
   `Timeout`, never sends the request, does not tear down.
2. **Post-send reply wait.** Awaiting the actor's reply races the *same* instant, so time spent queued
   after send is covered rather than restarting.
3. **Pre-service check.** Immediately before serving a dequeued request, an already-expired deadline
   replies `Timeout` and continues the actor loop. No teardown: service had not begun.
4. **Serve.** `serve_one` wraps `serve_frames` — the initial `send_frame` AND the response reads — in
   one `timeout_at`. Expiry there terminates the transport and ends the actor.

## 3. Why the change is right, stated so a reviewer can disagree

Under BEFORE a deadline could not bound time spent waiting for a queue slot: the clock did not start
until the actor began serving, so a caller passing a deadline had no bound on total request latency,
only on the serve phase. Under AFTER the field means what its name says.

**The teardown narrowing removes a liveness action, so it is the half that deserves scrutiny.** BEFORE
justified unconditional teardown by "the single-flight wire cannot recover a wedged request" — a
rationale specific to a request that reached the actor. A request that expired while queued never
touched the transport, so tearing the engine down would destroy a healthy engine and every other
in-flight request on it in response to a caller-side timeout. Two further points settle it:

- **No previously reachable guarantee is lost.** Under BEFORE the deadline did not run at all until
  the actor began serving, so queue-phase expiry could not have torn the engine down: there was
  nothing there to narrow.
- **An expired envelope leaves no wire state.** It may briefly occupy a bounded queue slot, but the
  actor drops it before `serve_frames`. If it sat behind an earlier wedged request, that earlier
  request is the wedge; the expired envelope did not create it.

**Where the boundary genuinely sits, and why the obvious wording is wrong.** It is tempting to say
teardown happens "only if the request had already been written to the wire". The code cannot draw that
distinction: `serve_one`'s single `timeout_at` spans the write, and the transport's write is
cancellation-unsafe `write_all` + `flush`, so expiry may land before the write starts, mid-write, or
after it completes, and the actor terminates in all three. The honest boundary is therefore
*before service begins* versus *after service begins*, not *before the wire* versus *after the wire*.

**Recorded because it nearly propagated:** the first draft of this amendment stated the
already-written-to-the-wire boundary, because that is what the field's own rustdoc said. **An
amendment that merely quotes the existing contract can ratify a false statement** — a document whose
whole job is to be authoritative can launder a doc's error into a ruling. That is the reason a
ratification reads the CODE and not only the contract it is asked to bless, and it is why the
production rustdoc was corrected in the same change rather than the amendment being written to match
it.

**Residual risk, named rather than dismissed.** The single production consumer configuring this
deadline is `verter_tsc`'s API-check path, which submits sequential calls and sets a deadline on every
request. No caller documents a dependence on expiry producing a fresh engine. `ClientHandle::restart()`
is NOT available as in-flight recovery: it queues a control message the actor reads only between
requests, so it cannot preempt a wedged `serve_one`. That limitation is real and predates this change;
it is stated here rather than papered over with a `restart()` citation. A consumer outside this
workspace is not a consideration — the crate is not published.

## 4. What this amendment does NOT do

- It licenses no further change to `RequestOptions`, the actor, the queue, or `restart()`.
- It alters no block's scope and none of the four required timing-architecture corrections.
- It creates no external-liveness exception: the deadline is an outer real watchdog over an external
  process, of the kind the timing taxonomy already requires, not a substitute for a receipt.
- It writes no ledger or registry row; that row is the program orchestrator's to write.

## 5. Acceptance

The coverage mapping for `block/deterministic-tests` records this change as COVERED by AMD-017, citing
this document by path plus the AFTER blobs in §0.
