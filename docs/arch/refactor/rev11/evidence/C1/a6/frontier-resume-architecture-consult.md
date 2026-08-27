# C1 frontier-prefix resume architecture consult

## Prompt metadata

- Task: `/root/c1_recovery_implementer/c1_frontier_resume_architect`
- Sender: `/root/c1_recovery_implementer`
- Date: `2026-08-26` (`Europe/Lisbon`)
- Candidate inspected: `0c22953821f57eedd32b812b1478a449a976f964`
- Mode: fresh read-only architecture consult; no production mutation or heavy suite
- Independent report preservation: `wall-diagnostic.md` remained byte-exact at SHA-256 `63c632006b5f5df404876389f48c7b1e7858919388f736f52c3fa149ab44ebb9`

Question:

> Does resuming the existing `PriorityFrontierState`'s pre-block completed prefix across request-local `NeedInputs` waves, owned and basis-cleared by the existing retained `ResolveFrame`, remain within ratified C1 request-local allocation/copy optimization, or does it introduce a new state-machine/semantic/cache contract? Require exact preservation of candidate ordering, full same-basis `LoadSet` union for each wave, speculative siblings, output discard/publication, consumed-witness order, terminal precedence, basis mismatch fail-closed behavior, frame/two-request isolation, and no cross-request retention. If `FIX_PATH`, specify the minimal permitted private shape and mutation/acceptance controls; if `NO_PATH`, name the exact violated invariant/authority.

## Verdict

Resuming a completed priority-frontier prefix across `NeedInputs` waves is not authorized as a C1-local allocation/copy optimization, even if:

- retention is confined to one `ResolveFrame`;
- state is cleared on basis change;
- only candidates completed before the first block are retained;
- every candidate at and after the first block is reevaluated;
- no speculative post-block hit, miss, terminal, or blocked state is retained.

It creates a request-local semantic continuation protocol across distinct immutable observation snapshots.

## Decisive invariants

1. The normative input-loading algorithm requires each retry to “run the whole operation against snapshot `S`” and, after extending the snapshot, “restart from step 1” (`contracts/input-loading.md`, §4 steps 1 and 10). Starting at the previous first-block candidate is not a whole-operation restart.

2. `AttemptOutput` is explicitly one fresh accumulator per attempt. A `NeedInputs` or `Terminal` result discards that attempt's accumulator entirely; only `Complete` transfers output to the driver (`attempt_output.rs` module contract and `AttemptOutput::new`).

3. Priority-frontier rules 3 and 9 are exact:

   - on the first block, retain only its `LoadSet`;
   - every `NeedInputs` or `Terminal` path discards all branch/frontier output.

   A completed pre-block prefix necessarily contains ordered consumed-observation output. Keeping it for a later wave retains more than the `LoadSet` and later republishes output originating in a discarded attempt.

4. `PriorityFrontierState` being described as “resumable” does not authorize this lifetime extension. Its documented purpose is suspending a frontier while evaluating a child frontier within one graph traversal and one attempt. It currently carries an attempt-local `AttemptOutput` and basis-local blocked set; it is not a cross-snapshot continuation token.

5. The prior memo ruling permits request/`ResolveFrame`-local pure derivation reuse. A completed candidate miss is not pure request geometry: it depends on observation values and carries semantic witness output.

## Why pre-block-only retention is still insufficient

Excluding every speculative post-block result is necessary to preserve:

- full same-basis `LoadSet` union;
- bounded speculative sibling evaluation;
- lower-priority-hit precedence;
- terminal-after-block precedence;
- reconsideration after loading.

It does not solve the attempt-boundary violation.

There are only two ways to skip the completed prefix:

- retain its `AttemptOutput` and merge it into a later `Complete`, violating the discard contract; or
- retain only a prefix cursor and reconstruct its witness later without reevaluating it, introducing a new observation-result replay/proof mechanism.

The second form still needs a ratified prefix identity, snapshot/fact revalidation rule, invalidation behavior, and publication contract. Same-basis equality and frame isolation do not supply those semantics.

## Current lawful boundary

C1 may still reuse request-local pure data while every attempt begins at candidate zero and constructs a fresh `AttemptOutput`, including:

- immutable candidate strings and candidate-list geometry;
- precomputed ancestor/recovery-scope spellings;
- existing `Arc` identities;
- request-local scratch-buffer capacity;
- basis-cleared pure string derivations.

Such reuse must continue to perform the current candidate observations in order and freshly record the exact consumed witness for the current attempt.

The measured request-local recovery-scope memo experiment stayed inside this boundary but was rejected rather than committed: the fresh diagnostic worsened retired instructions from `57,534,528,973` to `57,843,217,976` (`+0.537%`) and wall median from `99.90` to `100.14` ms. It therefore does not close A6.

## Authority required

If passing A6 requires skipping completed semantic candidates across waves, implementation must stop and obtain architecture authority covering:

- the snapshot-consistent input-loading restart rule;
- `AttemptOutput` ownership and discard/publication semantics;
- the F18 priority-frontier rules;
- current F24 witness/replay obligations;
- a typed continuation identity and current-snapshot revalidation contract.

This is not an A6 threshold or recalibration question, and tests alone cannot authorize the change.

RESULT NO_PATH
