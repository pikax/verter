# A6 — The Implementation Lock Record

Freezes one exact post-Gate-0 implementation baseline, the program's inputs, and its numeric
performance gates, before any foundational candidate exists — so that implementation cannot invent
its own contracts or negotiate its own pass criteria afterwards.

Identity-free by construction: this file records no candidate SHA and no review verdict, per the
convention the adoption landing established. The exact-candidate record lives in the ledger tree.

## The evidence

| artifact | what it freezes |
|---|---|
| [`A6/implementation-lock-record.md`](A6/implementation-lock-record.md) | the lock itself — twelve sections, from repository identity to the acceptance checklist |
| [`performance-gates.toml`](../../../../../performance-gates.toml) (repository root) | one required benchmark cell, fifteen metrics, **zero placeholders**, `status = "LOCKED"` |
| [`A6/baseline-measurement.md`](A6/baseline-measurement.md) | every number in the gate file and how it was derived |
| [`A6/baseline-counters.tsv`](A6/baseline-counters.tsv) | the counter dataset at this baseline, 44 data rows (45 lines including the header) |
| [`A6/counter-reproduction.md`](A6/counter-reproduction.md) | four independent observations per gated counter, and the two counters that moved and are therefore not gated |
| [`A6/command-proofs.md`](A6/command-proofs.md) | the non-vacuous command manifest with exit codes and executed counts |
| [`A6/stack-window-policy.toml`](A6/stack-window-policy.toml) | the locked bounded-stack policy |
| [`A6/AMD-001-deviation-memo.md`](A6/AMD-001-deviation-memo.md) | the `governance.md` §10 deviation memo for the amendment's four undelivered artifacts — **RULED and superseded**: the maintainer adopted the rescope, not the memo's `DEFER`; retained as the historical record |
| [`charters/B1.md`](../charters/B1.md) | the first unlocked charter, **bound** — source paths, owners, gates and reviewers filled in |
| [`A6/B1-context-packet.md`](A6/B1-context-packet.md) | that block's dispatch packet |
| `scripts/validate-performance-gates.mjs` + `.test.mjs` | the gate file's validator and its twenty discriminating negative controls |

## What the lock decides

### The baseline is source-identical to the tree the measurement was taken on

The gates are frozen against the post-instrumentation lineage. The instrumentation block captured its
dataset on its own accepted candidate; the two blocks after it changed no production source. That is
verified rather than assumed — the diff between the measured commit and the baseline, restricted to
`crates/`, `packages/`, `scripts/`, `.github/` and both lockfiles, is **empty**. Only the ledger and
files under `evidence/` differ.

So the retained counter dataset describes this exact source, and the gate file can cite it. The
timings were nevertheless re-measured, because the sampling policy this lock freezes (≥30 samples) is
stricter than the one the retained timings used (7). A baseline that fails its own sampling rule is
not a baseline.

### The gate file gates one cell, and says so

One required cell: a cold 41-file project batch — upsert, load, component metadata per component,
host-backed batch compile — with fifteen conjunctive metrics. Four timing and memory gates — an absolute and a
no-regression bound each on wall clock and peak RSS — measured on the disabled instrumentation arm,
eleven deterministic work counters measured on the enabled arm, three zero-work CSS assertions, and an exact output oracle on the component-meta digest.

It is **not** an exhaustive suite for every future block, and it does not pretend to be. The
direct-compiler, managed-compiler, CSS, provider and competitor benchmark families the verification
contract names are not locked here and no cell claims them. Adding a cell later is an *extension*
requiring a new lock digest and the same independent review class; it never licenses weakening one
that exists.

Two disciplines make the numbers trustworthy rather than convenient:

- **Relative bounds are derived from measured noise.** `max(3%, 2 × noise)` gives 3.000% for wall
  clock (measured noise 1.4757% across four 30-sample invocations) and 4.952% for peak RSS (2.4760%).
  The RSS bound is *not* rounded up to 5.0 — the rule is an upper bound.
- **Absolute limits are product budgets, not fits.** 100 ms for the batch follows from a 2.5 ms
  per-component cold budget so a 1,000-component project's initial batch stays inside 2.5 s. 256 MiB
  follows from a whole-project RSS ceiling. Neither is derived from the 70.5 ms and 74.9 MB actually
  measured, and the RSS absolute is recorded as a catastrophe stop whose tight fence is the relative
  gate.

### Only counters that actually held still are gated

