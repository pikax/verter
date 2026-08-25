---
ruling_id: "SIX-WAY-B6-CM1-2026-08-24"
type: "architecture-ruling"
date: "2026-08-24"
date_source: "stated"
binds: ["B6", "BF1", "CM1", "governance.md", "performance-gates.toml"]
source_file: "ARCHITECT-RULING-2026-08-24-SIX-WAY-B6-CM1.md"
summary: "Six-question architecture ruling taken on 4643edbaa. Q1 rules A: authorize a narrow amendment adding B6 to the enumerated legacy-gap set. Q2 rules A: authorize the mechanically extended harness under independent authority, pre-pinned before its results are read, with lock amendment and affected reruns. Q3 rules a narrower third option: all three mandates rebind to 3ae319a23/8547c26b8, adversarial lightly, while architecture and conformance inspect the B6-P production delta plus the final test-only hunks and affected evidence. Q4 rules A: record the registered B6 charter digest now; the header is stale, non-authoritative prose. Q5 rules a third option: CM1 adds only two discriminating ignored captures, and the existing planned post-program maintainer type-correction work owns both repairs. Q6 rules B: the root-session test proves compat-shaped output but not the /compat checker surface, and CM1 already owns the missing checker batch projection. The receipt records RESULT: FAIL with four P1 findings."
supersedes: []
superseded_by: []
contradicts: []
notes: "Transcribes the ruling seat's verdict verbatim. The seat was acting under explicit maintainer delegation and its verdict is the recorded decision. The ruling itself states that the dispatch was primed toward position A on Q2, Q3 and Q5, and that it ruled on the evidence regardless — recorded here because it bears on how the ruling is read. The source artifact contained the ruling body twice (an echoed final turn from the producing tool, with a `tokens used` line between the copies); the two copies were verified byte-identical before one was landed and the echo dropped. RESULT: FAIL is the receipt's finding count, not a defect in the ruling — the four P1 findings are open blockers carried by the reviewed tree."
---

# Architect ruling — six questions on B6 context, gate lock, mandate rebinding, charter digest, CM1 captures, and the compat matrix cell

**Date:** 2026-08-24
**Status:** RATIFIED — architecture ruling issued under explicit maintainer
delegation; the ruling seat's verdict is the recorded decision.
**Authority:** architecture ruling seat, lane `architecture-sixway`, dispatched
against `4643edbaa909268ec1152610f3af97a2d8135526` under the delegated
ruling authority recorded for this program.
**Supersedes:** none.

## Provenance

- **Reviewed sha:** `4643edbaa909268ec1152610f3af97a2d8135526` — the tip of
  `program/architecture-lock` at the time of the ruling.
- **Lane:** `architecture-sixway`.
- **Result artifact:** `~/.claude/briefs/rev11/verify/results/RULING/4643edbaa909268ec1152610f3af97a2d8135526/architecture-sixway.out`.
- **Receipt validation.** Re-run at transcription time:

```text
$ node scripts/orchestration/check-results.mjs \
    ~/.claude/briefs/rev11/verify/results/RULING/4643edbaa909268ec1152610f3af97a2d8135526 \
    4643edbaa909268ec1152610f3af97a2d8135526 architecture-sixway
OK      architecture-sixway FAIL  blockers=4 carried=0  8335B
          FINDING Q1-CONTEXT | P1 | docs/arch/refactor/rev11/governance.md:183 | B6 was dispatched and landed without the mandatory immutable context packet.
          FINDING Q2-GATE | P1 | performance-gates.toml:805 | The required four-arm gate pins a harness that refuses three arms, making the cell unexecutable.
          FINDING Q5-CAPTURES | P1 | docs/arch/refactor/rev11/rulings/MAINTAINER-RULING-BUGS-AND-TYPES.md:39 | Two demonstrated type defects lack the mandatory ignored discriminating capture tests.
          FINDING Q6-MATRIX | P1 | docs/arch/refactor/rev11/charters/CM1.md:167 | The required batch-by-compat acceptance cell has no compat-checker implementation or test.
          ! the result appears 2 times, identical — an echoed final turn
ALL SOUND — 1 result(s) on 4643edbaa909 blockers=4 carried=0
```

- **Delegation.** The ruling seat was acting under explicit maintainer
  delegation. Its verdict is the recorded decision; this document transcribes
  it and does not re-decide any part of it.
- **Priming, recorded because it bears on how the ruling is read.** The ruling
  itself states that its dispatch was primed toward position A on Q2, Q3 and
  Q5, and that it ruled on the evidence regardless. In its own words: Q2 — "The
  presentation is A-primed, but the deciding invariant is the independent
  recalibration procedure at `governance.md:169-177`"; Q3 — "The presentation is
  A-primed; rule a narrower third option"; Q5 — "A is AMD-009-primed, but the
  specific type waiver controls." Q3 and Q5 did not return the primed position.
