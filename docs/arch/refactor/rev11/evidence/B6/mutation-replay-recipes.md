# Prepared-carrier / direct-batch closure — executed mutation recipes

> **Read this first: parts of this file describe a tree that no longer exists.**
>
> An architecture ruling cut the publication-boundary work out of this block. Everything in
> this file about EMITTED IMPORTS — the four import-field digest rows, the member-list arity
> row, the `Named`-versus-`SideEffect` tag row, the `emitted_imports().len()` row, the
> fail-open section, and the byte arithmetic that depends on an import record's encoding —
> was executed against the pre-cut tree and is accurate for it. Those plants will NOT
> reproduce here: the digest no longer hashes emitted-import facts, and the tests they
> reddened were removed with their subject.
>
> They are kept deliberately rather than deleted. The ruling says not to erase the evidence,
> and the counterexample constructions were expensive to find and are reusable against the
> repaired contract. They belong with the work, which is assigned in
> [`deferred-to-publication-owner.md`](deferred-to-publication-owner.md).
>
> Everything else in this file — the retained-weight plants, the staleness and namespace
> plants, the producible-kind plants, the single-owner plants, and the digest rows for
> artifact kind / code / dialect / both source-map slots / every style field / every
> diagnostic field — describes the current tree and reproduces on it.
>
> One number is known stale even for the pre-cut tree: the style/diagnostic collision was
> recorded as 735 bytes and later MEASURED at 786. Where a figure here was derived rather
> than instrumented, treat it as derived.

Every discriminating claim this block makes was proved by planting the defect the
assertion exists to catch, running the test, watching it go RED, restoring, and watching
it go GREEN again. This file is the replay ledger: enough detail to re-execute each
recipe rather than trust the report.

**Discipline applied to every plant below**, without exception:

- The target text was confirmed to occur EXACTLY ONCE before the edit; a script applied
  it and aborted otherwise. `sed`, `perl` and `grep` all exit 0 on a non-match, so an
  exit code is never proof a mutation landed.
- The marker was confirmed PRESENT and UNIQUE in the file after the edit, and confirmed
  NEW to `crates/` (`git grep -c <marker> HEAD -- crates` finding nothing), so a
  verification search could not be satisfied by a pre-existing occurrence.
- Tests ran crate-scoped under `~/.claude/bin/rust-lock.sh`, never the full gate and
  never trybuild.
- The file was restored from a byte copy taken before the edit, and the restore was
  confirmed by an empty `git status --porcelain` before the next plant.
- A GREEN planted run was treated as a plant failure until proven otherwise, and where
  it turned out to be genuine it is recorded below as a FINDING, not as a caught plant.

**Commit SHAs are deliberately not cited.** This branch is squashed before landing, so a
recorded SHA becomes a dangling reference the moment it lands — an identity rot this
program has hit before. Each recipe names the file, the exact text, and the test instead,
which is what makes it re-runnable.

## Recipes that found real defects

These plants stayed GREEN. Each one is a defect the suite could not see at the time, and
each is followed by the change that closed it and the re-plant that now reddens.

