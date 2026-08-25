# Intra-parser allocation attribution (`parse_style_ir` / `StyleSyntaxIr`)

Owner: `verter_css_syntax` allocation predecessor. Complements
[`allocation-phase-attribution.md`](allocation-phase-attribution.md) (pipeline
phases, a donor record — see its banner) by splitting `parse:initial`
internally. Units are allocator **call count** and cumulative **requested
bytes** — not peak, retained, or RSS.

Requested bytes are the `Layout::size()` the program asked for (`new_size` for
`realloc`). That includes `Arc`/`Vec` header and alignment overhead, because the
layout does; it does NOT include the system allocator's own size-class rounding,
which the counting allocator never observes.

Harness: `crates/verter_compiler/tests/allocator_canaries.rs::intra_parser_attribution`.
Reproduce with
`cargo nextest run -p verter_compiler --test allocator_canaries --no-capture`.
Generators and N=50 match `css_bench.rs` / the legacy canary baseline.

## Isolated split (after the reductions in this block)

Lexer+parser with a no-op sink is **zero heap**. `CssSource::new` after
`Arc::from` is **zero heap**. The admission copy is **one** `Arc::from(code)`
call whose requested size is the source length plus the `Arc` header.

| category | admission calls/bytes | source wrap | parser (noop sink) | IR total | sink_new | parse_emit | list transfer |
|---|---|---|---|---|---|---|---|
| class_rules | 1 / 2,048 | 0 / 0 | 0 / 0 | 458 / 277,232 | 2 / 17,520 | 456 / 259,712 | 0 / 0 |
| descendant_selectors | 1 / 1,896 | 0 / 0 | 0 / 0 | 507 / 290,136 | 2 / 16,248 | 505 / 273,888 | 0 / 0 |
| pseudo_selectors | 1 / 1,632 | 0 / 0 | 0 / 0 | 457 / 251,024 | 2 / 13,936 | 455 / 237,088 | 0 / 0 |
| selector_lists | 1 / 2,480 | 0 / 0 | 0 / 0 | 807 / 487,140 | 2 / 21,252 | 805 / 465,888 | 0 / 0 |
| v_bind_rules | 1 / 1,848 | 0 / 0 | 0 / 0 | 457 / 259,308 | 2 / 15,820 | 455 / 243,488 | 0 / 0 |
| v_bind_dotted | 1 / 2,696 | 0 / 0 | 0 / 0 | 457 / 266,636 | 2 / 23,148 | 455 / 243,488 | 0 / 0 |
| deep_rules | 1 / 1,656 | 0 / 0 | 0 / 0 | 807 / 472,456 | 2 / 14,168 | 755 / 456,288 | 50 / 2,000 |
| slotted_rules | 1 / 1,760 | 0 / 0 | 0 / 0 | 807 / 473,300 | 2 / 15,012 | 755 / 456,288 | 50 / 2,000 |
| mixed_vue | 1 / 1,832 | 0 / 0 | 0 / 0 | 688 / 400,904 | 2 / 15,648 | 653 / 383,936 | 33 / 1,320 |
| global_rules | 1 / 1,712 | 0 / 0 | 0 / 0 | 807 / 472,884 | 2 / 14,596 | 755 / 456,288 | 50 / 2,000 |
| repeated_classes | 1 / 1,272 | 0 / 0 | 0 / 0 | 408 / 253,088 | 2 / 10,816 | 406 / 242,272 | 0 / 0 |

`sink_new` is the one parse-wide `Vec` reservation for IR tokens and top-level
statements. `parse_emit` is `StyleSyntaxIrSink` + `SelectorSink` construction.
**list transfer** brackets both selector-list ownership transfers: a rule's own
list (a move — zero allocations) and every functional pseudo's nested argument
list (exactly one `Box<SelectorList>` — 40 bytes — per occurrence, which is why
only the `:deep` / `:slotted` / `:global` / `mixed_vue` categories are non-zero).
The columns must sum to the total, and the harness asserts they do.

## Per-rule cost is constant in stylesheet size

`per_rule_buffers_are_not_sized_from_the_whole_source`, N=50 vs N=400:

| category | 50 rules | 400 rules | per-rule growth |
|---|---|---|---|
| class_rules | 277,232 B (5,544.6 / rule) | 2,233,948 B (5,584.9 / rule) | 1.007x |
| deep_rules | 472,456 B (9,449.1 / rule) | 3,786,336 B (9,465.8 / rule) | 1.002x |
| selector_lists | 487,140 B (9,742.8 / rule) | 3,913,548 B (9,783.9 / rule) | 1.004x |

Bound: 1.10x. Identical rules mean correct per-rule cost is a constant, so this
is a shape claim (constant vs growing), not a frozen ratio.

