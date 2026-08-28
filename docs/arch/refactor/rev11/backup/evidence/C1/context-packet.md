# C1 — context packet

Written **before** the dispatch it governs, while the block is still `LOCKED`. That ordering is
the whole substance of this document: a packet authored after its work would be a reconstruction,
and this block has the opportunity to author a real one rather than inherit an exemption.

Charter `docs/arch/refactor/rev11/charters/C1.md`, **RATIFIED** — the single controlling sentence
incorporating `ARCH-ADDENDUM-C1-THREE-GAPS.md` was confirmed at branch tip
`8799580e16165897ce1dac3f1cc16f8bd42431a4` (lane `architecture-c1-charter-confirm`, `PASS`,
findings none). Class **foundational**, so all three review mandates are required on one candidate
sha; `NOT_REQUIRED` is permitted only for `architecture_review` on a `subsystem`-class block and is
therefore unavailable here.

Branch `block/module-resolver-core`, based on trunk `f593b24c8a2a53b5496d85ee4de1bab0dafe61d1`.

## What this packet covers, and what it does not

**Covers** the remaining C1 dispatch: **Stage 2, the atomic module-resolver cutover**, executed
against the ratified execution plan, plus its review mandates and acceptance.

**Does not cover** the 116 commits already on this branch. Stage 1 — the algorithm port — is
complete and was dispatched before this packet existed; nothing here is claimed to have governed
it. Stating that boundary is the point, exactly as it was for the sibling CSS block: a packet that
silently claimed prior work would be the backdated artifact the record exists to prevent.

**Explicitly does not authorise the seven `wip(core)` commits.** They are unapproved scratch, not
dispatch authority. Stage 2 dispatches against the plan, never against those commits; the plan
records them as superseded rather than as work to unwind. They are additive — new semantic-owned
modules, no deletions, no repointing, no Cargo or guard changes — so setting them aside costs
nothing.

## Authority, and one thing this branch may never do again

Authorization is authored **trunk-side by the registry/ledger owner**, after ratification, from
current facts. This branch carries **zero** `authority-registry.toml` delta and may only inherit
the trunk-owned registry byte-for-byte through rebase. Verified: the registry blob is identical on
both sides (`04da165b78d4905b6256952a72665d055b2fd383`).

The reason is recorded because it was expensive to learn. This branch previously authorised itself,
and that self-authorisation failed three ways at once: unsupported TOML escapes made the **entire
authority layer inert** on the branch — no digest check, no ratification check, in the one file
whose purpose is preventing unauthorised dispatch; the pinned charter digest did not match the
charter's bytes; and the authorization asserted every predecessor `ACCEPTED` when `CM1` had since
returned to `REVIEW`. That last is the general failure, not a C1 quirk: **an authority document
that asserts a mutable fact is a snapshot pretending to be a rule.** A block never authorises
itself, and a predicate about another block's status does not belong in a document that outlives
the moment it was true.

## Stage 2 — what is dispatched, and its preconditions

The execution plan is `docs/arch/refactor/rev11/evidence/C1/stage2-execution-plan.md`, promoted
from item 5 of `sequencing.md`. It binds an exact rebased baseline by literal sha,
the caller and deletion inventory across nine crates, the Cargo-edge reversal together with the
`verter_identity` guard transition in one commit, and eight numbered atomic abort conditions with a
return-to-known-good procedure.

**The plan is `PROPOSED`, not authority.** Stage 2 does not dispatch until it is ratified and a
trunk-side registry row exists. Neither is this block's to author.

The cutover is **genuinely irreversible** by its own sequencing record — delete the old resolver,
repoint ~14+ call sites across six-plus crates, reverse the Cargo edge, flip the guard cluster.
That is why it gets a ratified plan rather than a decision taken inline at the tail of a porting
round, and why the plan's abort conditions are part of the authority rather than commentary on it.

## Gating

C1 remains gated on **CM1** throughout. Nothing here lands until CM1 does, so there is no schedule
pressure that would justify dispatching Stage 2 against an unratified plan. Its own successors —
`D1` most immediately — wait on C1, which is a reason to get the cutover right, not a reason to
start it early.

## Evidence state at dispatch

No `results/C1/` directory exists and there is no sha-bound review evidence for C1 of any kind.
Every acceptance claim is built fresh and bound to a sha through `check-results.mjs`. The ~117
commit subjects carry program vocabulary and seven use a non-approved `wip` type; both are resolved
by the squash to one compliant landing commit, and the **bodies** are audited too, not only the
subjects. Any vocabulary scan is scoped to this block's own commits and files — an unscoped grep
after a rebase spans all of trunk and reports false hits.
