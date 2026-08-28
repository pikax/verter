# C1 seventeenth deviation — F17: the next safe slice is a sequencing RECORD, not more code

Found while assessing whether round 9's additional groundwork (Part I's
witness-contract precision, `ConsumedResolutionObservationKey`'s now-complete
4-variant shape) changed F15's go/no-go verdict on starting the
`ProjectResolver` -> `ModuleResolverCore` algorithm conversion itself, and
whether two candidate next-slices (a standalone pure-traversal port; stub/
ignored characterization tests) were safely landable now. Consulted before
acting. Full consult prompt/output: `/tmp/c1-phase7-next-slice-prompt.md` /
`/tmp/c1-phase7-next-slice-output.md` (not committed — ephemeral scratch;
this file is the durable record).

## Consult verdict

**Q1 (does the verdict change): DEFER, unchanged.** The correctness
contract is now ready (Part I); the dependency/ownership cutover is not.
What remains structurally unresolved: `verter_semantic` still depends on
`verter_workspace` (not merely one Cargo line — production semantic code
still names workspace-owned resolver re-exports, `WorkspaceRead`, fact
types, `ProjectStableKey`, `AmbientSymbolHit`, `PathProbe`, and
`FactVersionRef`); the semantic-owned immutable `ResolverAttemptView` and
workspace-side construction/translation path remain undesigned and
unwired (F15); `CompletedAttempt<T>`/`KernelAttempt<T>` and `AttemptOutput`
threading still need the real top-level kernel (F16); the single-authority
cutover still needs every resolver entry point on one converted core with
an atomic edge reversal (F12). One precision note: `RecoveryScope` is
correctly a consumed-OUTPUT key (F16/Part I), but production currency
describes it as watcher-advanced rather than a path re-read — supports
keeping it in output/driver translation, NOT inventing a fourth INBOUND
observation method (confirms `path_probe`/`real_path`/`package_manifest`
stay the complete inbound trio; `RecoveryScope` stays output-only).

**Option (a) — a standalone semantic-side port of the pure project-reference
traversal (`resolve_project_references_inner`): REJECTED as a pre-cutover
slice.** My "the traversal is pure" claim was only qualified-true: the DFS
mechanics (reference iteration, graph lookup, active-path cycle guard,
depth decrement/restore, sibling continuation) ARE pure, but the function
itself is NOT a pure graph function — it accepts `WorkspaceRead` and
synchronously calls alias/tsconfig-path/base-url resolution inline
(`resolver.rs:831,836,842`), which reach manifest reads/probes/realpaths.
A pure traversal COULD be factored behind a node-evaluation callback, but
that still wouldn't make the slice safe: the current `Option<String>` walk
encodes blocking hit/miss semantics, while the converted version needs
hit/exhausted-miss/BLOCKED semantics (earlier blocked candidates must
prevent a lower-priority hit from becoming final — directly the "staged
priority-frontier batching" concern F15 already raised); `verter_workspace`
cannot call `verter_semantic` before the edge reversal, so existing
workspace tests can't make a semantic-side port canonical; landing an
unused port would create a SECOND algorithm authority (F15 explicitly
requires pure helpers to move WITH the canonical core, never ahead as
duplicates); `ConfiguredMembership` remains load-bearing for INITIAL
project selection even though it's not touched inside the recursive
function itself, so a projected graph DTO needs its own explicit
ownership/construction ruling, not an inference from the inner loop alone.
**Verdict: extract the traversal DURING the coordinated conversion, with
old workspace tests immediately parameterized/dual-run against the new
core — never landed early as a semantic-side port.**

**Option (b) — ignored/stub semantic witness tests: REJECTED.** Premature
without a real kernel entry point: `#[ignore]` tests still type-check, so
a nonexistent kernel call can't be hidden that way; a stubbed kernel would
either make the test meaningless or create a test-only parallel
implementation; an ignored regression isn't exercised by the canonical
gate and doesn't satisfy the repo's TDD rule. `resolution_witness_
contract_tests.rs` is ALREADY an executable specification — port its two
cases as the FIRST unignored tests against the real kernel seam once that
seam exists, not before.

**Option (c) — a dedicated implementation-sequencing record: ADOPT-NOW.**
"No further ruling is needed to perform that investigation and write the
proposal. Ratification of the resulting sequencing record is required
BEFORE production conversion begins. If it preserves F12's atomic edge
reversal and single core, it is implementation sequencing rather than an
architecture reopen; any third-crate staging, temporary duplicate, or
compatibility resolver would require an explicit deviation ruling." The
record must settle six things:

1. The disposition of every remaining production `verter_semantic ->
   verter_workspace` reference, demonstrating a buildable atomic edge
   reversal.
2. The exact semantic-owned project-resolution DTO/graph shape, including
   where `ConfiguredMembership`-based owner selection ends and pure
   resolver configuration begins.
3. `ResolverAttemptView`, `KernelAttempt<T>`, and workspace replay of
   consumed selectors into versioned facts (including `RecoveryScope` and
   workspace-only `DirectoryMembers`).
4. Same-basis `LoadSet` union, terminal precedence, and attempt-output
   discard rules for the ordered priority-frontier combinator.
5. One atomic migration/deletion table covering `resolve_with_reader`,
   explicit-project resolution, preferred-specifier resolution, workspace
   snapshot fields, and every compatibility re-export.
6. The dual-runner harness plan: existing characterization + witness tests
   as the legacy baseline, the new runner added UNIGNORED as soon as its
   entry point exists.

## Explicit instruction, followed

Write the sequencing-record document now (investigation + proposal, no
production code) — see `docs/arch/refactor/rev11/evidence/C1/
sequencing.md` for round 9's concrete progress on items 1
and 2 (a complete inventory for item 1; a partial finding for item 2) and
the honestly-still-open scope for items 2 (remainder)-6. **Production code
for the actual conversion waits for that record's ratification** — not
attempted this round.
