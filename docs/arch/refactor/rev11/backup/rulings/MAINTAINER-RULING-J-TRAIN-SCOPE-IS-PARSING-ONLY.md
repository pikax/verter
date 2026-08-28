---
ruling_id: "J-TRAIN-SCOPE-IS-PARSING-ONLY"
type: "maintainer-ruling"
date: "2026-08-21"
date_source: "stated"
binds: ["J1", "J2", "J3", "J4", "CSS/style pipeline architecture"]
source_file: "MAINTAINER-RULING-J-TRAIN-SCOPE-IS-PARSING-ONLY.md"
summary: "The maintainer bounds Track J: it is ONLY parsing and the removal of Lightning CSS. The VS Code extension's CSS service stays where it is, and LSP CSS intellisense is accepted as-is — neither is a Track J deliverable. This overrides the consult chain that had ruled `packages/vue-vscode/src/css/cssService.ts` (third-party `vscode-css-languageservice`) ADOPT-NOW for J1, and requires every other item on that ADOPT-NOW inventory to be re-tested against the parsing-only bound rather than carried forward."
supersedes:
  - document: "css-family-authority-inventory-gap.md (consult 2)"
    claim: "The ruling that all nine additional CSS-family readers are ADOPT-NOW for J1 is superseded to the extent any of them is not parsing or Lightning CSS removal. Item 7 (`packages/vue-vscode/src/css/cssService.ts`, `vscode-css-languageservice`) is expressly OUT of Track J: the CSS service stays in VS Code and LSP CSS intellisense is accepted. Items whose role is presentation or editor intellisense rather than parsing fall outside the bound and must be re-tested against it, not inherited."
superseded_by: []
contradicts: []
notes: "Answers the escalation raised after J1's fourth ratification round, which surfaced an unaccounted CSS parser and then nine further CSS-family readers — a roughly tenfold scope expansion the block orchestrator declined to self-authorize. The maintainer's bound resolves it by category rather than item by item. Lightning CSS removal and `StyleSyntaxIr` as sole CSS-family syntax authority are unchanged; what is excluded is editor tooling that consumes CSS for intellisense rather than parsing it as a compiler authority."
---

# Maintainer Ruling — Track J is parsing and Lightning CSS removal only

**Status:** RATIFIED by the maintainer, 2026-08-21.

Recorded verbatim:

> the css service in vscode is accepted to stay there, LSP CSS for intelisence is
> accepted, J is only for parsing and removal of lightning CSS

## What this settles

`packages/vue-vscode/src/css/cssService.ts`, which uses the third-party
`vscode-css-languageservice`, **stays**. It is not a Track J deliverable, and
Verter does not reimplement the VS Code CSS editing experience in Rust. LSP CSS
intellisense is likewise accepted as it stands.

A consult chain had classified that file as a second editor syntax authority and
ruled it ADOPT-NOW for J1, on the reasoning that third-party ownership does not
change the classification. That reasoning is overridden here. The distinction the
maintainer draws is between **parsing CSS as a compiler authority** — which is
Track J's subject — and **consuming CSS to offer editor intellisense**, which is
not.

## Scope bound

Track J is:
- CSS-family **parsing**, with `StyleSyntaxIr` as the sole syntax authority;
- **removal of Lightning CSS**.

It is not editor tooling, presentation, or intellisense.

## Consequence for the ADOPT-NOW inventory

`evidence/J1/css-family-authority-inventory-gap.md` lists ten items ruled
ADOPT-NOW. That list is no longer inherited wholesale. Item 7 is out by name.
Every remaining item is re-tested against the bound above: an item is in Track J
if it parses CSS as a compiler authority or is part of removing Lightning CSS,
and out if its role is presentation or editor intellisense.

The re-test is a scoping exercise for the charter, not a re-litigation of this
ruling.