| Plant | Where | What it proved |
|---|---|---|
| `#[derive(Clone)]` on `VuePreparedCarrier` / `SveltePreparedCarrier` / `PreparedCarrier` | `standalone.rs` | **Compiled, 4/4 named tests stayed GREEN.** `ParsedSfc`/`ParsedSvelte` are themselves `Clone`, so the single-owner claim was a comment and nothing more. Closed by the private zero-sized `SingleOwner` field on both carriers plus three unconditional library-code `assert_not_impl_any!(…: Clone, Copy)` asserts. Both gates now fire independently: `E0277` at the field and `E0283` at the assert. |
| `prepare()` returns `ParsedSvelte::default()` instead of parsing | `standalone.rs` | **GREEN — a stub.** `explicit_prepare_of_unsupported_svelte_product_still_parses` was satisfied by `retained_weight() > 0`, which `ParsedSvelte::default()` plus the 32-byte digest padding already meets. The test now witnesses the parse it is named for: non-empty template, node and style counts matching an independently computed `parse_svelte` oracle. |
| Recursive node COUNT in place of retained BYTES | `retained_weight.rs` | **GREEN — undetected.** The nested-children test's fixtures differed in node count as well as in bytes, so any oracle growing with node count satisfied it. Replaced with two trees of identical shape, depth, node count and attribute count differing only in the length of one owned attribute-name string on the child, bound to the exact byte delta. |
| `descriptor_bytes` starts from `0usize` instead of the retained tag `String` | `retained_weight.rs` | **GREEN — undetected.** The enclosing `vec_cap_bytes` allocation carried the inequality on its own, so the assertion never bound the descriptor string its name is about. Now two tagged fixtures differ only in tag length and the whole weight delta must equal the tag `String`'s own capacity delta. |
| Reverse the artifact order inside `publish` (all routes at once) | `publish.rs` | **GREEN — 47/47 passed.** The identity digest sorted artifacts by product kind before hashing, so a reorder was normalised away, and comparing the four routes against each other cannot see a reorder they all share. The digest now hashes exposed publication order, and `every_route_exposes_artifacts_in_the_same_order` pins the observable sequence rather than only asserting the routes agree. |
| Add `PublicApi` to `VUE_PRODUCIBLE_KINDS` | `standalone.rs` | **GREEN — undetected.** Every existing refusal test named a kind already absent from the list, so the declaration was bound only in the removal direction. Adding a kind the route cannot produce passed the preflight and failed later with a different error that nothing asserted. Closed by two tests stating the expected set independently of the production constant and asserting both directions. |
| Restore `size_of::<Self>()` at the top of `TemplateAst::retained_bytes` | `ast/types.rs` | Invisible to every landed test: retained-weight tests compare a DELTA between two fixtures, and a constant cancels on both sides of a subtraction. The Vue carrier over-reported by exactly `size_of::<TemplateAst>()`, which `ParsedSfc::retained_bytes` had already counted inline. Nested contributors now report heap only, matching the Svelte half, bound by a self-closing-template fixture whose whole arena total is enumerable. |
| Assign `cold_build_count`/`reuse_count` from the harness's own loop counts | `compiler_route_overhead.rs` | The original harness could not fail when `compile_prepared` regressed to re-parsing on every call, because its counters were true by construction of the loop. It now reads the real `compiler.carrier_parse.calls` attribution counter around each leg and asserts exact equality in both directions. |
| Submit one batch item per distinct source | `compiler_route_overhead.rs` | With `items.len() == n`, `cold_build_count == n` held whether or not anything deduped, so a no-dedup regression was invisible. The batch leg now submits the corpus twice. |

## Recipes that confirm a closed defect

Each of these reddens the test named beside it, and greens again on restore.

| Plant | Where | Target test | Observed |
|---|---|---|---|
| Return `UnsupportedSvelteRuntimeSurface::DynamicAttribute` instead of `ServerGenerate` | Svelte SSR refusal | `compile_batch_unsupported_svelte_runtime_server_performs_zero_carrier_prepares`, `svelte_runtime_server_request_fails_closed_not_reinterpreted`, `svelte_dual_runtime_client_and_server_request_fails_closed_with_no_partial_output` | RED 3/3. Before the fix these matched `DirectCompileError::Svelte(_)` and stayed GREEN for the wrong error. |
| The capability oracle always reports "producible" | `refuse_unproducible_runtime_surface` | the two direct-route SSR refusal tests and the batch zero-prepares test | RED 3/3 from ONE edit — which is the point: the preflight, the compile loop and the refusal all now follow from that single answer. |
| Skip the cached `v-if` condition's modifier `SmallVec` | `TemplateAst::retained_bytes` | `retained_weight_counts_vue_cached_directive_modifier_spills` | RED on the exact byte-delta equality (`left: 0, right: 32`, `spilled_weight=3104 plain_weight=3104`), not merely on "something moved". The Svelte custom-element test stayed GREEN — correct, it binds a different field. |
| Skip the recursion into `element.children` | `retained_weight.rs` | `retained_weight_counts_nested_svelte_children_not_just_top_level_nodes` | RED (`nested=1611 shallow=1611`, `left: 0, right: 22`). |
| Drop the custom-element descriptor tag bytes | `retained_weight.rs` | `retained_weight_counts_svelte_custom_element_descriptor_strings` | RED (`longer=2134 tagged=2134`, `left: 0, right: 16`). |
| `retained_weight()` always returns `0` | `standalone.rs` | every weight test | RED, including `prepared_carrier_exposes_positive_retained_weight`. |
| `prepare_owned` uses source `len()` not `capacity()` | `standalone.rs` | `prepare_owned_retains_source_and_reports_greater_weight_than_borrowed` | RED (`owned=6353 borrowed=6048`; the delta is the source `len`, not the expected 8192 capacity). |
| Batch skips the `refuse_inputs_mismatch` preflight | `standalone.rs` | `compile_batch_framework_mismatch_is_refused_before_prepare` | RED — got `UnsupportedProduct(IdeCompanion)` instead of `FrameworkMismatch`. |
| Treat Svelte `RuntimeServer` as producible so batch prepares first | `standalone.rs` | `compile_batch_unsupported_svelte_runtime_server_performs_zero_carrier_prepares` | RED — `cold_build_count` left `1`, right `0`. |
| Side-effect import check matches specifier only | `publish.rs` | `extra_side_effect_import_is_refused_when_only_a_named_import_is_declared` | RED — `unwrap_err()` on `Ok`, with both `Named` and `SideEffect` published. |
| Reverse-path `.any()` hole (specifier-only match) | `publish.rs` | `reverse_check_requires_declared_kind_not_just_specifier` | RED — `unwrap_err()` on `Ok`. |
| Reverse artifacts in ONE route only (the prepared path) | `standalone.rs` | the corpus identity test and `every_route_exposes_artifacts_in_the_same_order` | RED 2/2. |

