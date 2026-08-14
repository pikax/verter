# Conformance normalizer contract

**Proposed by:** AMD-005. **Authority:** none until maintainer ratification, EXCEPT the single
named-specifier-order allowance below, which is ratified by the Codex xhigh scoping ruling recorded
in `docs/arch/refactor/rev11/evidence/vue-known-defect-correction/reviews/round2-scope-consult-ruling.md`
(Q2, "Named helper import-specifier order") and landed under a formal BF2 reopen per that ruling's
required disposition. AMD-005 §8's four-item cosmetic-normalization list is amended by this same
ruling to a fifth item — see `amendments/AMD-005-framework-compiler-conformance-rescope.md` §8.

The normalizer exists only to remove cosmetic compiler spelling differences. It is a
versioned, deterministic, parser-backed test component. Its digest is recorded in
every normalized golden record.

## Allowed normalization

- whitespace and line-layout changes outside literals/comments with semantic force;
- harmless redundant parentheses proven equivalent by the parser;
- quote delimiter spelling with identical decoded literal value;
- private generated identifier spelling under scope-aware alpha-normalization that
  preserves binding, shadowing, references, and authored/public names; and
- the ORDER of the NAMED specifiers within ONE import declaration, which is
  cosmetic: named ESM specifier order has no semantic effect, so
  `import { a, b } from "x"` and `import { b, a } from "x"` are the same program.
  Nothing else about an import is normalized — specifier membership, each
  specifier's imported name and local alias as a pair, the source module,
  default/namespace form and position, import attributes, the top-level order of
  the declarations themselves, and declaration grouping (whether two separate
  `import` statements from the same source are merged) all remain structural in
  THIS normalizer, and the side-effect import sequence remains ordered.
  This is deliberately NARROWER than `crates/verter_vue_conformance/src/compare.rs`'s
  Rust comparator, which is not the authority this normalizer mirrors: that
  comparator's own `diff_imports`/`merge_imports` treats declaration GROUPING as
  cosmetic (it merges every declaration sharing a source into one set before
  comparing, so two declarations from the same source collapse regardless of
  count or order) and only keeps the side-effect sequence ordered. The two
  comparators agree on ONE point only — named-specifier membership compares as a
  set, not positionally — which is the sole distinction this normalizer adopts
  here; it does not adopt the Rust comparator's additional declaration-grouping
  cosmetic treatment. Keeping declaration order/grouping structural in this
  normalizer is the intentionally stricter reading: it is what this contract's own
  required negative controls enforce (a specifier set regrouped across two
  declarations, or two declarations reordered, must still diverge).

## Forbidden normalization

Apart from the single named-specifier-order exception listed above, it cannot erase
or reorder import/export sources, helper families, declarations,
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
