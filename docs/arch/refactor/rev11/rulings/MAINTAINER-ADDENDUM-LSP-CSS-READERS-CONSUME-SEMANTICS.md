---
ruling_id: "LSP-CSS-READERS-CONSUME-SEMANTICS"
type: "maintainer-ruling"
date: "2026-08-21"
date_source: "stated"
binds: ["J1"]
source_file: "MAINTAINER-ADDENDUM-LSP-CSS-READERS-CONSUME-SEMANTICS.md"
summary: "Second addendum to the parsing-only bound: the two LSP CSS readers — `color_info.rs`'s comment/string mask and colour scanners, and `css/mod.rs`'s declaration-value-vs-selector context classifier — are IN Track J where Verter already holds the parsed information. They must consume Verter's CSS semantics instead of re-scanning raw bytes. This is not a mandate to reimplement intellisense in Rust; it removes duplicate byte-scanning where the answer already exists. Track J stands at seven items in, three out."
supersedes:
  - ruling: "J-TRAIN-SCOPE-IS-PARSING-ONLY"
    claim: "Not the bound, which stands. This corrects its application to items 2 and 3 of the inventory, which the re-test placed OUT as LSP intellisense. The exclusion protects intellisense that CONSUMES an external parse; it does not protect an intellisense feature that re-scans raw CSS bytes when Verter's own parse already carries the answer."
superseded_by: []
contradicts: []
notes: "Follows the addendum on extract_static_style_vars and remove_unused_css, and applies the same distinguishing test one level further: the question is whether the code re-derives CSS structure from bytes, not which surface consumes it. The condition matters — where Verter does NOT already hold the parsed information, this addendum does not by itself require producing it."
---

# Maintainer Addendum — the LSP CSS readers consume Verter's semantics

**Status:** RATIFIED by the maintainer, 2026-08-21.

Recorded verbatim:

> #2 & #3 if we have already the CSS parsed information we should use our css
> semantics

## What this covers

- `crates/verter_lsp/src/features/color_info.rs:43` — a comment/string mask plus
  hex / `rgb` / `hsl` scanners feeding editor colour decorations.
- `crates/verter_lsp/src/css/mod.rs:72` — infers declaration-value versus
  selector context from the last `{` / `:` / `;` , feeding completion.

Both re-derive CSS structure from raw bytes. Where Verter already holds that
information from its own parse, they consume Verter's CSS semantics instead.

## What this is not

It is not a mandate to reimplement intellisense in Rust. The VS Code CSS service
still stays, and LSP CSS intellisense is still accepted as a feature. What is
removed is duplicate byte-scanning inside our own code when the parsed answer
already exists.

The condition is load-bearing: **where Verter already has the parsed
information**. Where it does not, this addendum does not by itself require
producing it — that would be a separate decision.

## Standing test, now applied three times

In Track J if it re-derives CSS structure from bytes. Out if it consumes another
parser's result to present something. The surface — compiler, analysis query,
code action, completion, colour chip — does not decide it.

## Effect

Items 2 and 3 are IN. Track J stands at **seven in, three out**: out are the
VS Code CSS service, the Monarch/TextMate highlighting grammars, and the
PostCSS/SugarSS grammars.