## The identity digest, field by field

The digest is the ONLY oracle comparing the four compile routes, so a field it silently
stops hashing makes the identity proof vacuous for that field while every test stays
green. Each row below deletes exactly one field's hash from
`direct_compile_output_digest` (or its `hash_declared_import` / `hash_output_descriptor`
helpers) and records which tests went RED.

Command for every row:

```
~/.claude/bin/rust-lock.sh b6-plants -- cargo nextest run -p verter_compiler --lib \
  -E 'test(identity_digest_)' --no-fail-fast
```

| Field whose hash was deleted | Marker | Tests that went RED |
|---|---|---|
| artifact `kind` | `PLANTED_OMIT_ARTIFACT_KIND` | 1: `identity_digest_changes_when_the_product_kind_changes` |
| artifact `code` | `PLANTED_OMIT_ARTIFACT_CODE` | 1: `identity_digest_changes_when_one_byte_of_artifact_code_changes` |
| artifact `dialect` | `PLANTED_OMIT_ARTIFACT_DIALECT` | 1: `identity_digest_changes_when_the_dialect_changes` |
| `source_projection_map` | `PLANTED_OMIT_SOURCE_PROJECTION_MAP` | 1: `identity_digest_changes_when_the_source_projection_map_changes` |
| `runtime_source_map` | `PLANTED_OMIT_RUNTIME_SOURCE_MAP` | 2: `identity_digest_changes_when_the_runtime_source_map_changes`, `identity_digest_distinguishes_a_present_source_map_from_an_absent_one` |
| import `specifier` | `PLANTED_OMIT_IMPORT_SPECIFIER` | 1: `identity_digest_changes_when_an_import_specifier_changes` |
| import kind discriminant | `PLANTED_OMIT_IMPORT_KIND_TAG` | 1: `identity_digest_changes_when_an_import_kind_tag_changes` |
| import member names | `PLANTED_OMIT_IMPORT_MEMBER_NAMES` | 1: `identity_digest_changes_when_an_import_member_name_changes` |
| style `code` | `PLANTED_OMIT_STYLE_CODE` | 1: `identity_digest_changes_when_style_code_changes` |
| style `source_map` | `PLANTED_OMIT_STYLE_SOURCE_MAP` | 1: `identity_digest_changes_when_a_style_source_map_changes` |
| style `lang` | `PLANTED_OMIT_STYLE_LANG` | 1: `identity_digest_changes_when_style_lang_changes` |
| style `scope_hash` | `PLANTED_OMIT_STYLE_SCOPE_HASH` | 1: `identity_digest_changes_when_a_style_scope_hash_changes` |
| style `has_global` | `PLANTED_OMIT_STYLE_HAS_GLOBAL` | 1: `identity_digest_changes_when_style_has_global_changes` |
| style `output_descriptor` | `PLANTED_OMIT_STYLE_OUTPUT_DESCRIPTOR` | 1: `identity_digest_changes_when_the_style_output_descriptor_changes` |
| diagnostic `severity` | `PLANTED_OMIT_DIAGNOSTIC_SEVERITY` | 1: `identity_digest_changes_when_a_diagnostic_severity_changes` |
| diagnostic `code` | `PLANTED_OMIT_DIAGNOSTIC_CODE` | 1: `identity_digest_changes_when_a_diagnostic_code_changes` |
| diagnostic `message` | `PLANTED_OMIT_DIAGNOSTIC_MESSAGE` | 1: `identity_digest_changes_when_a_diagnostic_message_changes` |
| diagnostic `span` | `PLANTED_OMIT_DIAGNOSTIC_SPAN` | 2: `identity_digest_changes_when_a_diagnostic_span_changes`, `identity_digest_distinguishes_a_present_diagnostic_span_from_an_absent_one` |
| import member-list arity | `PLANTED_OMIT_IMPORT_ARITY` | 1: `identity_digest_changes_when_an_import_member_arity_changes` |
| diagnostic span presence tag | `PLANTED_OMIT_SPAN_PRESENCE_TAG` | 1: `identity_digest_changes_when_diagnostic_span_presence_moves_between_diagnostics` |