Four independent observations of every row. Two columns moved: `scheduler.task_wait.amount`, whose
declared unit is nanoseconds — a duration in a counter's clothing — and
`session.read_set_signature_build.amount`, which drifted by 60 items (+0.045%) with its call count
identical. Neither is gated.

That second one is the load-bearing negative result: it is a *count*, not a clock, and it moved
anyway. "It is a counter" is not by itself a licence to gate on it.

Two sites that record zero are also deliberately **not** asserted zero — the compiled-output digest
and the FFI boundary sites record zero because this workload's lane does not reach them, which is a
known gap, not a requirement. Freezing a gap as a gate would make a later block's correct fix fail.
The CSS sites *are* asserted zero, because their absence is a property of the corpus rather than of an
unreached lane.

### "No placeholders" is machine-checked, not asserted

The template's own acceptance condition is that the locked file contain no `REQUIRED_*` value and pass
a validator. That validator did not exist; the Python original was never available, and the maintainer
ruled the validators are reimplemented in Node. So this landing adds
`scripts/validate-performance-gates.mjs` and its suite, wired into the same `test:scripts` runner as
the program-state validator.

The suite carries twenty negative controls, each one a single mutation of a complete locked shape.
Each mutation is asserted **present and unique in the source before it is applied**, so a control that
silently fails to apply cannot report a pass — the exact failure mode the repository's verification
rule names. A meta-check asserts every control also fails against a permissive stub.

### The stack policy is a policy, not a snapshot

`max_open_stack_layers = 2` (the minimum of the permitted range), `ATOMIC_REVIEW`, and a local branch
chain — because GitHub-native stacks, merge queues and dependent PRs are all unavailable when nothing
is pushed. Default operating depth is **1**: a window of 2 is a ceiling for the one mandated
private-checkpoint pair, not a target.

The artifact is deliberately a *policy* file rather than an instance of the snapshot template. No
window is open, and minting a one-layer snapshot would record a stack that does not exist.

### One charter is unlocked, and one deliberately is not

The neutral-contracts block is unlocked with a **bound** charter that fills in what the template left
open: current owners, the locked dependency-direction strategy, the ratified equality-pinned
exception with its target condition, the enumerated public and wire consumers, the required commands,
and the performance cell.

The CSS block is **not** unlocked. The template's required set is one block "and optionally" the CSS
block "when CSS work is selected". It is not selected: no CSS cell is locked, the baseline workload
records zero CSS work, and nothing before or within the unlocked block consumes the CSS inventory.
Unlocking it would put a block in flight whose evidence nothing reads.

### What the lock does not ratify, stated rather than hidden

Three unresolved items exceed the bar the template sets for its own unresolved-items section, which
requires every entry to be a private implementation choice that cannot change semantics or
compatibility. They are recorded as exceeding it:

- **The capability matrix is entirely unratified.** Every status cell is `VERIFY`; no maturity,
  default or compatibility promise is ratified here, because doing so needs product/conformance
  review and oracle evidence no block through this one produced. This is fail-closed — an unratified
  row is not approved for architecture claims — but it means the atomic flow-cutover block's
  obligation to preserve every Supported/Stable row is currently *vacuous*, and the matrix must be
  ratified before that block begins.
- **A hand-pinned provider protocol version may duplicate a compatibility domain owned elsewhere.**
  Recorded NOT PROVEN in either direction rather than resolved by assumption.
- **The registered amendment's four artifacts are not delivered — and, after a maintainer ruling,
  that is now correct rather than a gap.** `AMD-001` originally named this block as their deliverer:
  a Node stack-window validator, composite program-state cross-validation, that validator's CI
  wiring, and a discriminating checkpoint/acceptance transition test. None is delivered. The deferral
  was **not this block's to grant**, so it was written up as a `governance.md` §10 deviation memo
  recommending `DEFER`. The maintainer ruled **AMEND-AMD-001-TIMING** instead: §1 is amended in place
  so the four artifacts bind to whichever accepted candidate immediately precedes the first opened
  stack window, and unconditionally to the one before the private-checkpoint block begins — not to
  this block by name. So the amendment text and the delivery reality now agree; the memo is retained
  as the historical record of a recommendation the ruling superseded. §§2-4 stand unchanged: the
  amendment is named by identifier and bound by its **post-amendment** digest in the lock record and
  in the context packet's second addendum — the half of it a lock block can actually discharge — and
  its rule that the program-state validator's fail-closed refusal may be superseded but never deleted
  is honoured: that refusal is untouched here, which is what keeps the unmodelled path closed rather
  than open.