## Before / after vs the pipeline `parse:initial` bucket

Before is the donor study's `parse:initial` column. **The two columns are not
directly comparable and must not be summed with owner #2's figures without
adjusting**: `parse:initial` INCLUDES the caller's `Arc::from(code)` admission
copy, `parse_style_ir after` EXCLUDES it (it is measured separately above, at
1 call and ~source-length bytes per category). Add one call and the admission
bytes to the "after" column before composing.

| category | parse:initial before (calls / bytes) | parse_style_ir after (calls / bytes) | count | bytes |
|---|---|---|---|---|
| class_rules | 617 / 284,088 | 458 / 277,232 | 0.74x | 0.98x |
| descendant_selectors | 717 / 327,536 | 507 / 290,136 | 0.71x | 0.89x |
| pseudo_selectors | 617 / 286,472 | 457 / 251,024 | 0.74x | 0.88x |
| selector_lists | 1,168 / 557,896 | 807 / 487,140 | 0.69x | 0.87x |
| v_bind_rules | 617 / 283,888 | 457 / 259,308 | 0.74x | 0.91x |
| v_bind_dotted | 617 / 284,736 | 457 / 266,636 | 0.74x | 0.94x |
| deep_rules | 1,317 / 526,896 | 807 / 472,456 | 0.61x | 0.90x |
| slotted_rules | 1,317 / 527,000 | 807 / 473,300 | 0.61x | 0.90x |
| mixed_vue | 1,079 / 444,384 | 688 / 400,904 | 0.64x | 0.90x |
| global_rules | 1,317 / 526,952 | 807 / 472,884 | 0.61x | 0.90x |
| repeated_classes | 567 / 272,112 | 408 / 253,088 | 0.72x | 0.93x |

Functional-pseudo categories dropped the most in count: they were paying a
nested `SelectorList` clone **and** an outer list clone, 150 calls each.

## ESCALATION — parse alone still exceeds the locked ceiling

**This block does not bring the pipeline inside J1's 1.2x per-category
allocation-count ceiling, and no work inside `verter_css_syntax` will.** With
the admission copy added back, `parse_style_ir` alone — zero planning, zero
codegen, zero reparse — against legacy's WHOLE-pipeline count:

| category | parse-only (incl. admission) | legacy whole pipeline | ratio |
|---|---|---|---|
| descendant_selectors | 508 | 374 | 1.36x |
| pseudo_selectors | 458 | 374 | 1.22x |
| deep_rules | 808 | 525 | 1.54x |
| slotted_rules | 808 | 475 | 1.70x |
| global_rules | 808 | 373 | 2.17x |

Before this block those ratios were 1.42x–3.53x; they are now 1.22x–2.17x. The
reduction is real and the direction is right, but the residual is the owned,
cloneable, span-addressable `StyleSyntaxIr` tree itself — one `Vec` per selector
list, complex selector, compound, value list, statement list, and selector-sink
token buffer. Removing it means changing the IR's ownership model (a bump
arena), which is not this block and would weaken the IR the ruling protects.

Per the ownership ruling this is **reported, not resolved here, and the ceiling
is not touched**: no `performance-gates.toml` cell was added, no bound was
rebased, and the ratio was not adjusted to fit the result. Owner #2
(`block/css-arena-lifecycle`) and the cutover still have their own reductions to
make; whether the remainder is genuinely irreducible, and therefore whether the
historical ratio is the right long-term guard at all, is a maintainer rescope
decision this block cannot make for itself.

## Discrimination record

Every assertion this block adds or changes was planted, run, and observed. Each
plant was proved present, unique, and new in the source before its run, and the
tree proved clean again after. Categories cited are the first to trip.