**Read the third column, not just the second.** A row that reddens exactly one test is a
discriminator. A row that reddens several means those tests are not isolating what their
names claim, and a row that reddens none is a stub.

**Two of these rows exist because an earlier revision of this file was wrong.** It
claimed the import member-list arity prefix and the diagnostic span presence tag were
REDUNDANT — that the surrounding encoding was already prefix-free, so no input pair
could isolate them, so no test was owed. Review produced counterexamples and the claim
did not survive them. The encoding is prefix-free WITHIN one record and NOT across
concatenated ones:

- With the arity prefix deleted, a member's length-prefixed name is re-read as the
  following import's specifier and one-byte kind tag, and two different four-import
  sequences encode to the same bytes. Every such pair needs NUL bytes in a specifier or
  a bound name; that is expressible because a `DeclaredImport` is a FACT the composer
  reports and `publish` compares fact sets rather than re-reading `code`.
- With the span presence tag deleted, two diagnostics whose presence is SWAPPED encode
  identically: one's eight span bytes and the other's eight-byte zero severity exchange
  positions. (A span is EIGHT bytes — `Span` is two `u32`s — not the sixteen that same
  revision asserted.)

Both pairs were verified by computing the encodings before writing the tests, and both
tests now redden under the plant that motivated them. The lesson recorded here is not
the encoding detail: it is that "no test can isolate this" is a claim about all possible
inputs, and it needs the same evidence as any other claim.

**The twenty rows above were executed in two cycles, and they are recorded separately
because their test counts differ.**

The first EIGHTEEN rows ran when the `identity_digest_` selector matched 25 tests: the
suite was 25/25 GREEN before each plant and 25/25 GREEN after every restore, and every
row reddened only tests bound to the field it deleted. The two rows that reddened a PAIR
(`runtime_source_map`, diagnostic `span`) are the two fields carrying both a value test
and a presence test; both members of each pair are about that same field, so the
isolation holds.

The LAST TWO rows — `PLANTED_OMIT_IMPORT_ARITY` and `PLANTED_OMIT_SPAN_PRESENCE_TAG` —
were executed later, against the tree that added the two discriminators they exist to
prove. By then the same selector matched 27 tests. Each ran as

```
~/.claude/bin/rust-lock.sh b6-plants4 -- cargo nextest run -p verter_compiler --lib \
  -E 'test(identity_digest_)' --no-fail-fast --color never
```

and each produced `27 tests run: 26 passed, 1 failed`, reddening exactly its own test:
`identity_digest_changes_when_an_import_member_arity_changes` and
`identity_digest_changes_when_diagnostic_span_presence_moves_between_diagnostics`
respectively. After both restores the selector was 27/27 GREEN and
`git status --porcelain` was empty.

## The producible-kind declaration, both directions

