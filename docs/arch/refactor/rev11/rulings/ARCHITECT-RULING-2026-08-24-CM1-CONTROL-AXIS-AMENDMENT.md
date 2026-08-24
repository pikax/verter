---
ruling_id: "CM1-CONTROL-AXIS-AMENDMENT-2026-08-24"
type: "architecture-ruling"
date: "2026-08-24"
date_source: "stated"
binds: ["CM1"]
source_file: "ARCHITECT-RULING-2026-08-24-CM1-CONTROL-AXIS-AMENDMENT.md"
summary: "Ratifies charter amendment A for CM1, with clarification. CM1 is unsatisfiable as written: CM1.md:118, :174 and :187 demand green evidence, while an ignored capture documents a deferred defect and cannot satisfy a demanded cell. B is rejected. Under the ratified amendment the green negative control is restricted to module-owned and imported custom classes; PropType<T> and setup-local custom classes are removed from that control and left solely as the two settled ignored captures. This corrects a false premise without weakening valid acceptance coverage; under the amendment no demanded cell is unevidenced, whereas B would permit exactly that."
supersedes: []
superseded_by: []
contradicts: []
notes: "Transcribes the ruling seat's verdict verbatim. The seat was acting under explicit maintainer delegation and its verdict is the recorded decision. The ruling itself states that the dispatch was primed toward A by its stipulated failures, and that ruling on the governing text alone reaches the same result — recorded here because it bears on how the ruling is read. The source artifact contained the ruling body twice (an echoed final turn from the producing tool, with a `tokens used` line between the copies); the two copies were verified byte-identical before one was landed and the echo dropped."
---

# Architect ruling — CM1 charter amendment: the green negative control's class axis

**Date:** 2026-08-24
**Status:** RATIFIED — architecture ruling issued under explicit maintainer
delegation; the ruling seat's verdict is the recorded decision.
**Authority:** architecture ruling seat, lane `architecture-cm1-amend`,
dispatched against `4643edbaa909268ec1152610f3af97a2d8135526` under the
delegated amendment-ratification authority recorded for this program.
**Supersedes:** none.

## Provenance

- **Reviewed sha:** `4643edbaa909268ec1152610f3af97a2d8135526` — the tip of
  `program/architecture-lock` at the time of the ruling.
- **Lane:** `architecture-cm1-amend`. **Result:** PASS, findings none.
- **Result artifact:** `~/.claude/briefs/rev11/verify/results/RULING/4643edbaa909268ec1152610f3af97a2d8135526/architecture-cm1-amend.out`.
- **Receipt validation.** Re-run at transcription time:

```text
$ node scripts/orchestration/check-results.mjs \
    ~/.claude/briefs/rev11/verify/results/RULING/4643edbaa909268ec1152610f3af97a2d8135526 \
    4643edbaa909268ec1152610f3af97a2d8135526 architecture-cm1-amend
OK      architecture-cm1-amend PASS  blockers=0 carried=0  1786B
          ! the result appears 2 times, identical — an echoed final turn
ALL SOUND — 1 result(s) on 4643edbaa909 blockers=0 carried=0
```

- **Delegation.** The ruling seat was acting under explicit maintainer
  delegation. Its verdict is the recorded decision; this document transcribes
  it and does not re-decide any part of it.
- **Priming, recorded because it bears on how the ruling is read.** The ruling
  itself opens by stating that "The dispatch is primed toward A by its
  stipulated failures; ruling on the governing text alone reaches the same
  result."
- **Duplicate-copy handling.** The source artifact carried the ruling body
  twice — an echoed final turn from the producing tool, separated by a
  `tokens used` line, which the result checker flags as "the result appears 2
  times, identical". The two copies were compared and are byte-identical
  (sha256 `870555c371622de41986c6699aa5404b6fe2577ab3506c6f34eb7f84be8f1180`
  for each). One copy is landed below verbatim; the echo and the `tokens used`
  line are dropped.

## The ruling

The dispatch is primed toward A by its stipulated failures; ruling on the governing text alone reaches the same result. CM1 is unsatisfiable as written: `CM1.md:118`, `:174`, and `:187` demand green evidence, while an ignored capture documents a deferred defect and cannot satisfy a demanded cell. B is rejected.

A is ratified with clarification: restrict the green negative control to module-owned and imported custom classes; remove `PropType<T>` and setup-local custom classes from that control, leaving both defects solely as the two settled ignored captures. This corrects a false premise without weakening valid acceptance coverage. Under the amendment no demanded cell is unevidenced; B would permit exactly that.

## Receipt

```text
===VERTER-RECEIPT-BEGIN===
LANE: architecture-cm1-amend
RESULT: PASS
REVIEWED: 4643edbaa909268ec1152610f3af97a2d8135526
FINDINGS: none
===VERTER-RECEIPT-END===
```
