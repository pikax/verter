# Roles, communication and state

## Program orchestrator — thin scheduler and landing coordinator

Owns the DAG, sequencing, dependencies, capacity and landing order. Spawns and steers block
orchestrators and the landing agent — nothing else — and is the single landing authority.

Never implements, runs a review-fix cycle, manages implementers or reviewers, or ingests raw worker
logs, traces or review reports — it reads compact receipts only. Reacts to milestone, blocker,
decision and completion events rather than polling. Keeps only unblocked trains active; checkpoints
or shuts down finished teammates.

## Block orchestrator — owns durable local context

Owns the charter, scope boundary, relevant architecture, dependencies, settled decisions, acceptance
criteria, and decomposition into reviewable slices.

Spawns one manager and resumes it while it remains effective, priming it with the relevant
architecture and current slice — not the whole doctrine. Validates scope and completion without
duplicating code review. Sends compact events upward. Never implements.

**Its role ends at reporting the block ready and verified** — the candidate identity, the evidence,
and the manager's drafted squash message. That report is an input to landing confirmation, never a
substitute for it: the program orchestrator, or the landing agent acting for it, confirms
independently.

**A block never authorises itself.** A block branch carries no authority-registry delta — the registry
is authored trunk-side by its owner and inherited byte-for-byte through rebase, so a branch-local
registry edit is dropped rather than repaired. The ledger's `base_sha` and everything under
`repository.*` are orchestrator-owned in the same way.

## Manager — owns delivery

Dispatches implementation, selects risk-appropriate review lanes, adjudicates and deduplicates
findings, verifies a potentially blocking finding before it interrupts an implementer, dispatches the
smallest sufficient fix, and manages targeted re-review and acceptance. Drafts the squash message —
subject and body — at verification, and sends it upward with the acceptance evidence.

Stops when acceptance is met and no confirmed in-scope blocker remains. Escalates architecture, scope
or non-convergence rather than buying more rounds. Never writes production code.

## Workers

Implementers write code and its tests. Reviewers are independent and read-only. External tool
sessions (Codex, Grok) are manager-controlled workers, not teammates. Claude workers run in-process
or in the supplied CLI pool (runtime modes, `README.md`); the role contract is identical in both.

**One writer per candidate worktree at a time.** Parallel implementers only for naturally independent
slices with disjoint ownership, isolated worktrees and stable boundaries.

## Communication contract

**Program → block:** objective; in-scope and out-of-scope boundaries; acceptance criteria; relevant
architecture paths; dependencies; available capacity; any binding user instruction.

**Block → program:**

    STATUS: running | blocked | ready | complete
    RESULT: <one sentence>
    CANDIDATE_SHA: <the sha the evidence binds to, or none>
    EVIDENCE: <paths or commands>
    SQUASH_MESSAGE: <subject and body — required on ready>
    DECISION_NEEDED: <only if blocked>
    NEXT: <next material action>

**Manager → block:** outcome; changed scope if any; acceptance evidence; the drafted squash message;
confirmed unresolved risks; required decision if any.

**An idle notification is not completion.** A role sends its result before going idle.

Do not repeatedly paste SHAs, digests, diffs or reports between layers. Raw logs and review traces
stay out of parent contexts and are referenced by path.

## Block state — one compact record, replaced in place

Not an append-only ledger. One file per active block, containing only:

    OBJECTIVE:      what this block delivers
    IN SCOPE:       what it may change
    OUT OF SCOPE:   what it may not
    ARCHITECTURE:   the invariants and paths that bind it
    ACCEPTANCE:     the criteria, each with how it will be evidenced
    SLICE:          the current review slice
    CANDIDATE_SHA:  the frozen candidate under review, or empty
    DECIDED:        settled decisions, with what settled them
    DONE:           completed work
    OPEN:           confirmed findings still open
    BLOCKED ON:     required decisions, or empty
    EVIDENCE:       latest validation, by path
    NEXT:           next material action

Agents reread it after a resume or context replacement. If an agent repeats mistakes, contradicts
settled decisions, forgets scope or degrades, checkpoint and replace it — reuse is the default, not a
law.

## Large blocks

Large cohesive blocks are legitimate. Do not force decomposition on line, commit or file count alone;
treat size as a signal. Decompose for independent acceptance boundaries, owners, risks or review
surfaces.

Keep one persistent block orchestrator. Give each slice explicit scope and acceptance evidence, and
review it while its context is small — the acceptance phase in `review.md` is what covers cross-slice
behaviour, so do not rerun a cumulative review after every slice. Targeted tests per slice; the full
gate only on the final landing candidate. Do not invent interfaces or scaffolding to make artificial
slices. Sequential slices when ownership overlaps; parallel only for genuinely independent work.

A block that does not converge after targeted fix cycles is resliced or escalated, not reviewed
harder.

## Machine resources

Read-only review work may run in parallel. Write work runs only in isolated ownership.

**Heavy Cargo work — a full gate, a build, a mutation run — goes through the machine semaphore:**

    rust-lock.sh <name> -- <command>

Host-provided, on `PATH`; verify `command -v rust-lock.sh` before the first dispatch that needs it.
It bounds concurrent builds host-wide — not memory — and passes through re-entrantly, so a nested call
cannot deadlock on a slot its own tree holds. A "wait until no cargo is running, then start" check is
not mutual exclusion — every waiter sees idle at once and they all start together.

A gate carries its own memory ceiling and aborts its child tree on breach; a bare `cargo nextest`
under the semaphore carries none, so shed that unprotected workload first. Steer on swap used, not
free RAM — macOS keeps free pages near zero by design, and `vm_stat` reports its own page size, which
is not 4096 on Apple Silicon. Take two `loadavg` samples before calling a trend.

Cargo waiting on a target lock is not progress; do not read it as a stalled agent. Run the full gate
only on the final landing candidate; workers run targeted affected checks.