| Plant | Where | Target test | Observed |
|---|---|---|---|
| Add `PublicApi` to `VUE_PRODUCIBLE_KINDS` | `PLANTED_VUE_ADMITS_PUBLIC_API` | `vue_route_produces_exactly_the_kinds_it_declares_producible` | **First attempt GREEN — a real gap in the new test**, then RED after the fix below. |
| Add `Analysis` to `VUE_PRODUCIBLE_KINDS` | `PLANTED_VUE_ADMITS_ANALYSIS` | same | RED. |
| Remove `RuntimeClient` from `VUE_PRODUCIBLE_KINDS` | `PLANTED_VUE_DROPS_RUNTIME_CLIENT` | same | RED. |
| Add `IdeCompanion` to the Svelte derived kind set | `PLANTED_SVELTE_ADMITS_IDE_COMPANION` | `svelte_route_produces_exactly_the_kinds_it_declares_producible` | RED. |
| Make `refuse_unproducible_runtime_surface` always admit the server surface | `PLANTED_SVELTE_SERVER_ALWAYS_PRODUCIBLE` | the Svelte producible test, `compile_batch_unsupported_svelte_runtime_server_performs_zero_carrier_prepares`, `svelte_runtime_server_request_fails_closed_not_reinterpreted`, `svelte_dual_runtime_client_and_server_request_fails_closed_with_no_partial_output` | RED 4/4 from ONE edit, which is the point: preflight, compile loop and refusal all follow from that single answer. |


**The first `PublicApi` plant stayed GREEN, and that is the most useful result in this
file.** The two-sided test as first written compared only the refusal VALUE — and the
plan preflight and the late completeness check both return `UnsupportedProduct(kind)`,
so admitting a kind the route cannot emit produced a byte-identical error. What actually
differs is the work done before the refusal: an admitted kind gets its carrier PARSED
first. The test now observes that through `compile_batch`'s reported `cold_build_count`,
which must be zero for every unproducible kind and one for every producible one. All
five rows above are against the strengthened test.

## One field the publication boundary will not let a test isolate

`source_projection_map` is REQUIRED on an `IdeCompanion` artifact and REFUSED on a
runtime one, so present-versus-absent cannot be varied independently of the product
kind through `publish`. Its VALUE is bound
(`identity_digest_changes_when_the_source_projection_map_changes`, reddened by the
`PLANTED_OMIT_SOURCE_PROJECTION_MAP` row above), and that is the discrimination that
matters: a hasher that stops covering the field fails.

A parallel draft closed the presence case instead by adding `#[cfg(test)]` constructors
that mint an `AssembledArtifact` copy and an `ArtifactSet` without going through
`publish`. That is not adopted here. `ArtifactSet` deliberately has no `Default` and no
public constructor precisely so that a publishable-looking set cannot exist without
having passed the atomicity checks, and a test-only bypass of that is a worse trade than
one unbound presence assertion — especially when the field's value is already bound.


## Open scope challenge — the publication surface moved

Recorded here rather than resolved, because it is not this block's to decide.

Relative to the accepted predecessor base, this branch changed
`crates/verter_compiler/src/assembly/publish.rs` in three ways that are visible to a
caller:

- `AssembledArtifact` now CARRIES the emitted-import fact through publication and
  exposes `emitted_imports()`. At the predecessor base, `emitted_imports` existed only
  as an `ArtifactContribution` input field; the published artifact dropped it after the
  atomicity check.
- Two new `AssemblyRefusal` variants, `UnreportedDeclaredImport` and
  `UndeclaredEmittedImport`.
- A reverse declared-versus-emitted import check, which refuses a fragment-declared
  import the composer never emitted, and an emitted import no fragment declared —
  including side-effect imports, which bind no name and therefore escape the
  `UndeclaredHelper` check entirely.

**Why they exist.** The reverse check closes a real fail-open that an adversarial plant
found: with a specifier-only `.any()` match, an artifact could publish both a named and
a side-effect import for the same specifier while only one was declared. The accessor
exists because the four-route identity digest must observe the emitted-import set; a
fact validated at publication and then discarded cannot be compared across routes.

