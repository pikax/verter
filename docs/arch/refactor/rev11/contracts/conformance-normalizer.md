# Conformance normalizer contract

**Proposed by:** AMD-005. **Authority:** none until maintainer ratification.

The normalizer exists only to remove cosmetic compiler spelling differences. It is a
versioned, deterministic, parser-backed test component. Its digest is recorded in
every normalized golden record.

## Allowed normalization

- whitespace and line-layout changes outside literals/comments with semantic force;
- harmless redundant parentheses proven equivalent by the parser;
- quote delimiter spelling with identical decoded literal value; and
- private generated identifier spelling under scope-aware alpha-normalization that
  preserves binding, shadowing, references, and authored/public names.

## Forbidden normalization

It cannot erase or reorder import/export sources, helper families, declarations,
side effects, DOM nodes, blocks/effects, events, props/attributes, component calls,
slots, hydration markers, SSR structure, diagnostics, mappings, literal values,
source-authored names, or public names. It cannot fold control flow, canonicalize
different helpers to one label, sort statements, remove declarations, or repair
syntax/link errors.

## Required discrimination

BF2 supplies positive cosmetic pairs, forbidden-difference negatives, and mutation
tests for every forbidden category. Each mutation must be proven applied and must
change the comparison result. The suite includes scope capture/shadowing attacks,
helper-source substitution, prop/attribute swaps, reordered effects, missing
hydration markers, altered SSR escaping, diagnostic-span drift, mapping drift, and
literal changes.

Raw parse, import/export/link, execution, diagnostic, and mapping checks run outside
the normalizer. A normalizer pass cannot override failure of any independent oracle.