- **Duplicate-copy handling.** The source artifact carried the ruling body
  twice — an echoed final turn from the producing tool, separated by a
  `tokens used` line, which the result checker flags as "the result appears 2
  times, identical". The two copies were compared and are byte-identical
  (sha256 `9ff608a81aeb3f45d9d6cb38f30a1f4ab4ec48c7b504606598489f487fdbeea2`
  for each). One copy is landed below verbatim; the echo and the `tokens used`
  line are dropped.

## The ruling

Q1: Before the positions, architecture requires an immutable worker context packet and its digest at REVIEW or later (`governance.md:112-123,183`; `scripts/validate-program-state.mjs:1677-1696`). The deciding invariant is `governance.md:183`. Rule A: authorize a narrow amendment adding B6 to the enumerated legacy-gap set; B would preserve the historical violation as a permanent deadlock. This changes governance, not B6’s charter; the missing historical packet remains unrecoverable.

Q2: Before the positions, BF1/ADR-016 owns the locked four-arm, eight-source gate, while B6 owes compliance (`ARCHITECT-RULING-2026-08-23-B6-ROUTE-OVERHEAD-CELL-LOCK.md:30-37`; `performance-gates.toml:809-825`). The presentation is A-primed, but the deciding invariant is the independent recalibration procedure at `governance.md:169-177`. Rule A: authorize the mechanically extended harness under independent authority, pre-pinned before its results are read, with lock amendment and affected reruns. This does not expand B6; BF1 owns reconciliation, and the three added-arm metrics remain uncovered until it lands.

Q3: Before the positions, every mandate must bind the exact candidate, but reattestation is impact-bounded unless architecture, authority, compatibility, lifetime, platform, or blast radius materially changes (`governance.md:251-277`; `program-state.toml:66-74`). The presentation is A-primed; rule a narrower third option: all three mandates rebind to `3ae319a23`/`8547c26b8`, adversarial lightly, while architecture and conformance inspect the B6-P production delta plus the final test-only hunks and affected evidence. The unrelated orchestration delta remains outside B6 scope; full rechecks under B are unwarranted.

Q4: Before the positions, block authority is the digest-bound registry authorization, and CHARTER status prose is expressly excluded from status classification (`authority-registry.toml:3-22,529-546`; `scripts/validate-program-state.mjs:2274-2295`). The deciding invariant is exact-byte binding at `scripts/validate-program-state.mjs:2274-2312`. Rule A: record the registered B6 charter digest now; the header is stale, non-authoritative prose.

Q5: Before the positions, CM1 owns only Findings B/C and `UnraisableSource` and must stop on a third defect class; C3 excludes runtime-form syntax, while the later program-wide type waiver forbids opening type-correctness work and requires an ignored test with a named owner (`CM1.md:213-221`; `C3.md:30-36`; `MAINTAINER-RULING-BUGS-AND-TYPES.md:35-47`). A is AMD-009-primed, but the specific type waiver controls. Rule a third option: CM1 adds only two discriminating `#[ignore]`d captures; neither repair enters CM1 or a new immediate block. The existing planned post-program maintainer type-correction work owns both, which remain uncovered until it lands.

Q6: Before the positions, CM1 requires the full cross of batch invocation and the `@verter/component-meta/compat` surface (`CM1.md:165-183`); projection doctrine requires one native request plus mechanical mapping, not absence of the compat API (`CLAUDE.md:213-219`). Rule B: the root-session test proves compat-shaped output but not the `/compat` checker surface. CM1 already owns the missing checker batch projection, so this is no charter expansion; the cell remains uncovered until that method and its test land.

## Receipt

```text
===VERTER-RECEIPT-BEGIN===
LANE: architecture-sixway
RESULT: FAIL
REVIEWED: 4643edbaa909268ec1152610f3af97a2d8135526
FINDINGS: 4
FINDING Q1-CONTEXT | P1 | docs/arch/refactor/rev11/governance.md:183 | B6 was dispatched and landed without the mandatory immutable context packet.
FINDING Q2-GATE | P1 | performance-gates.toml:805 | The required four-arm gate pins a harness that refuses three arms, making the cell unexecutable.
FINDING Q5-CAPTURES | P1 | docs/arch/refactor/rev11/rulings/MAINTAINER-RULING-BUGS-AND-TYPES.md:39 | Two demonstrated type defects lack the mandatory ignored discriminating capture tests.
FINDING Q6-MATRIX | P1 | docs/arch/refactor/rev11/charters/CM1.md:167 | The required batch-by-compat acceptance cell has no compat-checker implementation or test.
===VERTER-RECEIPT-END===
```