**Why it is a challenge and not a decision.** This block's charter says prepared state
may not change publication meaning, and the binding spec freezes the predecessor's
publication boundary. These changes are in that boundary's owner layer. Reverting them
would re-open the fail-open and remove a field the identity oracle depends on;
keeping them is a scope expansion this block cannot authorise on its own. The
disposition belongs to the program orchestrator and, if it agrees the challenge has
merit, to an architecture ruling.


## Inputs the digest hashes that NO test isolates

Named because the alternative — leaving them out of the ledger — is how the last two
false claims survived.

Each of these was deleted under adversarial review and the whole `identity_digest_`
selector stayed GREEN:

| Input | Marker | Result |
|---|---|---|
| `artifacts.len()` | `ADVPLANT_ARTCOUNT_A7` | 27/27 passed |
| `artifact.emitted_imports().len()` | `ADVPLANT_IMPCOUNT_A6` | 27/27 passed |
| `output.styles.len()` | `ADVPLANT_STYCOUNT_A8` | 27/27 passed |
| `output.diagnostics.len()` | `ADVPLANT_DIAGCOUNT_A9` | 27/27 passed |
| the `Named` kind tag collapsed onto `SideEffect` | `ADVPLANT_NAMEDTAG_B5` | 27/27 passed |

**Two of those five turned out to be isolable, and are now bound.** A later review
attacked the list rather than accepting it, and the encoding's weakness is at SECTION
boundaries rather than inside a record:

| Input | Plant | Test that now reddens |
|---|---|---|
| `artifact.emitted_imports().len()` | `PLANTED_OMIT_IMPORT_COUNT` | `identity_digest_changes_when_an_import_moves_between_artifacts` — RED, alone |
| `output.styles.len()` | `PLANTED_OMIT_STYLE_COUNT` | `identity_digest_changes_when_a_style_block_is_replaced_by_diagnostic_bytes` — RED, alone |

The selector for these runs was `test(identity_digest_) or test(refused_svelte_namespace)`,
not `test(identity_digest_)` alone — the run totals in this file are against that combined
filter. An earlier revision quoted one of those totals beside the narrower selector name,
which does not reproduce.

An empty-specifier `SideEffect` import encodes as nine zero bytes and a `RuntimeClient`
head with empty code and the `JavaScript` dialect as twenty-six; the two runs commute, so
without the count prefix the same import attributed to a different artifact encodes
identically. For styles, one 24-byte-code style block plus its 91-byte all-empty
descriptor is re-read as a 24-diagnostic count plus a 103-byte diagnostic code; both arms
land on exactly 735 bytes.

**The three that remain, and their honest status:**

- `output.diagnostics.len()` — believed genuinely rigid, with an argument rather than a
  bare assertion: it is the LAST section, every diagnostic record is self-delimiting, and
  a self-delimiting record sequence with a known stream end is uniquely decodable, so no
  two distinct diagnostic lists can collide once the count is removed.
- `artifacts.len()` — attacked and held. The obstacle is the descriptor's fixed 91-byte
  block: the lengths cannot be balanced without introducing a nonzero length byte where
  the other arm has a zero. A recorded failed attempt, not a proof.
- The `Named`-versus-`SideEffect` tag — **the lead was executed, and it IS isolable.**
  Under a tag collapse `Named([])` encodes as `SideEffect` followed by its own
  eight-byte zero arity, so two imports with empty specifiers — one `Named([])`, one
  `SideEffect` — encode identically once swapped, while unplanted the streams differ
  only in whether the `0x03` sits at offset 8 or 17. The existing tag test compares
  `Default` against `Namespace` (tags 1 and 2) and structurally cannot see it. Closed by
  `identity_digest_changes_when_a_named_import_and_a_side_effect_import_swap`, RED under
  `PLANTED_COLLAPSE_NAMED_TAG` (33 run / 32 passed / 1 failed, its own test alone).

  That is the FIFTH time an input on this list has turned out to be isolable after being
  written down as one nothing could reach. The remaining two rows above are recorded with
  their actual status for that reason, and neither is claimed unreachable.

They are kept because they are what makes the encoding self-delimiting, which is what
makes every other field's discriminator sound. **No claim is made that no input could
isolate them.** This block asserted exactly that twice, about the arity prefix and the
span presence tag, and both times a reviewer produced the counterexample — which is why
those two now have tests. The honest statement is the one above: planted, observed
green, retained deliberately, not proven unreachable.

