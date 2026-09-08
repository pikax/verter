# Closure evidence and routine CI

Status: adopted by explicit maintainer instruction on 2026-09-08.

The maintainer requested disabling routine confirmation/replay and brittle
transcript-count bookkeeping after a fully passing targeted run was rejected
because it contained more tests than an older transcript.

Routine merge validation retains focused negative tests of important guards,
record/schema consistency, coverage/selection checks, and the normal Rust and
JavaScript test lanes. It does not replay historical proof commands or require
today's passing, skipped, or fixture totals to equal historical counts. Adding
or removing tests therefore does not require rewriting transcripts or pins.
Selection must still resolve and actual executed work must pass; empty or
incomplete runs are not success.

Full repository mutation replay is available through the manually dispatched
**Closure Control Replay** workflow. It is a diagnostic, outside `CI Required`.
Its clean run must execute nonempty work successfully, the mutation must apply,
and the mutated run must produce the expected refusal. Successful sibling tests
and unrelated inventory counts do not determine that refusal. Source mutations
are restored even when execution fails.

Closure transcripts remain historical evidence, checked for internal consistency.
The generated view states that distinction. No current execution or confirmation
is inferred from an old transcript, and no historical counts are rewritten merely
to make routine CI pass. This policy supersedes contrary automatic-replay and
live-count-matching requirements in the closure instrumentation documentation.

The change belongs to the lowest affected branch of the current stack, PR #472;
PR #507 and PR #508 inherit it. It does not change their semantic implementation
scope or the future flow nodes.
