# Deferred: the publication-contract repair, and what it must settle

An architecture ruling at review round 15 cut a body of work out of this block and assigned
it forward. This file is that assignment. It is written for someone with no memory of this
block, because that is who will pick it up.

**Ruling:** `~/.claude/briefs/rev11/verify/b6-checkpoint-r15-verdict.md`, sections 2 and 4.

## Why it was cut

This block owns preparation and batch lifecycle. Its charter says in terms that prepared
state may not change publication meaning. The work below changes publication meaning: it
adds a published fact and its accessor, two refusal variants, and two new validation
directions. It is in the right technical layer — `verter_compiler::assembly::publish` is
the sole publication implementation and nothing here creates a second one — but being in
the right layer is not the same as being this block's to decide.

The ruling is explicit that reverting is not a judgement on the code: the defects the work
found are real, and one of them was found by an executed plant rather than by argument.
They are simply not this block's to fix.

## Owner and gate

**Owner:** a publication-contract repair owned by `verter_compiler::assembly` — the block
that owns publication semantics. The ruling directs that it be settled together with the
existing text-versus-fact drift already recorded against that owner, because they are the
same contract.

**Gate:** plan close at the latest. Not open-ended.

## What must be settled

### 1. Exact import-kind matching on the forward check

The emitted-import check accepts an import when the names it binds are declared, without
requiring that the SAME kind was declared. So a fragment declaring `Default("x")` from
`"m"` admits an ADDITIONAL undeclared `Namespace("x")` from `"m"`: the forward check passes
because `"x"` is a declared name, and a reverse check passes because the declared default
does have its exact counterpart. The published artifact then carries an import no fragment
declared.

A correct resolution requires every emitted import to be traceable to a declaration of the
same `(specifier, kind)` identity, while preserving the existing rule that a declared
HELPER can satisfy a named import's bound names — those two must not be conflated, and the
naive "require exact identity for everything" fix breaks the helper case.

### 2. Imports that bind no name

`SideEffect` binds nothing by definition and `Named` with an empty member list binds
nothing in fact, so a check written as a loop over bound names never runs for either and
the import publishes unvalidated. The condition to branch on is whether the import binds
anything, not which kind it is.

### 3. Helper-name and specifier ownership

A declared HELPER satisfies an import's bound names regardless of which specifier the
import came from, because `declared_names` unions a fragment's import names with its helper
names and the caller then only asks "is this name declared". Two shapes were executed
against the tree and both PUBLISHED:

- a fragment declaring the helper `_openBlock` and NO imports at all admits an emitted
  `Named(["_openBlock"])` from the specifier `https://evil.example/x.js`;
- a fragment declaring a helper named `""` admits an emitted `Namespace("")` from any
  specifier whatsoever.

The second is the more instructive: an empty bound name is a degenerate value that makes
the name check vacuous. A correct resolution has to decide what a helper declaration
actually licenses — a name in the module scope, or a name from a particular specifier —
and the answer governs all three items in this file. Fixing any one alone leaves the
contract incoherent.

A third probe emitted the same `SideEffect` import TWICE against a single declaration and
it published. That one was ruled NOT a defect: duplicate side-effect imports remain
observable in publication order and in the result digest, so nothing is being hidden.

### 4. Whether the emitted-import fact should be published at all

The work being cut here retained `emitted_imports` on the published artifact and exposed an
accessor, where previously the fact was validated and discarded. That is a product-contract
decision, and it is the publication owner's. It is worth deciding deliberately rather than
by accident: a validated fact that is then dropped cannot be observed by any consumer, and
this block wanted it observable so a result-identity comparison could see it.

## What stays uncovered meanwhile

Plainly: **all three fail-opens above remain live in the tree after this cut.** An artifact
can publish an import that no fragment declared, in three distinct shapes. They were live
at the accepted predecessor and remain live; this block's removal restores that state
rather than creating it. Nothing detects them until the repair lands.

The result-identity digest also stops observing emitted-import facts. Generated import
bytes remain covered through the artifact's `code`, so a change to what is actually emitted
is still visible; what is no longer visible is a change to the reported FACTS about those
imports while the bytes stay identical.

## Evidence not to erase

The tests and executed plants built against this work are real and were RED/GREEN verified.
The tests themselves could not stay in the tree — their subject is the API this cut
removed, so they no longer compile — and this file plus `mutation-replay-recipes.md` is
what carries them forward: the names below, the shapes spelled out in sections 1–3
above, and the recipes' own counterexample constructions. No commit SHA is cited,
because this branch is squashed and a recorded SHA becomes a dangling reference the
moment it lands. Do not read their absence from the tree as a judgement on them:

- `emitted_named_import_binding_no_names_is_refused_when_undeclared` and its positive
  control.
- `extra_side_effect_import_is_refused_when_only_a_named_import_is_declared`.
- `reverse_check_requires_declared_kind_not_just_specifier`.
- `unreported_fragment_import_is_refused`.
- The import-fact discriminators in the identity-digest suite.
- The plant recipes for each, in `mutation-replay-recipes.md`.

The counterexample constructions in those recipes — the two-artifact commuting encoding and
the empty-`Named` shape — were expensive to find and are reusable against the repaired
contract.