Two more of the same class, both from the same review:

- **`span.start` and `span.end` were individually neuterable** — the value test varied
  both at once, so either alone carried it. Closed:
  `identity_digest_changes_when_only_a_diagnostic_span_start_changes` and
  `..._end_changes` hold the other component fixed.
- **Two layered fail-closed checks are individually deletable with the suite green**:
  the plan preflight inside `compile_vue_from_parsed` (`ADVPLANT_PREPARED_PREFLIGHT_VUE_M1`,
  49/49 passed) and the Vue completeness check (`ADVPLANT_COMPLETENESS_R1B`, 49/49
  passed). Each is absorbed by the layer beneath it, and removing ALL THREE product
  gates at once (`ADVPLANT_TRIPLE_S1`) still fails closed at `publish` with
  `MissingPlannedArtifact`. They are defence in depth, not discriminated behaviour, and
  the doc comment calling the completeness check "the ENFORCEMENT" overstates what a
  test can currently see.

## A fail-open the reverse check could not reach

The emitted-import check special-cased `SideEffect` and sent every other kind through a
per-name loop over `bound_names()`. `Named` with an EMPTY member list binds no names
either, so that loop's body never ran and such an import published without any fragment
having declared it — contrary to the `UndeclaredEmittedImport` contract this route
documents, and contrary to this file's own earlier description of the reverse check as
covering "including side-effect imports, which bind no name".

The reverse check could not cover it: it iterates the imports a FRAGMENT declared, and the
hole is exactly the case where the fragment declares none. Both directions were blind to
the same shape from opposite sides.

Closed by choosing the branch on whether the import binds anything rather than by naming
one kind, so the rule states the actual condition instead of one instance of it.

## The gap the ledger's own shape predicted

Adversarial review found one production path with no test at all: the SVELTE half of
`compile_prepared`'s stale-source refusal. `if false {` on that check left
`49 tests run: 49 passed` at the standalone scope and
`6383 tests run: 6381 passed, 2 failed` across the whole crate — and those two failures
were the pre-existing tsc launcher tests, verified failing identically on the
unmutated tree. Nothing in `verter_compiler` caught it.

The Vue siblings were bound; the Svelte arm is a separate `if` in a separate match arm
that no Vue test can reach. Without it, `compile_svelte_from_parsed` receives the NEW
source alongside the OLD parse, and both the runtime lowering and the output descriptor
read that source — mixing stale spans with fresh bytes into exactly the silently-wrong
result the refusal exists to prevent.

Closed by `compile_prepared_rejects_a_different_svelte_source_as_stale`. Note where the
gap was: this ledger had twenty digest rows and twelve confirm rows and NOT ONE
staleness plant. The untested path was precisely the one nobody had planted.


## Notes carried forward, so a later reader does not re-find them

- **The four-route "agree on diagnostics" claim is Vue-only in practice.**
  `compile_svelte_from_parsed` hardcodes `diagnostics: Vec::new()`, so no Svelte route can
  populate the digest's diagnostic section at all; the claim rests entirely on the Vue
  duplicate-directive fixture. That is framework semantics and not this block's to change
  — recorded so the identity claim's SCOPE is not read wider than it is.
- **`compile_batch` increments `report.reuse_count` even when `compile_prepared` returns
  `Err`.** Consistent with the field's documented meaning (the number of
  `compile_prepared` calls), so not a defect — recorded because it looks like one.
- **`fragment_declaring` changed behaviour in the same change that moved the publication
  tests to a sibling file.** It stopped generating real import-binding code and now uses a
  fixed `export default {}` body. That is legitimate — `publish` compares declared and
  emitted FACT sets and never re-reads `code`, which is exactly what lets a digest test
  express an import whose specifier is not writable as JavaScript — and it is contained to
  `DigestFixture`, with the fact-versus-code publish tests still using the code-bearing
  helpers. It is called out because a reviewer diffing that commit as a pure file move
  would not see it.


## What stops each finding coming back

Per finding: the edit that would reintroduce it, whether that edit still compiles, and the
tier of rail that catches it. Where a fix stopped below tier 1, the reason is stated rather
than left implied.