The acceptance checklist in the lock record is ticked accordingly — three rows partial, two pending.
A checklist ticked complete while those sit open would be exactly the failure the program's
verification rule exists to prevent.

### The ledger gains an integration-lineage field

The ledger's `[repository]` table records the *entry checkout*, while accepted blocks land on a
separate integration branch, and no field distinguishes the two. A resuming agent reading that table
alone would land onto the default branch and silently drop every accepted block. The lock records the
lineage explicitly and adds it to the ledger schema.

One consequence is recorded before the first landing rather than discovered at it: the lineage must
not be fast-forwarded into the default branch while the ledger-import commit is in its history,
because the transport copy's removal obligation includes git history.

## Ratified decisions carried into the lock

Five decisions the previous block raised for ratification are incorporated as accepted, each into the
section that owns it, and each is locatable rather than merely claimed:

| decision | where it lands |
|---|---|
| the instrumentation converge-then-delete disposition, with its counter owner, its watchdog owner and its hard backstop named | lock record §7, as a debt row whose ruling reference is ratified with the lock; the record also states that this block performs none of that migration |
| the two feature arms as locked per-block commands, CI job deferred post-program | lock record §2, with the deferral as U-10 |
| the semantic-kernel upward edge as an equality-pinned exception, with its removal gate and its target condition | lock record §6 |
| the unlanded local-branch population abandoned as a class — no branch deleted, no GitHub action | lock record §1; the ruling itself is registered as R-12 in [`maintainer-rulings.md`](maintainer-rulings.md) |
| the bounded-stack policy above | lock record §9 and [`A6/stack-window-policy.toml`](A6/stack-window-policy.toml) |

## Verification, including what failed

The canonical gate and the end-of-change checks were executed at this candidate; exit codes, executed
counts and raw-output digests are in [`A6/command-proofs.md`](A6/command-proofs.md) and
[`A6/command-proofs-native.md`](A6/command-proofs-native.md). The three instrumentation-arm commands
this lock makes mandatory for every later block were run here first, so the requirement ships with a
proof that it can be met.

**The canonical gate returns FAIL, and running it is how that was discovered.** Five reported
failures, three distinct tests:

- **`tracked_paths_no_machine_roots` — a genuine tracked-tree defect that pre-exists this baseline.**
  Two already-accepted blocks' context packets embed an absolute machine path. Both blocks skipped
  the canonical gate on the reasoning that they changed no production source; the guard scans tracked
  *bytes*, not production source, so that reasoning had a hole in it. Proven against the baseline
  commit with `git grep`, not inferred. This block's own packet was a third instance and is fixed —
  verified discriminating, since re-running the guard alone now reports two violations rather than
  three. The remaining two are not repairable here: their files' digests are recorded in the ledger
  as two accepted blocks' `context_packet_digest`, so editing them is an orchestrator action.
- **A real-tsserver respawn test and a trybuild smoke test** both failed under a load average of ~34
  and both **pass in isolation on an idle machine** (2/2 and 1/1, the latter at 260 s against a 360 s
  cap). Neither can be caused by a candidate whose diff under `crates/` and `packages/` is empty.

`pnpm test` likewise exits 1, with 552 tests passing and three `@verter/typeinfo` resolution tests
failing on the same empty-diff argument. Its first invocation had exited before running a single
test, because a fresh worktree has no built `.node` binding; building it and re-running turned a
vacuous green into three real red tests, which is the direction that record should move.

None of this is rounded off in the lock record. Two acceptance-checklist rows are marked partial for
it, and the record states plainly that the gate does not return PASS on this tree and did not return
PASS on the baseline either.

The gate file's own claim — no placeholders — is verified by running its validator, not by reading the
file. The validator's claim — that it would catch a placeholder — is verified by twenty controls that
each fail against a permissive stub.

Guards whose scan surface includes this content, and which therefore constitute its real coverage:

- `tracked_paths_are_portable` — enumerates `git ls-files`, enforces portable path shapes;
- `tracked_paths_no_machine_roots` — fixed-marker scan of tracked bytes for machine/user path roots;
- `node --test scripts/validate-performance-gates.test.mjs` and
  `node --test scripts/validate-program-state.test.mjs`, both under `pnpm run test:scripts`.

`no_phase_archaeology_in_production_code` scans `crates/*/src/**` and does not see this directory or
`scripts/`; the program-vocabulary prohibition on source is honoured by the added script naming the
program tree by path only, as the existing validator does.
