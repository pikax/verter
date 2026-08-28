# Review reports for the TCM-plan integration (TCM0 evidence tree)

## Round history

| round | reviewed SHA | seats | verdict | report |
|---|---|---|---|---|
| 1 | `84a5018fe` | conformance/architecture/adversarial, `gpt-5.6-sol` @ xhigh | FAIL / FAIL / FAIL | not committed at the time — see "Process gap" below; findings are recorded instead in [`../OPEN-GAPS.md`](../OPEN-GAPS.md) and the fix commit `fb8f9b28e` |
| 2 | `da31a892d` | conformance/architecture/adversarial, `gpt-5.6-sol` @ xhigh | BLOCKING / BLOCKING / BLOCKING | [`round2-conformance.md`](round2-conformance.md) / [`round2-architecture.md`](round2-architecture.md) / [`round2-adversarial.md`](round2-adversarial.md) |

Round 1 ran against `84a5018fe`. Its findings were addressed by `fb8f9b28e` (cache-key ABI fix,
ledger taxonomy/#5a, `OPEN-GAPS.md`) and `850886bca` (partitioning the combined gap rows into
single-owner entries and fixing the resulting rows #25-26 acceptance-sequencing cycle). No round
naming `850886bca` as the reviewed SHA was ever run before this file existed — that gap, and a
narrated "clean 3/3" that had no artifact backing it, is why round 2 exists and is committed here
in full, run against `da31a892d` (this same session's own fix of the rows #25-26 cycle and the two
`OPEN-GAPS.md` "blocked" headings, on top of `850886bca`).

## Round 2 result: BLOCKING on all three legs — by design, not a regression

Round 2's architecture leg explicitly confirms the fix this round exists to verify: **"The round-1
rows #25-26 sequencing defect and forbidden 'blocked' headings are fixed"**
([`round2-architecture.md`](round2-architecture.md)). All three legs nonetheless returned BLOCKING,
for two different reasons:

1. **The large majority of findings re-state gaps `OPEN-GAPS.md` already names, owns, and gates**
   (no topology-benchmark numbers, no full performance-baseline table, feature-ownership coverage
   beyond the 44 `TypeProvider` methods, the rows #25-26 ruling not yet obtained, deletion-closure
   items deferred to TCM4). Every reviewer defaults to BLOCKING for an uncited or unresolved
   criterion, per this program's own mandatory instruction — so an HONEST, already-disclosed,
   TCM0-owned gap reads as BLOCKING every time it is reviewed, by design, until TCM0's own
   acceptance actually closes it. This is not something a docs-integration review-response pass can
   or should close by editing prose; doing so would be exactly the "narration is not evidence"
   failure this pass exists to eliminate.
2. **Two genuinely new, integration-fixable defects were found and fixed in this same pass** (both
   pre-existing TCM0 evidence this integration had not previously touched, surfaced only because
   round 2 was dispatched for real instead of narrated):
   - `cache-lifecycle-contracts.md`'s derived-serialization-key section listed four permitted
     terminal key axes and then, immediately below, forbade "anything not already reachable from
     the prepared-artifact identity" — which forbids three of the four axes it had just permitted.
     Narrowed the prohibition to source/semantic-computation dependencies; the four terminal axes
     are now explicitly carved out.
   - `mapping-products-string-surface.md` cited `VerterTsxBlock.source_map` at
     `compile/types.rs:497` (the real field is at `:495`, fixed in place) and named two structs,
     `SvelteClientOutput` and `SvelteIdeProjector`, that do not exist anywhere in the repository
     (confirmed by a fresh `grep -rn` returning zero hits). The line-number fix was applied in
     place (unambiguous, independently verifiable); the struct-existence problem is recorded as a
     new named gap, `G-STRING-SURFACE-CITATIONS` in [`../OPEN-GAPS.md`](../OPEN-GAPS.md), owned by
     TCM0 (this file predates this integration's baseline and TCM1's charter cites it by reference
     rather than re-deriving it), rather than fixed by guessing a replacement inventory under
     review pressure.
   - `feature-ownership-ledger.md`'s "Summary counts" section (near line 119) carried an ambiguous
     sentence ("the consult is TCM3's exit criterion") that could be misread, out of context, as
     TCM3 owning the ruling — tightened to state TCM0's ownership and TCM3's downstream-only role
     explicitly, matching the corrected "rows #25-26" section below it.

No round 3 is dispatched: a literal clean 3/3 is not achievable by a docs-integration pass while
TCM0's own, already-disclosed evidence-completeness gaps remain open by design — chasing one would
mean either fabricating evidence (forbidden) or silently weakening a reviewer's default-to-BLOCKING
posture (also forbidden). Round 2 is committed in full specifically so that claim is auditable
rather than asserted.

## Process gap: an earlier dismissal of a real finding was not persisted

An earlier pass of this same review cycle received a finding that a commit message (`6bb2872f2`)
contains the phrase `certifies no package`, framed as a self-contradiction against
`authority-registry.toml`'s TCM0 scope text. That finding was rejected in conversation, on the
stated grounds that `git log` showed the phrase absent. **That rejection reasoning was wrong on
its face — the phrase is present**, both in `6bb2872f2`'s own commit message and quoted verbatim
by `fb8f9b28e`'s commit message (which cites fixing "the TCM0 authorization scope text's
self-contradiction ('certifies no package' beside a document that does)"). No report or note
recording the finding, the rejection, or the reasoning was committed anywhere, so the dismissal
could not be audited — a violation of this program's own review-persistence requirement.

**Re-opened and evaluated on the merits against the current tree (`da31a892d`):**

- `docs/arch/refactor/rev11/evidence/TCM0-summary.md:5` reads "...certifies no package by
  assumption, and deletes nothing — every finding below is against bytes actually inspected or a
  probe actually run, not against documentation alone..." — qualified (`by assumption`), not a bare
  "TCM0 certifies no package" claim.
- `docs/arch/architecture-lock/ledger/authority-registry.toml:515` (the TCM0 `[[authorization]]`
  scope text) reads "TCM0 does not itself certify a package by INVESTIGATION AUTHORITY ALONE...
  but TCM-PACKAGE-CERTIFICATION-SETTLED, cited below as a second document backing this same
  record, is the maintainer's OWN ruling doing exactly that certification directly..." — this
  explicitly distinguishes "TCM0's own investigation authority" (which does not certify) from "the
  authorization record as a whole" (which includes a separate maintainer-ruling document that
  does certify), and is not self-contradictory as currently written.
- Both occurrences of "certifies no package" in the live tree are quoted in full above; neither is
  a bare, unqualified claim beside a document that certifies. The self-contradiction the original
  finding named was real against `6bb2872f2`'s state and was fixed by `fb8f9b28e`
  (`authority-registry.toml` gained the "INVESTIGATION AUTHORITY ALONE" / "second document" split
  quoted above). No further correction to this text is warranted by this finding as of `da31a892d`.

**Verdict on re-open: the underlying self-contradiction is FIXED, not present in `da31a892d`. The
rejection of the *finding* was directionally right (no live defect), but the *reasoning* given
("`git log` proves the phrase absent") was factually false and would have failed an audit — this
file is that audit record, going forward.** Any future rejection of a review finding in this block
is recorded here (or in a round file below) with the reviewer's exact text and the evidence for
the rejection, per this program's rule that a dismissal with no artifact is indistinguishable from
suppressing a real finding.
