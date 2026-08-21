---
ruling_id: "SEMANTIC-CSS-EXTRACTION-CONSUMERS"
type: "maintainer-ruling"
date: "2026-08-21"
date_source: "stated"
binds: ["J1"]
source_file: "MAINTAINER-ADDENDUM-SEMANTIC-CSS-EXTRACTION-CONSUMERS.md"
summary: "Addendum to the parsing-only bound: `extract_static_style_vars` (verter_semantic) and `remove_unused_css` (verter_actions) must BOTH consume Verter's semantic CSS extraction rather than parsing CSS themselves. They are IN Track J, reversing the re-test that had placed them OUT. The editor-tooling exclusion covers consuming CSS for intellisense; it does not cover code that does its own CSS parsing, whatever surface it serves."
supersedes:
  - ruling: "J-TRAIN-SCOPE-IS-PARSING-ONLY"
    claim: "Not the bound itself, which stands. This corrects its application: the re-test placed `extract_static_style_vars` OUT because its only caller feeds a host analysis query, and `remove_unused_css` OUT as an LSP code action. Both readings weighed the CONSUMER; the bound turns on whether the code PARSES. Both parse, so both are IN."
superseded_by: []
contradicts: []
notes: "Issued after the J1 re-test reported 3 IN / 7 OUT. The distinguishing test is what the code does with CSS bytes, not which surface consumes the result: a code action or an analysis query that parses CSS itself is a duplicate parsing authority, while intellisense that consumes an external service's parse is not."
---

# Maintainer Addendum — semantic CSS extraction has these two consumers

**Status:** RATIFIED by the maintainer, 2026-08-21.

Recorded verbatim:

> extract_static_style_vars should use our semantic css extraction,
> removed_unused_css should also be using our semantic css extraction

## What this corrects

The Track J re-test placed two items OUT that are IN:

- `crates/verter_semantic/…/template.rs::extract_static_style_vars` — placed OUT
  because its only production caller feeds a host analysis query rather than a
  compiler consumer.
- `crates/verter_actions/…/remove_unused_css.rs` — placed OUT as an LSP code
  action, on the reading that "editor tooling" is an excluded category.

Both readings weighed the CONSUMER of the result. The bound turns on what the
code does with CSS bytes. Both parse CSS themselves — deriving declarations,
custom-property membership, selector-list grouping, rule extents — so both are
duplicate parsing authorities and both converge on the shared semantic CSS
extraction.

## The distinguishing test

In Track J if it **parses CSS**, whatever surface consumes the result.
Out of Track J if it **consumes someone else's parse** to offer intellisense —
which is why the VS Code CSS service and the LSP intellisense readers stay out.

A code action is not exempt for being a code action, and an analysis query is not
exempt for being an analysis query.

## Effect

`extract_static_style_vars` and `remove_unused_css` are IN Track J: five items
in, five out. The parsing-only bound is otherwise unchanged.
