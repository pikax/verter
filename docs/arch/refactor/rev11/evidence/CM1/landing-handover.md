# Landing handover

What the landing sequence needs, and what a successor should not spend time re-deriving.
The block does not execute any of the landing steps; this records the inputs to them.

## Authority-registry rows the landing sequence must author

Two rulings bind CM1 and need `authority-registry.toml` rows. **The block must not author them** —
the registry is trunk-side and orchestrator-owned, the same class as `base_sha`, and a
branch-authored row is dropped on rebase rather than repaired.

| `id` | `kind` | `path` |
|---|---|---|
| `RULING-2026-08-25-CM1-AUTHORED-ASSERTION-CAPTURE` | `RULING` | `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-25-CM1-AUTHORED-ASSERTION-CAPTURE.md` |
| `RULING-2026-08-24-CM1-RUNTIME-FORM-AXIS` | `RULING` | `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-CM1-RUNTIME-FORM-AXIS.md` |

The second is superseded by the first and is transcribed only so the `supersedes` chain resolves;
register it or not as the registry's own convention dictates, but do not delete the file — the
successor cites it.

**Do not copy a digest from here.** Each row needs a `sha256` of its file, and a digest quoted in
prose is stale the moment the file is touched — this document has already made that mistake once.
Derive both at landing time, from the tree being landed:

```
for f in docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-2{5-CM1-AUTHORED-ASSERTION-CAPTURE,4-CM1-RUNTIME-FORM-AXIS}.md; do
  printf '%s  %s\n' "$(shasum -a 256 "$f" | cut -c1-64)" "$f"
done
```

`node scripts/effective-state.mjs` must exit 0 after the rows land. Note its failure mode: an
unparseable ruling does not get skipped, it makes the deriver return ZERO rulings for every block —
so a malformed record is not a local defect, it silently empties the rail that several landing
records take as an input.

## Ledger row — intended values, with derivations

The landing sequence writes these. The block does not.

| Field | Intended value | Derivation |
|---|---|---|
| `candidate_sha` | the final candidate at ready-time | `git rev-parse HEAD` on `block/cm1-charter-completion` |
| `candidate_tree` | that candidate's tree | `git rev-parse HEAD^{tree}` — survives a squash unchanged, see below |
| `evidence_digest` | recomputed, never carried | `shasum -a 256` over `evidence/CM1/2026-08-23-charter-completion.md` |
| `base_sha` | **do not write** | orchestrator-owned; see the next section |
| `landing_equivalence_digest` | **do not write** | accepted identity is empty, so there is no candidate/accepted divergence to prove |
| `status`, `accepted_sha`, `accepted_tree`, `maintainer_decision` | **do not write** | acceptance is the maintainer's act alone |
| `context_packet_digest` | stays empty | grandfathered by name in the validator; nothing is reconstructed |

A squash preserves the tree exactly — a synthetic squash of this branch's tip onto its merge-base
produced an identical tree hash — so `candidate_tree` taken before the squash remains correct after
it. A rebase does not preserve the tree, so `candidate_tree` must be taken after the landing
rebase, not before.

## Why `base_sha` must not be written by this block

This branch carried a stale `base_sha` for 27 commits. Trunk's owner had since rebound the field;
the branch's older value won on rebase, because a rebase faithfully replays the branch's intent and
the branch's intent was stale. Landing it would have silently reverted an orchestrator-owned field
inside a 29-commit diff that nobody reads line by line.

It was restored in-branch so the branch carries no edit to a field it does not own. **Two things
follow for anyone touching this row:**

1. A tool that guards *writes* cannot catch this. The revert was already committed, so there was
   nothing left to write and nothing for a write-guard to refuse.
2. Rebase integrity is not row equivalence. Every signal was green — patch-ids 1:1, clean tree,
   byte-identical delta — with the revert riding inside. The only thing that caught it was
   field-diffing the row against the merge-base.

