# Path-precision fixture corpus

Shared between Stage 0 (pre-change tests; today's coarse cache
invalidation) and Stage 6d (post-change tests; path-precise
invalidation per `R14` / `R28`).

Each `.ts` fixture exercises one type-system archetype the
fact-based cache must observe with path-precision. Fixtures are
self-contained — relative imports between fixtures are valid, no
`@/...` aliases.

Sibling `<archetype>.expected.json` records the consumer-side
invalidation shape under post-Stage-6d path-precise semantics. The
expected JSON is **not yet enforced**; Stage 0's characterisation
tests assert the OPPOSITE (today's coarse over-invalidation). Stage
6d's `tests/path_precise_invalidation.rs` inverts each assertion
against the same fixture and the same `.expected.json` becomes the
target shape.

## Archetype index

| Fixture | Archetype | R-rule |
|---|---|---|
| `pick_literal_key.ts` | `Pick<Foo, "a">` — literal-key projection | R14, R28 |
| `omit_literal_key.ts` | `Omit<Foo, "a">` — literal-key exclusion | R14, R28 |
| `indexed_access_chain.ts` | `Foo["a"]["b"]` — indexed-access chain | R14 |
| `intersection_selection.ts` | `(Foo & Bar)["x"]` — intersection selection | R14 |
| `keyof_full_surface.ts` | `keyof Foo` — whole-surface projection | R14, R28 |
| `generic_arg_of_generic.ts` | `Container<Wrapper<Inner>>` — recursive normalised type args | R27 |
| `recursive_via_pick.ts` | `type Self = { next: Pick<Self, "next"> }` — cycle handling | R27 |
| `mapped_type_full_surface.ts` | `{ [K in keyof Foo]: T }` — mapped type | R28 |
| `module_augmentation_external.ts` | `declare module 'vue' { ... }` | R29 |
| `module_augmentation_added_augmenter.ts` | initial single augmenter — companion `_secondary.ts` adds a second augmenter during the test | R29, G1 |
| `module_augmentation_relative.ts` | `declare module "./local" { ... }` — `ResolvedRelativeCanonical` | R29 |
| `module_augmentation_wildcard.ts` | `declare module "*.css" { ... }` — `WildcardAmbient` | R29 |
| `module_augmentation_global.ts` | `declare global { interface Window { ... } }` — `GlobalAugmentation` | R29 |
| `declaration_merge.ts` | interface merge + overloaded function + namespace+value merge | R7 |
| `cosmetic_edit_jsdoc.ts` | JSDoc edit — semantic-lane invariant; display-lane sensitive | R13 |
| `cosmetic_edit_comment.ts` | comment edit — `parse_stable_hash` invariant | R13 |

## Fixture conventions

- Plain `.ts`, no Vue SFCs (the resolver/projector pipeline reads
  the typed IR from `.ts` and from the SFC's `<script>` block
  uniformly; `.ts` is the canonical archetype substrate).
- All fixtures are valid TypeScript that the existing oxc parser
  accepts. Where a fixture intentionally exercises a recursive or
  edge-case shape, the comment in the file names the shape and the
  expected current-tree termination sentinel.
- File-level documentation comments name (a) the archetype, (b) the
  Stage 0 baseline characterisation that exercises it, (c) the
  Stage 6d post-change discriminator. This is part of the locked-in
  contract: the comment is part of the file's
  cosmetic-edit-discrimination test (changing the comment must not
  affect `parse_stable_hash`).
- LF line endings enforced via `.gitattributes` (the
  `tests/fixtures/**` glob; see commit `ccc05223`).