| assertion | plant | result |
|---|---|---|
| list transfer bucket — outer | `structure.list().clone()` back into the rule | RED — class_rules 150 calls / 13,200 bytes, IR total 458 → 608 |
| list transfer bucket — nested | `Box::new(value.clone())` inside the bracket | RED — deep_rules 200 calls / 15,200 bytes against 8,000 admissible |
| phase columns sum to total | `PHASE_CAP = 2` | RED — loud overflow panic (previously silent truncation) |
| `source_wrap.calls == 0` | `CssSource::new` copies the text | RED — class_rules |
| `ir.total.calls > 50` | counter reset after the parse, before capture | RED — class_rules, got 0 |
| per-rule cost is constant | per-rule `SelectorSink` whole-source pre-size at `len / 32` | RED — class_rules 1.903x against a 1.10 bound |
| 8x source-spread sanity | large generator emits N, not 8N | RED — class_rules |
| declaration block value vs rule body | restore the byte-adjacency approximation | RED — Sass reads `foo: bar { … }` as a Declaration, Css as a QualifiedRule |
| indented unknown-at-rule opacity | statement-parse the indented body | RED — Sass classifies a Declaration inside an opaque body |
| custom-property tagging | `SyntaxKind::Declaration` for every declaration | RED — Sass, 0 CustomPropertyDeclaration vs 1 |
| custom-property colon arm | drop the `--`-prefixed early return | RED — Sass reads `--theme:hover { … }` as TypeSelector + PseudoClass |
| at-rule classification | force `UnknownAtRule` in the layout parser | RED — Sass `@media` became UnknownAtRule |
| braced unknown-at-rule opacity | statement-parse the body like any directive | RED — Sass diagnosed AmbiguousStatement |
| malformed variable name | classify `$tone junk: red` as a clean Variable | RED — strict mode no longer rejects |
| at-rule prelude wrapper | re-nest instead of retagging the replayed node | RED — Sass gained an outer ComponentValueList |
| selector-window diagnostics | suppress the pre-hand-off diagnostic record | RED — `.a[ {}` returned no diagnostics |

**One plant failed to apply and reported a false pass.** Guarding the diagnostic
hoist as `if false { } else if let ... {}` leaves the `else if` arm live, so the
suite went green while the mechanism was untouched. It was re-planted to
actually suppress the record, and then went RED. A green planted run is a failed
plant until proven otherwise.

### Honest controls — assertions that cannot fail

- `admission.calls <= 4` and `admission.bytes >= source.len()` bound
  `Arc::<str>::from(&str)`, a std behaviour. No change in this workspace moves
  them. They exist so the admission column is a measured fact, not an assumption.
- `ir.total.calls > admission.calls * 10` cannot fail while `admission.calls <= 4`
  and `ir.total.calls > 50` pass, which precede it. It states the ruling's own
  question — one admission copy cannot explain hundreds of parse allocations —
  rather than discriminating.
- `ir.total.calls > parser_noop.calls` reduces to `> 0`, because `parser_noop`
  measures zero in every category.
- The per-dialect loop in `diagnostics_raised_inside_a_selector_list_still_reach_the_style_ir`
  is layout-path COVERAGE, not an independent discriminator: every plant that
  breaks the hand-off breaks the Css assertion above it first.

### Changes measured but deliberately not guarded

Two reductions are real and behaviour-preserving, and no assertion here
discriminates them. Naming them is the point; inventing a threshold to cover
them would be the self-authored performance gate the ownership ruling forbids.

- **Selector events no longer also build discarded IR frames.** Restoring the
  fall-through leaves all `verter_css_syntax` tests green — which is the evidence
  it is behaviour-preserving — and costs 1 call plus 3-5% of parse bytes
  (`descendant_selectors` 507 / 290,136 → 508 / 305,160).
- **The parse-wide `sink_new` reservation is a TRADE, not a win in both units.**
  Removing it costs about 6 allocation calls per parse and *saves* requested
  bytes in most categories (`class_rules` 458 / 277,232 → 464 / 268,016;
  `repeated_classes` 408 / 253,088 → 413 / 244,528; `v_bind_dotted` is the one
  category where it costs both, 457 / 266,636 → 464 / 268,016). It is kept
  because the locked gate is per-category allocation **call count**, where 6
  calls is the larger movement; the `len / 3` estimate over-provisions and the
  spare capacity is where the extra bytes go. It is not an improvement in both
  units and must not be reported as one.

## Known divergences left standing

- **Diagnostic vocabulary.** For a malformed variable name (`$tone junk: red`)
  both parsers reject, but the direct parser reports `ExpectedRuleBlock` with a
  `Recovery` node while the layout parser reports its own `AmbiguousStatement`,
  a concept the direct parser does not have. Unifying the two vocabularies is
  convergence work for the cutover, not a parse-allocation change. The test pins
  the difference rather than claiming parity.
- **Grammar stated twice.** The declaration VALUE-SHAPE decision is now shared
  (`parser::declaration_value_shape_admits`, consumed by both the direct parser
  and the layout parser). The unknown-at-rule body-opacity decision is still
  expressed twice — `parse_component_values` in the direct parser, and a
  matching-brace/indent splice in the layout parser — because the two differ in
  how they find the body's extent, not in what they do with it.

## TDD sequencing note

The production reductions and their guards were committed in that order rather
than guard-first, and the discrimination above is retrospective mutation
testing. That proves each guard would have caught its defect; it does not
evidence a RED-before-GREEN commit sequence. The correctness fixes in the review
round — the declaration value shape and the indented at-rule body — were driven
the other way: the failing assertion was written and observed failing against
the pre-fix tree first.
