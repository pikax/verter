# AMD-020 — a request cancelled mid-flight resolves `Cancelled`, not `Closed`

**Status:** RATIFIED, with the two corrections the ratifying seat named — both applied in this
revision. Verdict receipt (lane `architecture-amd-020`, RESULT PASS, 1 x P2 + 1 x P3, validated by
`scripts/orchestration/check-results.mjs` as structurally sound):
`~/.claude/briefs/rev11/verify/results/RULING/279531b4ad775c9c9442c236420e8356c919d807/architecture-amd-020.out`

**Scope sentence.** This amendment binds ONE observable value change on `block/deterministic-tests`:
when `verter_tsgo_api`'s actor finishes a request that was cancelled mid-flight, it now sends
`Err(TsgoApiError::Cancelled)` on the reply channel instead of dropping the channel, so
`ClientHandle::request` resolves `Cancelled` deterministically rather than `Closed`. **It binds
nothing else in the actor** — not the deadline state machine (AMD-017 governs that), not
`CancelToken`'s `Notify` rework, not `reserve_queue_slot`, not the `#[cfg(test)]` `AdmissionTrace`,
not the queue, and not `restart()`. It amends no charter, accepts no block, changes no block status,
and adds or retires no DAG or ledger row.

## 0. Content anchors, and how to falsify them

Anchored to CONTENT, not to a commit sha: a sha citation is invalidated by this record's own commit
and again by any landing squash.

| what | path | blob |
|---|---|---|
| BEFORE | `crates/verter_tsgo_api/src/actor/mod.rs` at merge-base `9e7fa5c0c` | `36068a3a2e55d358e2061b2fc53ad3ca81e7ac7f` |
| AFTER | `crates/verter_tsgo_api/src/actor/mod.rs` on the branch | `196ffb6e1d6f9b943fa722e026ab910b921c7b9a` |

**Falsification test.** Run `git hash-object crates/verter_tsgo_api/src/actor/mod.rs`. If it does not
equal the AFTER blob, this record describes different content and is STALE — re-derive before relying
on any clause.

AMD-017 anchors the same two blobs. That is expected and is not a conflict: the two amendments bind
disjoint clauses of one file. AMD-017 governs the deadline contract and its §4 names the actor as
OUT of its licence, which is precisely why this change needs its own instrument rather than an
extension of that one.

## 1. Why this amendment exists

The branch's ratified scope is five permission-shaped bullets — introduced by "It is limited to:" —
plus four required corrections in the timing-architecture ruling. **No ratified row states a required
outcome for what a cancelled request resolves to.**

The nearest rows do not reach it:
- S3.2-c, "direct lost-wake and lifecycle corrections discovered while removing timing assumptions",
  grants permission to touch this area and states no outcome.
- **D6-OK2, "Atomic-state-plus-`Notify` cancellation", is an ENDORSEMENT of a mechanism, not a
  criterion.** It approves the shape the branch adopted; it does not say what a caller must observe.
  This change is a CONSEQUENCE of that endorsed mechanism, and a consequence is not a criterion.
- AMD-017 §4 states, verbatim: *"It licenses no further change to `RequestOptions`, the actor, the
  queue, or `restart()`."* It therefore excludes this change by name.

The change is observable to callers and lands on a public error enum, so it is ratified rather than
absorbed.

## 2. The comparison, recorded rather than asserted

`Actor::serve_one` matches on the outcome of `serve_frames`, whose `Ok(None)` arm means "the request
resolved without a reply because it was cancelled mid-flight".

**BEFORE** (merge-base blob):

```rust
Ok(None) => Ok(()),
```

The reply `oneshot::Sender` is dropped. `ClientHandle::request` awaits `reply_rx` and maps a receive
error to `TsgoApiError::Closed`, so the caller observes **`Closed`** — unless its own cancellation
detection happens to win the race first, in which case it observes `Cancelled`. Which one a caller
saw depended on scheduling.

**AFTER** (branch blob):

```rust
Ok(None) => {
    let _ = reply.send(Err(TsgoApiError::Cancelled));
    Ok(())
}
```

| clause | BEFORE | AFTER | changed |
|---|---|---|---|
| value observed on mid-flight cancellation | `Closed`, or `Cancelled` if the caller's own detection won the race | `Cancelled` | **YES** |
| determinism of that value | scheduling-dependent | scheduling-independent | **YES** |
| the actor's own control flow | `Ok(())`, actor continues | `Ok(())`, actor continues | no |
| the error enum's variants | unchanged | unchanged | no |
| the wire / transport | untouched | untouched | no |

## 3. Why the change is right, stated so a reviewer can disagree

`Closed` means the actor is gone. It was never true here: the actor is alive and continues its loop
on the very next line. A caller distinguishing "my request was cancelled" from "the engine died"
could not do so reliably, because the answer depended on which of two futures the scheduler polled
first. Dropping the channel communicated a fact that was false and did so nondeterministically.

**Why the two signals must agree.** The client-side cancellation detection is NOT removed by this
branch — its 2 ms timer poll is replaced by an event-driven `Notify`, and `ClientHandle::request`
still selects on it. That is exactly why the actor's arm matters: the caller's `select!` is `biased`,
so which of the two arms resolves first decides what it observes. If the reply channel is dropped
while the cancellation arm is also live, the outcome is scheduling-dependent — `Cancelled` when the
cancellation arm wins, `Closed` when the dropped channel does. **Both arms must agree, so that a
biased, scheduling-dependent selection cannot manufacture `Closed` from a live actor.** Sending
`Cancelled` explicitly makes the two arms report the same fact.

**Residual risk, named rather than dismissed.** A caller matching on `Closed` to detect cancellation
would now miss it. No such caller exists: `verter_tsgo_api` is `publish = false`, and in-repo callers
match `Cancelled` for cancellation and treat `Closed` as engine death, which is the meaning this
change restores. Six assertions in the actor suite expect `Cancelled`, but stated precisely: only TWO
— the raw response-frame and raw error-frame reply tests — directly pin this explicit `Ok(None)`
reply value. The other four exercise adjacent cancellation entrances or the client-side notification
path and would still pass if this arm regressed to dropping the channel, so they are not coverage of
this change.

**What would argue against it:** if some caller depended on cancellation being indistinguishable from
closure — for instance a retry path that reconnects on `Closed` and would now stop reconnecting.
No such path exists in-repo. That is the claim to falsify if this is rejected.

## 4. What this amendment does NOT do

- It licenses no other change in the actor, the queue, `CancelToken`, `RequestOptions`, or `restart()`.
- It adds and retires no error variant; `Cancelled` and `Closed` both already existed.
- It alters no block's scope and none of the four required timing-architecture corrections.
- It writes no ledger or registry row.

## 5. Acceptance

On ratification, the coverage mapping for `block/deterministic-tests` records this value change as
COVERED by AMD-020, citing this document by path plus the AFTER blob in §0. The remainder of the
group it sits in stays SCOPE-ONLY and is reported as such.
