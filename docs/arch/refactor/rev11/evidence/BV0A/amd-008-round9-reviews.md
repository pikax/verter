# AMD-008 round 9 — three separate blind mandates

Three independent Codex xhigh dispatches, `--sandbox read-only`, each blind to the
others, all against candidate `2f484af88b5c0f85f99d7f142084e2b7e102ec99`, tree
`3c73319aad75b9ae181f79b663700bae73a780ba`. Verdicts reproduced verbatim below.

Outcome: conformance `PASS`; architecture `BLOCKING_FINDINGS` (1); governance
`BLOCKING_FINDINGS` (2). Both blocking mandates were verified against the real cited
source lines before being actioned.

## Round 9 — conformance

No blocking findings.

1. Source claims conform. The assembler performs the global, sequential replacements shown in [compile.rs](/Users/carlosrodrigues/Documents/dev/verter/crates/verter_session/src/compile.rs:84); `__sfc__` and `_sfc_main` are respectively 7 and 9 ASCII bytes. For source-backed maps, [source_map.rs](/Users/carlosrodrigues/Documents/dev/verter/crates/verter_compiler/src/code_transform/source_map.rs:243) skips empty overwrites and emits exactly one token for a non-empty overwrite at the generated replacement start, mapped to the original range start. The cited wrapper emission and equal-coordinate lookup behavior also match source.

2. The amended acceptance criterion is coherent and non-circular. It independently requires baseline code-byte equality, valid wire decoding, exact ordered decoded-artifact equality against an input-only cross-language reference, and fail-closed handling. Although the detailed algebra is deferred, BV0A cannot exit until the vector suite is complete, schema-bound, independently reviewed, reproduced by both implementations, and frozen. Hand derivation by neither implementation, structural independence of the reference, inventory-count assertions, and field-specific mutation controls make that prerequisite enforceable rather than self-certifying.

3. The determinism layers are unambiguous and technically sound. [AMD-008](/Users/carlosrodrigues/Documents/dev/verter/docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:249) applies determinism to production's raw serialized map bytes, while exact equality applies to the complete decoded artifact. [carrier_compiler.rs](/Users/carlosrodrigues/Documents/dev/verter/crates/verter_compiler/src/framework_common/carrier_compiler.rs:208) feeds `raw_map.as_bytes()` into `map_hash` alongside the stable source-space token, so two logically equal but differently serialized maps would indeed produce different hashes under identical invocation inputs. Requiring both checks preserves the intended cache/hash contract without conflating it with logical artifact equality.

The worktree remained unmodified.

VERDICT: PASS

## Round 9 — architecture

Reviewed exact commit `2f484af88b5c0f85f99d7f142084e2b7e102ec99`, tree `3c73319aad75b9ae181f79b663700bae73a780ba`. Worktree was clean.

1. **AMD-008's supersession enumeration leaves two conflicting column-delta directives alive.** AMD-008 deletes the custom per-occurrence column-delta model in favor of normative local `CodeTransform` semantics ([AMD-008:328](/Users/carlosrodrigues/Documents/dev/verter/docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:328)). However, §4 supersedes only the recorded ratification's parenthetical "no CodeTransform/chunk-IR mandate" clause and says the remainder—and all other AMD-007 content—stands ([AMD-008:378](/Users/carlosrodrigues/Documents/dev/verter/docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:378), [AMD-008:391](/Users/carlosrodrigues/Documents/dev/verter/docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:391)). AMD-007 §8.1 separately records that the maintainer retained the "per-occurrence column-delta approach" ([AMD-007:501](/Users/carlosrodrigues/Documents/dev/verter/docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:501)) and concludes with an explicit "direction to keep the column-delta approach" ([AMD-007:528](/Users/carlosrodrigues/Documents/dev/verter/docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:528)). Those sentences are outside the six mirrored charter occurrences and outside the specifically superseded parenthetical. Because §4 promises exhaustive sentence-level supersession, they remain conflicting authority. Minimum correction: expressly supersede both §8.1 column-delta directives to the extent replaced by AMD-008.

The remaining checks pass:

- BV0A remains a narrow Vue assembler repair. The first Abort/rescope paragraph is unchanged between BV0A and AMD-007 and remains consistent with the revised composition-only objective.
- `CodeTransform` is normative only for the global `__sfc__` rename and global export removal, with whole-module/cross-block chunk IR expressly excluded.
- The "no harness copy" reading is consistent in context: AMD-007 prohibits a harness-generated BF2 candidate and duplicate shipped assembly path. The independent JavaScript expected-artifact oracle is neither.
- Deferring the detailed algebra/schema to the acceptance-frozen vector suite is architecturally sound. Acceptance still requires a complete schema, exhaustive failure taxonomy, hand-derived vectors, independent review, exact execution counts, field-discriminating mutations, and reproduction by both implementations. The current incomplete seed cannot itself satisfy BV0A acceptance.

VERDICT: BLOCKING_FINDINGS (1)

## Round 9 — governance / adversarial

Reviewed commit/tree match the request, and the worktree is clean.

