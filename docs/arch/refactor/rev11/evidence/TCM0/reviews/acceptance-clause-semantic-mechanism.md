# Acceptance clause — "semantic mechanism TBD"

The record of an independent verdict on one condition of this block's Acceptance clause. It is filed
here because a row that rests on a lane must rest on a receipt a reader can open, and the receipt this
row originally cited was not in this tree at all, at a sha that was not an ancestor of the candidate.

## The verdict

    ===VERTER-RECEIPT-BEGIN===
    LANE: tcm0-aa-bounded2
    RESULT: PASS
    REVIEWED: a7c93ac764702a3fed19c860d4b54f190c3eb523
    SUBJECT: 51 paths under evidence/TCM0 (excluding this verdict's own receipt),
             evidence/TCM0-summary.md, charters/TCM0.md
    SUBJECT_DIGEST: dedaf684b36586d19975a1afb5fff8021ba378e15243700597896d78967799f5
    FINDINGS: 0
    ===VERTER-RECEIPT-END===

## The subject was bounded BEFORE the lane ran

The path set and its content digest were written into the brief before the lane started, not derived
afterwards from the result. That ordering is the difference between a bounded verdict and a post-hoc
excuse, and it is checkable rather than argued:

    git ls-files -- docs/arch/refactor/rev11/evidence/TCM0 \
                    docs/arch/refactor/rev11/evidence/TCM0-summary.md \
                    docs/arch/refactor/rev11/charters/TCM0.md \
      | grep -v 'reviews/acceptance-clause-semantic-mechanism.md' | sort \
      | while IFS= read -r f; do printf '%s  %s\n' "$(shasum -a 256 "$f" | cut -d' ' -f1)" "$f"; done \
      | shasum -a 256

**This file is excluded from that set on purpose.** A verdict cannot cover the record of itself without
circularity, so the act of writing the result down does not disturb what the result is about. The
block's other review records stay in subject; only the one downstream of this lane is carved out.

**`closure-register.md` is deliberately kept IN**, even though this verdict's citation is written into
it. A register row could itself defer a mechanism, which is the very condition under test, so excluding
it would trade real coverage for a tidier boundary. The residue is named rather than hidden: the
citation edit still moves the subject, and is admitted under the exception below — now over one file
rather than two.

## Declared exemption: layout-only changes

Stated in the brief in advance, not invented afterwards. A layout-only change to a subject path does not
disturb this verdict, and layout-only is **proven by reconstruction**, never by inspection:

    node_modules/.bin/oxfmt <pre-change copy> && cmp <pre-change copy> <post-change file>

If the reconstruction does not reproduce the committed bytes exactly, the change is not layout-only and
this verdict is void. An exemption with a mechanical proof attached is a bounded rule; the same words
without one are an excuse.

This exemption exists because an earlier run of this lane was destroyed by a reformat. Two probe scripts
in the delta were unformatted, one of them inside the subject; fixing them moved the subject, and the
verdict could not survive because "formatting does not change meaning" was a carve-out being written
after a result was already in hand. The real defect was upstream of both: **the lane had certified a
tree that could not be committed.** The formatter check is a precondition of FREEZING, not a step of
landing, and it is now run before anything is frozen.

## How the verdict was obtained

An unseen lane, given the charter and the tree and **no prior findings, no earlier lane's conclusions,
and no list of places to look**. It derived both limbs itself: what the condition forbids — reasoned
from the charter's Scope, Non-scope and purpose, since the condition is a phrase in a list rather than a
definition — and whether this candidate contains it. It was told in terms that reporting no violation
was acceptable and expected, so a clean result could not be read as a lane looking for something to
justify itself.

Its diff range was pinned to the frozen sha, never a branch name. The receipt guard was proven in six
directions before the wait began — including against a prompt template echoed into the output, and
against the superseded lane's own **real** PASS, whose name is a prefix of this one's; the trailing
newline in the lane anchor is what refuses it.

The subject digest was verified three ways rather than read out of the receipt: declared, echoed by the
lane, and re-derived from the tree, all identical. A lane can echo a digest it never computed.

## Why a verdict on `a7c93ac76` is admitted for the tree that lands

Committing this file and the register row that cites it produces a different tree, and that later tree
is what lands. This file is outside the subject, so only the register edit moves it. That is admitted
**only** because the delta is the record of the verdict itself — a receipt no row cites is inert, so the
record is both files or neither.

**This is not a precedent for a verdict covering a tree it did not review.** The general rule is the
opposite and is enforced: a verdict bound to one sha does not cover another, and a rebase that moves a
freeze voids the verdict taken over it — which is why an earlier run of this lane was discarded rather
than argued for when trunk moved a single commit that overlapped nothing this block owns.
