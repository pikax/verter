# AMD-018 — H3 inherits a pre-landed stale-response debt record and its ignored regressions

**Status:** RATIFIED 2026-08-25 by the codex architect, to whom the maintainer
delegated amendment ratification. See §7.

**Prepared against:** local `program/architecture-lock` commit
`2fe4ad42dfd232f7606c6304ab0528b5112c4d95`.

**Amends on ratification:** nothing in any charter, DAG row, ledger status,
capability-matrix cell, or performance gate. It adds one digest-bound
`AMENDMENT` document record to
[`../../../architecture-lock/ledger/authority-registry.toml`](../../../architecture-lock/ledger/authority-registry.toml),
authored by the registry owner. **It does not edit `charters/H3.md`, does not
set or rebind any `charter_digest`, does not accept, unlock, dispatch, or move
the status of H3 or any other block, and is not an `enabling_amendment` for
anything.**

## 1. Why this exists

A cross-file stale-response defect in the LSP foreground path was found,
scoped, and recorded as a debt record naming H3 as owner. The record and its
two `#[ignore]`d regressions exist in the tree as a candidate. Nothing
authorises them.

Checked against the repository rather than assumed: `authority-registry.toml`
contains no `[[authorization]]` row covering this work; there is no charter for
it; it is not a DAG block. `charters/H3.md` covers H3 *delivering the fixed
behaviour* — it states in its own header that it "is not separately authorized
or dispatched". An acceptance criterion describing the eventual fix does not
cover a different candidate landing evidence ahead of it. So the work is real,
already done, and uncovered.

The alternative to this amendment is not "land it anyway". It is to map the
work to the nearest plausible H3 acceptance row, which would convert a
discoverable gap into an invisible one.

## 2. What is covered, exhaustively

Exactly one artifact set:

1. A debt record under `evidence/H3/` describing the defect as a mechanism,
   its blast radius per LSP surface, and what is deliberately not encoded.
2. A companion red/green proof record for the two regressions.
3. Two `#[ignore]`d regression tests encoding the defect, included into the
   existing LSP test harness.
4. Observer-only event hooks with no production call site.
5. Additions to the test-only mock type provider that let those tests observe
   which file contents the mock program has actually applied.

Nothing else. In particular this amendment covers no change to any LSP
handler, no change to synchronisation or publication behaviour, and no
production code path whatsoever.

## 3. The bounding condition, which is what makes it landable ahead of its owner

**Zero production behaviour change.** This is the entire basis on which
evidence for a locked block may land before that block. It is verifiable
structurally rather than by reading the candidate's description, at four
independent gates:

- the observer module is declared `#[cfg(test)]`;
- the test file is included into a module that is itself declared
  `#[cfg(test)]`;
- the mock provider's whole body — both its re-export and its inner module —
  is declared `#[cfg(test)]`;
- no symbol introduced by the artifact set has any reference outside the
  artifact set itself, in `crates/` or `packages/`.

If any of those four ceases to hold, the artifact set is outside this
amendment and requires fresh authority. The condition is not "the author
states there is no behaviour change"; it is the four gates.

## 4. Consequences

- The artifact set is covered work rather than uncovered work, and may land
  ahead of H3.
- H3 inherits it as evidence. Removing the `#[ignore]` attributes, and any
  assertion tightening that becomes possible once a producer defines the
  wait basis identity, are H3's acts and are covered by H3's own acceptance,
  not by this amendment.
- The recorded deferral of the defect itself rests on this amendment. The debt
  record previously cited a maintainer ruling that resolves to no document
  anywhere in the repository; that citation is removed rather than repeated,
  because an unverifiable authority reference is the same defect class as the
  unbacked evidence claim this candidate was corrected for.
- A recorded, unresolved deviation stands and is not settled here: the ignore
  reasons and the debt record name prerequisites `F1/F2/G2/H2`, while
  `charters/H3.md` declares predecessors `F1, H1, H2`. The two disagree in both
  directions. Reconciling them would require authority over H3's charter, which
  this amendment does not take.

## 5. What this does NOT do

- It does not accept, re-open, unlock, dispatch, or move the status of H3 or
  any other block. H3 stays LOCKED with an unsatisfied predecessor set.
- It does not edit `charters/H3.md`, and sets no `charter_digest`.
- It does not authorise un-ignoring either regression, any H3 behaviour, or any
  part of the eventual cutover.
- It does not resolve the predecessor-set divergence in §4.
- It creates no precedent that evidence may generally land ahead of its owning
  block. It decides one artifact set, bounded by the §3 gates.
- It changes no ADR, DAG row, capability-matrix cell, or performance gate.

## 6. Verification on ratification

1. Confirm the four §3 gates hold on the candidate tree, by reading the
   declarations rather than the candidate's description.
2. Confirm `authority-registry.toml` gains exactly one digest-bound
   `AMENDMENT` document record for this file, using the SHA-256 of this
   amendment after the ratification text and any corrections are present.
   No `[[authorization]]` record is created, because no block leaves LOCKED.
3. Confirm no charter file, `charter_digest`, `program_dag_digest`, or block
   status changed.
4. Run:
   ```sh
   node scripts/validate-program-state.mjs \
     --dag docs/arch/refactor/rev11/program-dag.toml \
     --state docs/arch/architecture-lock/ledger/program-state.toml \
     --mode live \
     --authority docs/arch/architecture-lock/ledger/authority-registry.toml
   ```
   Expected: the same block count and zero violations as before this amendment,
   with the registry read explicitly. An unchanged-result assertion that omits
   the authority argument does not read this amendment and is not verification.

## 7. Ratification

**RATIFIED**, 2026-08-25, by the codex architect acting under the maintainer's
delegated amendment-ratification authority. Lane `amd018-ratification`,
`RESULT: PASS`, `FINDINGS: none`, reviewed
`ab902a184a076902a521f9d90a8db73b72a182c9`. Ratifiable as written; no
corrections were required.

The consult confirmed independently that the four §3 gates hold by reading the
declarations rather than this document's description of them, that the
amendment grants nothing beyond the artifact set, that binding by amendment
alone is the correct instrument while H3 stays LOCKED, and that resting the
deferral on this amendment and recording the predecessor-set divergence
unresolved are both correct.

**Disclosure, because a ratification binds to what was read.** The amendment
text is byte-unchanged since the reviewed sha. The artifact set has since
changed in three ways: one regression test was renamed after the proposition it
actually proves, a second rustdoc comment was corrected to name all four file
operations, and the evidence records were regenerated. None of the three
touches the §3 gates, and all four gates were re-verified after those changes.
A change that did touch a gate would put the artifact set outside this
ratification.
