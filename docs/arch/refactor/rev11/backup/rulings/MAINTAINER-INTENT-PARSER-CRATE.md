---
ruling_id: "PARSER-CRATE-OWNERSHIP-INTENT"
type: "maintainer-directive"
date: "2026-08-18"
date_source: "stated"
binds: ["verter_parser crate ownership (cross-cutting, not a single block)"]
source_file: "MAINTAINER-INTENT-PARSER-CRATE.md"
summary: "verter_parser is Verter's parsing crate, not Vue's — Vue and Svelte SFC are both first-party carriers and both belong there; any future first-party SFC carrier lands there too. Settles the question of purpose only; does not settle module boundaries, internal foldering, program placement, or sequencing against B2/B3 — those are delegated to an open-ended codex architect consult. Notes the current split (svelte_reactivity.rs in verter_parser, Svelte tokenizer/template parsing in verter_compiler) is inconsistent, not merely untidy."
supersedes: []
superseded_by: []
contradicts: []
notes: "States explicitly: if the delegated consult contradicts this intent, that is a genuine conflict between a maintainer design decision and an architecture finding, to be surfaced to the maintainer rather than letting either side silently win. No such consult document is present in this migrated corpus."
---

# Maintainer design intent — verter_parser owns Verter's parsing (2026-08-18)

Maintainer: Carlos Rodrigues <carlos@hypermob.co.uk> (GitHub: pikax).

## Verbatim

> Noticed svelte parser is in verter_compiler, SFC parsing should belong to verter_parser crate
> instead, it seems much better to have it in parser crate, maybe separated by a folder vue/svelte/etc

and, clarifying the intent:

> verter_parser should be verter parsing, verter supports vue and svelte SFC as first party, so it
> makes sense that to hold it, future SFC should also stay there

## The intent, normalized

`verter_parser` is **Verter's parsing crate** — not Vue's. Vue and Svelte SFC are BOTH first-party
supported carriers, so both belong there, and any future first-party SFC carrier lands there too
rather than growing a new home inside a consumer crate.

This settles the QUESTION OF PURPOSE. It does not by itself settle:
- exactly which modules are "parsing" versus IDE projection or runtime codegen (~141.6k lines sit
  under `crates/verter_compiler/src/svelte/`, and plainly not all of it is parsing);
- whether per-framework foldering (`vue/`, `svelte/`, …) is the right internal shape, or whether it
  entrenches the per-framework forks the framework-adapter substrate exists to prevent;
- where the work belongs in the program (a sub-block, a new block, a later train, or after);
- sequencing against B2, which is landing changes to this exact surface right now, and B3 behind it.

Those are with an open-ended codex architect consult (`/tmp/parser-ownership-out.txt`).

## Standing consequence, independent of that ruling

The current split is INCONSISTENT, not merely untidy: `verter_parser` already contains
`svelte_reactivity.rs` while Svelte's tokenizer/template parsing sits under `verter_compiler`. So
"where does Svelte parsing live" currently has two answers. Whatever the consult rules on scope and
timing, the end state is a single answer, and it is `verter_parser`.

**If the consult contradicts this intent**, that is a genuine conflict between a maintainer design
decision and an architecture finding — surface it to the maintainer rather than letting either side
silently win.
