---
ruling_id: "ONE-CSS-PARSER-PARSE-ONCE"
type: "maintainer-ruling"
date: "2026-08-21"
date_source: "stated"
binds: ["J1", "J2", "J3", "J4", "CSS/style pipeline architecture"]
source_file: "MAINTAINER-RULING-ONE-CSS-PARSER-PARSE-ONCE.md"
summary: "Track J's governing invariant: exactly ONE CSS parser, and each item is parsed ONCE. No additional scanner may run over something already parsed — a consumer that needs CSS structure reads the existing parse result instead of re-deriving it. This is the general rule the per-item dispositions were instances of, and it is the standing test for anything discovered later: a second parse of already-parsed input is a violation regardless of which surface performs it."
supersedes: []
superseded_by: []
contradicts: []
notes: "Issued after the per-item re-test settled at seven in / three out. It generalises those decisions into an invariant, so future readers are judged by the rule rather than by analogy to the inventory. Structurally parallel to the codebase's existing 'exactly one type-resolution engine' rule and the build philosophy's parse-each-file-once requirement. The out-of-scope items remain out precisely because they consume a foreign parse rather than adding a second one of ours."
---

# Maintainer Ruling — one CSS parser, parse once

**Status:** RATIFIED by the maintainer, 2026-08-21.

Recorded verbatim:

> we ONLY need 1 CSS parser and we only parse once, no extra scanners are to be
> done if the item has been parsed already

## The invariant

1. **One parser.** `StyleSyntaxIr` is the sole CSS-family syntax authority. No
   second parser, no private grammar, no fallback parser.
2. **Parse once.** An item is parsed a single time. Its parse result is what
   later consumers read.
3. **No re-scanning.** Once something is parsed, nothing may scan it again to
   re-derive structure it already carries — no byte scan, no regex, no
   brace/comma walk, no comment/string mask, no context classifier.

A consumer needing CSS structure reads the existing parse result. If the result
does not carry what it needs, the answer is to extend what the parse records, not
to add a scanner beside it.

## Why this is the rule and the per-item list was the example

Track J's inventory was settled item by item — seven in, three out — by asking
whether each re-derived CSS structure from bytes. That question is this
invariant. The list is an instance of the rule, not the rule itself.

So anything discovered later is judged directly: **a second parse, or a scan over
already-parsed input, is a violation regardless of which surface performs it** —
compiler, analysis query, code action, completion, colour decoration. The
inventory is not exhaustive and was never claimed to be; the rule is what closes
that gap.

The three out-of-scope items stay out under this same rule: they consume a
foreign parse rather than adding a second one of ours. The VS Code CSS service is
someone else's parser serving someone else's feature; the highlighting grammars
are consumed by external tokenizers.

## Relationship to existing architecture rules

This is the CSS-family form of two rules the codebase already holds: exactly one
type-resolution engine, and read/parse each canonical file once per content hash
through one shared path. Divergence between two engines is the bug class both
exist to prevent, and the same applies here.

## Enforcement

Track J charters carry this as an acceptance criterion, and enforcement is
structural rather than a name-keyed scanner over the source tree — consistent
with the rule that landed guards are structural.
