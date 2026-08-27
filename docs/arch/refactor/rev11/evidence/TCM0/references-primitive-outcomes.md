# The missing project-wide references primitive — is a choice even live, and if so which outcomes are legal

`charters/TCM3.md` item 8 reads: *"If the official API lacks a required capability, the legal outcomes
are exhaustive: reassign to direct-LSP ownership, implement natively in Verter, require and certify an
upstream API addition/fix, or keep TCM4 blocked."* An acceptance lane found that clause and read it as
leaving a later block free to choose among semantic outcomes for the project-wide references primitive,
which `package-lock-and-semantic-api.md` §6.2(b) records as absent.

**This file derives. It does not choose.** Choosing among a plural set is not this block's act.

## First: does item 8's ANTECEDENT actually fire here?

The clause is conditional. It triggers only where the API **lacks a required capability**. §6.2(b)'s own
words settle what is absent and what is not:

> `Checker.getReferencesToSymbolInFile(file, symbol)` matches only a symbol whose identity is local to
> that file … **A cross-file references or rename feature must therefore be assembled caller-side:
> enumerate candidate files, resolve each file's own local alias symbol, and union.**

**What is missing is a single-call PRIMITIVE. The CAPABILITY is reachable**, by a composition the probe
demonstrated rather than hypothesised: the import-site alias symbol in the consuming file finds its two
references, so per-file resolution plus a union yields the project-wide answer.

**That distinction decides whether a choice exists at all.** Item 8 governs a capability the API cannot
deliver. It does not govern a capability the API delivers awkwardly, and this programme already treats
the two differently: cancellation is a genuine absence with **no** composition, and `charters/TCM3.md`
item 5 disposes of it explicitly rather than routing it to item 8. References is recorded, in the same
document, as *"a required design constraint for TCM3's `References`/`Rename` capability rows"* — the
vocabulary of a constraint on HOW a capability is served, which the ratified TBD test classes as an
ordinary open question, not a choice among semantic outcomes.

**On this reading item 8's four-way choice is not live for references, and the legal-outcome set is
empty because the trigger does not fire.** The falsifier is exact: **show that the caller-side
composition §6.2(b) describes cannot produce the project-wide answer** — for instance a reference
reachable by no candidate-file enumeration, or an alias chain the per-file resolution cannot follow.
If that is shown, the antecedent fires and the set below applies.

## If the antecedent IS read as firing, the legal set is TWO

Recorded so the choice is not blocked on re-deriving it, and structured the same way as the
`implementation` derivation: criteria taken from how the existing rows are actually classified,
exclusions structural, falsifiers stated.

**`reassign to direct-LSP ownership` — EXCLUDED, structurally.** That outcome means TypeScript answers
the capability directly against the mapped file. For project-wide references it cannot: the only
primitive is file-local, and §6.2(b) proves the cross-file result is **a silent empty answer, not an
error**. Choosing it would deliver a silently incomplete references result, which item 8's own final
sentence declares illegal — *"No fifth outcome (silently degrading a feature …) is legal"*. An outcome
that can only be implemented by doing something the same clause forbids is not a legal outcome of it.
**Falsifier: exhibit a project-wide references primitive in the certified API.** §6.2(b)'s probe shows
none.

**`implement natively in Verter` — EXCLUDED, structurally.** `VerterNative`'s test across the nine rows
that carry it is that the feature needs **no TypeScript capability**. References over TypeScript symbols
requires TypeScript symbol identity — resolving what a name binds to across files is the type-binder
question this programme defers to TypeScript by premise, not by convenience. A native implementation
would be a second symbol-resolution engine for the same question.
**Falsifier: exhibit a Verter-side path answering project-wide references over TypeScript symbols with
no TypeScript capability.** None exists in the nine `VerterNative` rows, every one of which records
"needs TS: none" truthfully.

**`require and certify an upstream API addition/fix` — ADMISSIBLE.** It removes the absence rather than
routing around it, and nothing in the evidence forecloses it. Its cost is an external dependency and a
re-certification gate.

**`keep TCM4 blocked` — ADMISSIBLE.** The fail-closed outcome, named legal by the clause itself. It
serves nothing rather than serving something incomplete.

## The shapes the analogues actually use, put in front of the choice

A derivation bounds what is LEGAL; it does not enumerate what is AVAILABLE. For the `implementation`
capability that distinction mattered — the derivation produced two candidates and the answer was
neither, but the disjoint SPLIT its four analogues already used.

Here the relevant precedent is `get_references` itself, ledger row #14, which is already owned as the
row-#12 split: `TypeScriptLspDirect` for the plain-TypeScript case, `VerterWithTypeSemanticOracle` for
the cross-file case the mapped file's own AST cannot express. **The caller-side composition §6.2(b)
describes is exactly what an oracle arm does** — Verter drives the enumeration and union, consulting the
`Checker` per file for the type facts it does not own. So a fifth shape is available and is the one the
existing assignment already implies: **the capability is served by the arm that already owns it, under
the recorded design constraint, with no item-8 choice required.**

That is the shape, offered because it is derivation. Which shape is taken is not this block's act.

## What this file does not do

It does not choose, rank or recommend. It does not amend `charters/TCM3.md`, whose clause this
concerns and which is another block's ratified charter. It establishes whether a choice is live, and
what a choice would legally range over if it is.