| Finding | Reintroducing edit | Compiles? | Tier | Rail |
|---|---|---|---|---|
| Emitted `Named([])` published unchecked | narrow the predicate back to `matches!(kind, SideEffect)` | yes | **3** | `emitted_named_import_binding_no_names_is_refused_when_undeclared` — RED under `PLANTED_NARROW_TO_SIDE_EFFECT_ONLY`, GREEN restored, with a positive control so the refusal is about the missing declaration rather than about empty `Named` imports |
| Svelte stale source accepted | `if false` on, or deletion of, the Svelte source-digest check | yes | **3** | `compile_prepared_rejects_a_different_svelte_source_as_stale` |
| Svelte namespace parsed before refusing | delete `resolve_svelte_namespace` from the preflight | yes | **3** | `compile_batch_refused_svelte_namespace_performs_zero_carrier_prepares`, asserting `cold_build_count == 0` — the error VALUE is unchanged by that edit, so only the work assertion catches it |
| Any one digest field silently unhashed | delete that field's `hash_*` line | yes | **3** | one `identity_digest_*` test per field, each observed RED under its own plant and reddening no other |
| Producible-kind declaration drifts | add or remove a `ProductKind` in the declaration | yes | **3** | the two `..._produces_exactly_the_kinds_it_declares_producible` tests, whose expected set is stated INDEPENDENTLY of the production constant, plus a `cold_build_count` assertion for the added-kind direction |
| Prepared carriers become cloneable | `#[derive(Clone)]` on a carrier | **no** | **2** | the private zero-sized `SingleOwner` field makes the derive an `E0277`, and three unconditional library-code `assert_not_impl_any!` asserts fire independently as `E0283` — both gates observed firing together |

**Why five of the six stopped at tier 3.** Each is a predicate over data inside one function —
"does this import bind a name", "is this digest already stale", "which kinds can this route
emit" — not a construction that a type could forbid. There is no value to make private, no
constructor to seal, and no ordering to encode as type-state: the wrong version of each is
ordinary, well-typed Rust over the same inputs. Tier 1 and tier 2 were considered and are
genuinely unavailable, so the honest record is that these are plant-proven tests and that a
determined future edit can rewrite the predicate; what it cannot do is leave the test green.

The single-owner rail is the one that reaches tier 2, and it does so because the defect there
IS a construction — a derive — which a missing trait bound can reject outright.

## Assertions that do NOT discriminate, stated plainly

Not every assertion in this block is a discriminator, and pretending otherwise is the
failure mode this file exists to prevent.

- `identity_corpus_actually_populates_the_map_and_diagnostic_digest_slots` is a COVERAGE
  guard. It fails if the corpus stops exercising the map or diagnostic slots; it proves
  nothing about the digest itself. The per-field rows above are the discriminators.
- `identity_digest_changes_when_the_style_count_changes` and
  `identity_digest_changes_when_a_diagnostic_appears` are not bound to the `hash_usize`
  length prefixes they sit next to: because every element is hashed length-prefixed,
  one block and two blocks already differ without the count. They discriminate "styles
  and diagnostics are hashed at all", which the per-field rows also cover.
- The structural preconditions inside the weight tests (no root attributes, the
  `SmallVec`s are inline, exactly one element node, the arena is allocated) are controls
  that keep the expected totals exact. The final equalities are the discriminators.
- `compile_batch_of_no_items_returns_no_results_and_does_no_work` is close to a control:
  deleting the early return leaves the same observable result, because the loop is then
  a no-op. It discriminates a spurious-work regression, not the early return's presence.
- `compile_prepared_failure_constructs_no_partial_output` is a CONTROL despite its name.
  It builds a Vue request for an unproducible product, calls `prepare` then
  `compile_prepared`, and matches the error variant — it never binds a
  `DirectCompileOutput` and asserts nothing about output. A partial result is impossible
  by construction because the function returns `Result`, so the test cannot fail for the
  reason its name states; it fails only if the error variant changes. It is a sanctioned
  shape, and listing it here is what makes this section complete.
- The wall-clock and RSS figures in the route-overhead receipt are informational. At a
  sub-millisecond operation, process-start jitter dominates and a green wall number is
  close to unfalsifiable. The discriminating evidence there is the measured
  carrier-parse counters and the cross-route digest equality.
