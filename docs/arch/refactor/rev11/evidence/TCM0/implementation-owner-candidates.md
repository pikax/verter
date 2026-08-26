# Which owners are LEGAL for `implementation` — derived, not preferred

**`implementation`'s disposition is complete.** Its survival and its primary execution model are both
bound, by acts `15db16e7b` and `3a9ba83d9`; what they decide is the registry's to state and is not
repeated here.

**This file is the record of the derivation that preceded the second act, and it is deliberately not
rewritten to match the outcome.** When it was written the execution model was unnamed — the migrate
act left the target to the receiving owner — and naming the legal candidates was this block's work
while choosing among them was not. That is the state the derivation below addresses. An earlier
revision of this paragraph still described that state in the present tense after the second act had
named the model, which read as though the choice were open.

**This file derives the LEGAL candidate set. It does not choose.** Choosing among a plural set is not
this block's act, and narrowing a plural set by preference dressed as structure is the failure mode this
derivation exists to avoid — so every exclusion below is stated as a structural criterion a reader can
falsify, never as a judgement about which outcome is better.

## The criteria, taken from how the other 31 rows actually use each owner

Not invented here. Read off `feature-ownership-ledger.md`'s own classification: 15 rows
`TypeScriptLspDirect`, 15 `VerterWithTypeSemanticOracle`, 9 `VerterNative`, 1
`DisabledByExplicitApprovedContract` (rows carrying a split are counted under both).

| owner | the structural test the 31 rows apply |
|---|---|
| `VerterNative` | the feature is answered end to end inside Verter with **no TypeScript capability required** — the exemplar, row 1, records "needs TS: none" |
| `TypeScriptLspDirect` | TypeScript answers the feature **directly against the mapped file**, and the result is mapped back to the carrier |
| `VerterWithTypeSemanticOracle` | Verter executes the feature but **consults the TypeScript `Program`/`Checker`** for underlying type facts it does not itself own — used where the mapped file's own AST cannot express what the answer needs |
| `DisabledByExplicitApprovedContract` | the capability is **deliberately not served**, under explicit governance approval |

## The facts about `implementation`, each independently checkable

- No `TypeProvider` trait method, no `verter_lsp` dispatch handler, and Verter advertises no
  `implementation_provider` — `projection-class-contract.md:284`.
- It is served today by an override of `languageService.getImplementationAtPosition` at
  `packages/typescript-plugin/src/index.ts:3095`, which carrier-routes the position and remaps each
  returned `DocumentSpan` back to source.
- It is advertised by TypeScript's own `implementationProvider` capability, never by one of Verter's.
- Answering it is a **type-level query over the TypeScript program** — which types implement this
  interface, which members override this one.
- Its sole server is marked **Deleted** at `deletion-closure.md:101`, which is what made the capability's
  survival a live question and what the migrate ruling settled.

## Candidate by candidate

**`DisabledByExplicitApprovedContract` — EXCLUDED, structurally.** This owner means the capability is
deliberately not served. The ruling states the opposite in terms: migrate, **do not remove**. An owner
whose definition is "not served" cannot be a legal target of a ruling that the capability survives. The
exclusion is the ruling's, not a preference.

**`VerterNative` — EXCLUDED, structurally.** Its test is that the feature needs **no** TypeScript
capability. Go-to-implementation is a type-level query over the TypeScript program, and Verter operates
no independent type engine for that question — deferring type semantics to TypeScript is the premise of
this whole program, not an incidental fact about today's code. A `VerterNative` assignment would
therefore have to answer "which types implement this interface" without TypeScript, which no evidence in
this set claims is possible. **Falsify this by showing a Verter-side path that answers it with no
TypeScript capability**; none exists in the ledger's nine `VerterNative` rows, every one of which records
"needs TS: none" truthfully.

**`TypeScriptLspDirect` — ADMISSIBLE.** TypeScript answers directly against the mapped file and the
result is mapped back. That is precisely what the existing plugin override does: carrier-route the
position, remap the returned spans. The capability is already being served this way, so admissibility is
demonstrated by the running code rather than argued.

**`VerterWithTypeSemanticOracle` — ADMISSIBLE.** The four closest structural analogues — `get_definition`
(#12), `get_type_definition` (#13), `get_references` (#14) and `get_document_highlights` (#19) — are all
location-returning read features, and all four carry the oracle arm for exactly the case a mapped file
cannot express: "component/slot cross-file defs the mapped file's own AST can't express". An
implementation located in a carrier region TypeScript's mapped view does not fully represent is the same
shape, so the criterion that admits the oracle for those four admits it here.

## The result: TWO legal candidates, and a third disposition the family precedent supports

**Legal: `TypeScriptLspDirect` and `VerterWithTypeSemanticOracle`. Excluded: `VerterNative` and
`DisabledByExplicitApprovedContract`.** More than one, so the choice is not this block's.

**One further observation, offered because it is derivation rather than preference.** For all four
closest analogues the ledger does not pick one of the two — it records a **split**:
`TypeScriptLspDirect` for the plain-TypeScript case, `VerterWithTypeSemanticOracle` for the cross-region
case the mapped file cannot express. So "both, as a disjoint split" is a third structurally supported
disposition for `implementation`, not a hedge between the two.

**And the one place the family does NOT split is worth putting in front of whoever chooses.**
`get_rename_locations` (#15) is oracle-**only**, because a pure `TypeScriptLspDirect` answer "would miss
the template-side occurrences" — for a WRITE feature an incomplete answer corrupts, so the split is
unavailable. Go-to-implementation is a READ feature: an incomplete answer degrades the result without
corrupting anything. That distinction is why it sits in the #12/#13/#14/#19 family rather than the #15
family, and it is the tradeoff between the two candidates in one line — **`TypeScriptLspDirect` alone
risks missing carrier-region implementations; the oracle arm exists precisely to catch them.**

## Outcome — decided elsewhere, cited here

**The choice was made by act `3a9ba83d9`.** What it decided is the registry's to state and is not repeated here; a derivation that restates the verdict it fed becomes a second store of that verdict.

**What this file owns, and why nothing above is amended:** the derivation was correct and produced two candidates, and the outcome was neither of them alone. Rewriting it to match would destroy the only record of the distinction that matters — **a derivation bounds what is LEGAL; it does not enumerate what is AVAILABLE** — and that distinction is why the shapes the analogues actually use were put in front of the choice rather than the candidate pair alone.

## What this file does not do

It does not choose, rank or recommend. It does not scope or schedule the migration — target and timing
belong to the receiving owner by the ruling's own words. It establishes only which owners a choice may
legally be made from, and what distinguishes them.