1. **BLOCKING — BV0A can still define its own acceptance semantics.** AMD-008 makes the future vector suite normative, develops it alongside both implementations, freezes it only at BV0A acceptance, and lets it override conflicting charter prose ([AMD-008:132](/Users/carlosrodrigues/Documents/dev/verter/docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:132), [AMD-008:143](/Users/carlosrodrigues/Documents/dev/verter/docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:143), [AMD-008:160](/Users/carlosrodrigues/Documents/dev/verter/docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:160)). The current artifact admits that its schema, expected artifacts, boundary geometry, and failure taxonomy are incomplete ([vectors:1](/Users/carlosrodrigues/Documents/dev/verter/packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:1)). "Complete," "full," and "independently reviewed" are textual anchors, but no independently frozen inventory or semantic specification determines completeness before implementation. A candidate can therefore encode the same wrong collision/boundary rule in the vector, JavaScript reference, and Rust implementation; reproduce every vector; pass the enumerated mutations; and rely on vector precedence to settle the disagreement. The only remaining gate is future reviewer judgment. This conflicts with the rule that a candidate cannot choose its own pass criteria. Minimum correction: independently review and freeze the DTO schema, validation order/taxonomy, chaining/collision policies, and exhaustive assembler write/boundary manifest before implementation comparison; later coverage-only additions may remain BV0A deliverables, but semantic changes must require an amendment.

2. **BLOCKING — §5.1 materially overstates the review record and suppresses an open issue.** The round-8 row claims `3x BLOCK` ([AMD-008:463](/Users/carlosrodrigues/Documents/dev/verter/docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:463)), but its evidence is one "combined review" by one context with one `BLOCKING_FINDINGS` verdict ([round-8 review:1](/Users/carlosrodrigues/Documents/dev/verter/docs/arch/refactor/rev11/evidence/BV0A/amd-008-round8-review.md:1), [round-8 review:50](/Users/carlosrodrigues/Documents/dev/verter/docs/arch/refactor/rev11/evidence/BV0A/amd-008-round8-review.md:50)). More importantly, §5.1 describes moving the remaining specification work to BV0A acceptance as settled ([AMD-008:477](/Users/carlosrodrigues/Documents/dev/verter/docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:477)), while round-7 architecture explicitly left co-developed specification authority and exhaustive boundary coverage blocking ([round-7 review:228](/Users/carlosrodrigues/Documents/dev/verter/docs/arch/refactor/rev11/evidence/BV0A/amd-008-round7-reviews.md:228), [round-7 review:245](/Users/carlosrodrigues/Documents/dev/verter/docs/arch/refactor/rev11/evidence/BV0A/amd-008-round7-reviews.md:245)). Cutting that prose did not resolve the authority problem. The table currently documents rounds 3–8, not nine completed amendment reviews; omission of this still-pending review is normal, but representing round 8 as three verdicts is not. Minimum correction: describe round 8 as one combined blocking review and acknowledge the unresolved specification-authority/boundary issue until substantively closed.

Cleared:

- **Bundle integrity:** §5 sufficiently prohibits silent substantive changes: it binds both SHA/tree pairs, requires either direct review of the bundle or a recorded restricted diff, requires amendment/charter bytes to remain identical, and requires fresh reports for changed reviewed-package bytes. An exact path/blob allowlist would harden this further but is not presently a blocking ambiguity.
- **Maintainer authority:** genuinely reserved. The amendment remains pending; only the designated maintainer may record ratification, while silence, review, merge, the proposal commit, and preparer action explicitly do not ratify it ([AMD-008:403](/Users/carlosrodrigues/Documents/dev/verter/docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:403), [AMD-008:444](/Users/carlosrodrigues/Documents/dev/verter/docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:444)). This matches governance's exclusive maintainer authority.

VERDICT: BLOCKING_FINDINGS

## Disposition

Both blocking findings verified against the real cited lines (AMD-007:502 and
AMD-007:531 for the column-delta directives; `amd-008-round8-review.md`'s single
combined verdict line; round-7 architecture findings 4 and 5) and fixed:

- §4 item 4 now enumerates all three AMD-007 §8.1 column-delta/chunk-IR sentences
  as superseded to the extent §2 item 5 replaces the model, and states why the two
  narrative sentences lie outside item 3's six mirrored occurrences.
- §2 item 1 now splits the normative specification into layer 1 (semantic
  specification: DTO schema, validation order and rejection taxonomy,
  chaining/collision policy, exhaustive assembler write/boundary manifest —
  independently reviewed and frozen at a digest before either implementation is
  written against it, changeable only by amendment) and layer 2 (the literal vector
  coverage set — a BV0A acceptance deliverable, frozen at acceptance). §2 item 2,
  §2 item 4, the Required-exits blockquote, and §5 carry the split through.
- §5.1's round-8 row is corrected to one combined-mandate `BLOCKING_FINDINGS` (4),
  a round-9 row is added, and the narration no longer presents specification
  AUTHORITY as settled by the round-6/7 vectors-scoping decision.
