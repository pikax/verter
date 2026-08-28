# Three false statements about this block, in charters this block does not own

An acceptance lane found three statements about this block inside the ratified charters of TCM2, TCM3
and TCM4. Each is disproved by evidence in this candidate. **This block cannot correct them** — they are
another block's ratified text, and correcting ratified text requires an act, not an edit. So this file
is the SPECIFICATION for that act: the statement, why it is false, and the evidence that disproves it.

**It does not gate this block's acceptance.** A block is not held hostage to errors in documents it does
not own. It is recorded here rather than left in a report so the defect cannot evaporate when the report
is read.

**Each correction below is a narrowing, not a reversal.** In every case the downstream obligation itself
survives — what is false is the premise that this block left the work undone.

## 1. `charters/TCM2.md`, numbered exit criterion 3

**Says:** *"Close the exact wire method-name spelling gap TCM0 left open (`package-lock-and-semantic-api.md`
§5: structural Go type-name evidence for the four-step lifecycle exists; a byte-exact wire trace does
not)."*

**False in its premise.** A byte-exact wire trace exists in this candidate.
`probes/probe7-mapper-wire-capture.mjs` runs the pinned native compiler against a real `contentMappers`
configuration with a stub mapper and records every JSON-RPC frame TypeScript sends, asserting the
four-step lifecycle **by name and order**. The captured spelling is
`initialize` / `openProject` / `transform` / `closeProject` — lowercase-initial camelCase, not the
capitalised Go type names the structural evidence had suggested
(`package-lock-and-semantic-api.md` §3a).

**The narrowing:** the method-name spelling was not the remaining question. The `transform` RESPONSE
body was the remaining question, and it too is now derived — `probes/probe9-transform-response-contract.mjs`,
written up at §3b — with a short, named residue: the offset unit under `utf-16`, individual `features`-bit
semantics, `diagnostics.category` semantics, and the TS100027/TS100028 trigger conditions. Probe 9 also
derives the `diagnosticDirectives` entry layouts.

## 2. `charters/TCM3.md`, owned-scope item 4

**Says:** *"The session-attach topology certification TCM0 explicitly did not run. TCM0 certified the
direct-native-client topology candidate live but did NOT probe `API.fromLSPConnection`
(`custom/initializeAPISession`) for the session-initialization-hang defect class."*

**False.** `probes/probe8-lsp-session-attach.mjs` spawns `tsc --lsp`, issues
`custom/initializeAPISession` to obtain the API pipe, attaches a second client over it, and answers a
`Checker` query through it. **No hang.**

**The narrowing:** TCM3's obligation to satisfy itself before selecting that topology is untouched, and
the ratified `MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED` gates the probe to TCM3 — one probe
run by another block is evidence, never a discharge of a ratified assignment. What is false is only the
claim that the probe was never run. TCM3 also inherits a constraint from it that nothing had recorded:
**attach is ASYNC-CLIENT-ONLY** — the sync client refuses socket connections — plus a bind race
requiring bounded retry.

## 3. `charters/TCM4.md`, owned-scope item 9

**Says:** *"Items 17-18 … are an OPEN TCM0 gap, not a settled part of this manifest — see
`evidence/TCM0/OPEN-GAPS.md`'s `G-DELETION-CLOSURE-ITEMS-17-18` row."*

**False after the binding act in `docs/arch/architecture-lock/ledger/program-state.toml`'s TCM0
`AMD-023` notes.** Items 17-18 were relocated: the RECORDING half is bound on TCM1
criterion 12, TCM2 criterion 14 and TCM3 criterion 10; the VERIFYING half was already bound on TCM4's own
exit criterion 5. This candidate's register carries the row as `NOT-OWNED` with those criteria named, and
the binding is derived rather than asserted — `receiving-coverage.md`, regenerable by
`probes/receiving-coverage-derivation.mjs --check`.

**The narrowing:** TCM4 still verifies rather than discovers, exactly as its criterion 5 already says.
What is false is the cross-reference describing the items as an unsettled gap belonging to this block.

## What the act needs to do

Correct the three statements above and rebind the three charter digests, as ONE act across the three
documents — they are one finding with three instances, and splitting it would leave the tree
inconsistent between acts. The corrections are narrowings: no downstream obligation is removed by any of
them.