The check that catches it is a field-diff of the block's ledger row, **baselined on the
merge-base**, failing on any change to a field the block does not own. Baselining on trunk instead
reports fields trunk *added* as fields the block *deleted*.

## Settled — do not re-derive

Each of these cost real effort and the reasoning is recorded, not just the conclusion.

- **The two replay-conflict paths have an empty intersection with everything this block ever
  changed.** Proven, and the set does not move when the block's range is corrected. What was NOT
  proven, and stays withdrawn, is that the stale integration pin was *the cause* — the validator
  treats a stale ancestral pin as valid input. That disposition belongs to the orchestrator.
- **The validator reports different single violations in different trees, and both are true.** In
  the block worktree it reports the row's own identity; on trunk it reports the landing-order replay
  conflict. Different ledgers, different violations. A validator result is meaningless without the
  checkout it was taken in. Do not adjudicate between two such reports — re-run in each
  environment, because the difference is usually the finding.
- **Artifact provenance binds to a digest over the artifact's inputs, never to a commit sha.** See
  `binary-provenance.md`. The landing sequence moves a commit twice, by rebase and by squash.
- **`mixed runtime + type-declared` is DETECTED without `PropType<T>`, via `X as () => T`, and is
  still not publishable.** This entry was written asserting the form was realisable, and execution
  falsified it within the hour — it is kept in that corrected form rather than deleted, because the
  half that is true keeps being rediscovered and the half that was false is worth not repeating.
  TRUE: the analyzer's doc comment lists three authored-type-position rules and `prop_mixed_fixture`
  exercises them, so detection accepts both spellings. FALSE, and the error: inferring from
  detection that the value was therefore coverable. Both spellings lose the authored payload at one
  publication site — runtime object members begin as a closed `Unknown` leaf and the props
  normalizer selects it before consulting the authored payload. Ratified as a deferred capture in
  `rulings/ARCHITECT-RULING-2026-08-25-CM1-AUTHORED-ASSERTION-CAPTURE.md`.
  Two mistakes to not repeat: absence of a test cell is evidence of a coverage gap and says nothing
  about what is coverable; and detection succeeding says nothing about publication surviving.
- **The compat checker's batch method must reuse the checker's own per-item mapping**, not delegate
  to the root session's — the two mappings differ, and delegating makes checker-batch disagree with
  checker-scalar. A per-id fallback loop can also serve "batch" as N scalar dispatches, so a test
  comparing only values proves nothing about dispatch shape.

## A caveat on the supersession this block records

The two CM1 rulings use the corpus's `supersedes:` / `superseded_by:` block-mapping form, and each
entry carries a `claim:` string scoping the supersession to the conclusion it overturns while
preserving the detection evidence the successor cites.

**`claim:` is documentation. No tooling reads it.** The deriver reduces every entry to
`entry.ruling` and supersedes the target *whole*; the only tooling that touches `claim:` is the
generator that writes it. So a supersession written as scoped to one conclusion is, mechanically,
total.

That is harmless here — the superseded ruling's operative content WAS its conclusion, and its
detection evidence survives as a fact cited by the successor rather than as a live ruling. It stops
being harmless the moment someone relies on `claim:` to supersede part of a ruling and expects the
remainder to stay in force. It will not.

Two related gaps in the same machinery, both pre-existing and neither this block's to fix:
`supersededByEdges` is built and then discarded, so a reverse-only supersession cycle is never
detected; and a bare-string supersession entry is silently accepted, which is why the defect this
block hit was invisible — the corpus diverged from the generator's shape while the deriver's own
tests stayed green, because those tests cover the deriver and not the corpus. The parser is already
strict elsewhere, so rejecting a non-mapping entry is a small change to it rather than a new guard.

## Squash message

Drafted at verification, screened mechanically for forbidden tokens rather than by eye. Held with
the ready report; the landing sequence uses it verbatim and returns the block rather than rewriting
a non-compliant one.
