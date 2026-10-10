# Embedded text codecs and exact authored maps

The shared codec owns embedded-region geometry. Profiles own activation,
language grammar, delimiter and escape semantics, and interpolation meaning.
The codec must never decide that a tag spelled `html` is Lit or that an object
property spelled `template` is Vue. Activation comes from the canonical-symbol
evidence in [export-activation.md](export-activation.md).

This decision describes `docs(arch): ratify the Angular release, activation,
parser, host and (#822)`, 2026-10-10. It follows the docs-only rule in
[README.md](README.md). No production code, test, validator or gate is added.
The current framework-shaped host/session registries remain live. The final
owner is the typed immutable catalog and demand-selected kernel services;
the shared implementation home is **`verter_language::embedded`**, owned by
EMB0I. This page specifies that future interface, not shipped support.

## Reviewed products and ownership

The contract data in `tests/kernel/EMB0/products/` consists of:

| File                            | Population                                                                                                                                               |
| ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `embedded-inventory.v1.json`    | Outcomes, all direct plan consumers plus the tagged-template adapter, API boundaries, current source census, one owner/path/receiving acceptance per row |
| `embedded-route-ledger.v1.json` | Inherited deletion categories, consumer-local rejection of private codecs, retained mapping authorities and empty populations                            |
| `embedded-case-table.v1.json`   | Raw/cooked, Unicode, delimiter, CRLF, indentation, holes, composition, incremental splice, fuzz and work-count cases                                     |

EMB0I owns the primitives, `CookedText`, `EmbeddedTextCodec`,
`AuthoredMapChain`, and the executable corpus (`EMB0I-AC1`–`EMB0I-AC4`).
UAO0 owns the executable ownership validator (`UAO0-AC-R1`) on the valid path
EMB0 → EAK1 → UAO0. The validator must reject missing inventory members,
unknown/pathless owners and conflicting assignments. No validator is shipped
by this decision.

Every implementation consumer uses EMB0I's primitives. INT-TT composes the
tagged-template adapter and hole-placeholder registry over them; LIT1 uses
that adapter and adds Lit's dialect/profile rows. Neither creates a private
decoder or composer. Family grammar and attachment nodes have separate
receiving rows: grammar recognition never acquires attachment activation
authority. ANGP, ASTP and LITP are findings-only probes, not implementation
prerequisites. FMK0, DIAL0 and GQL0 consume the contract as decisions, not as
permission to implement another codec. The conformance checkpoint audits
delivered products and owns no runtime route.

## Identity and named API boundaries

The codec consumes the repaired `SourceUnitId` and VID0T's `RegionId` and
`AttachmentId`, following [identities.md](identities.md). A region has
profile-independent geometry under `(SourceUnitId, RegionId)`; a semantic
attachment is a profile-qualified claim on that region. Nested regions keep
their parent lineage and declared authored ranges. Two components sharing an
external template share the external source unit but have distinct attachment
contexts. An edit must not rename an unaffected surrounding region. There is
no standalone `EmbeddedSourceId`.

`SourceRevision`, `ContentId` and `MapRevision` qualify the basis separately
from stable lineage. A map endpoint names its unit, coordinate domain, content
and map revision. Local `SourceSpaceId` and `BlockId` at the described head are
artifact-local geometry, never a replacement for source-unit identity.
Existing sealed `ArtifactBlockRef` and `QualifiedBlockContentSourceMap`
authority are preserved until their receiving owners migrate them.

| Named boundary       | Required relationship; sole implementation owner                                                                                                                                |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `CarrierProfileId`   | Selects the embedded syntax carrier, syntax epoch and codec policy; VID0T supplies identity, EMB0I consumes it                                                                  |
| `FrameworkProfileId` | Qualifies an activated semantic attachment; never determines syntax by a central framework switch; VID0T supplies identity                                                      |
| `ProjectProfileId`   | Qualifies project context/base-URI resolution when observed; never becomes a second source identity; VID0T supplies identity                                                    |
| `CatalogSnapshot`    | CAT0 `T05` declares host carrier + region role → embedded carrier, codec policy and map-chain kind; EMB0I owns this embedded table contract, CPF1 retains snapshot construction |
| `DemandPlan`         | DEM0's selected/requested operation supplies demand; a selected but unrequested region does no decode/parse/map work; COX0 implements planning                                  |
| `TypeInfoRequest`    | Consumes an exact attachment and map basis; no private component information or coordinate recovery; TIF1 owns the request/view cutover                                         |

These are references to existing owners, not a second migration of the catalog,
planner, TypeInfo or identity vocabulary. Ownership paths rooted in their
original contract are recorded as inherited references in the inventory.
Syntax keys contain carrier/policy/epoch and observed source/map basis;
semantic results additionally contain the exact activated profile/context.
Requester encoding and backend/process identity never enter codec identity.

The attachment's base URI identifies the actual authored resource used for
relative references. An external Angular template or stylesheet uses its
external resource base, not the component's start offset. Contextual resolution
still observes the component/project attachment separately. No map may silently
substitute a carrier URI for an external resource URI.

## Raw and cooked admission

The profile declares raw versus cooked selection before decoding, together
with delimiter, escape, line-terminator, indentation and hole policies. These
are data/typed policy inputs to neutral operations, not framework-name arms.
Raw input retains its exact authored spelling. Cooked input uses the host
literal's value, whose positions need not equal authored positions. The codec
does not execute JavaScript, a template, a preprocessor or a runtime plugin.

A JavaScript cooked value is potentially UTF-16 code units, not necessarily
Unicode scalar text. Before any embedded parser call, offset minting or map
publication, validate the complete cooked value. A paired surrogate becomes
one scalar encoded as four UTF-8 bytes. A lone high or low surrogate produces
typed **`NonUnicodeCookedLiteral`** partiality; never U+FFFD replacement,
WTF-8 output, lossily decoded text or an invented byte position. An invalid
escape in a tagged template can leave its cooked value absent: cooked selection
returns **`MissingCookedValue`**, whereas an explicitly raw profile can still
admit the raw spelling. There is no fallback from cooked to raw.

Private code-unit indices may exist while validating the input. They never
escape the codec as public, cached or core offsets. After validation,
`CookedText` is scalar UTF-8. The codec's geometry is ENC0 `CD4` embedded bytes;
authored geometry is `CD1`, generated geometry is `CD3`, and relative source
spans remain `CD2`. EMB0I consumes ENC0T's checked types and arithmetic.
Inside-scalar, inverted, overflowing and out-of-extent ranges are typed
failures, without clamping or nearest-token guesses.

Delimiter removal, escape contraction, CRLF normalization, line continuation
and indentation removal each contribute explicit mapping segments. For example,
the six authored ASCII bytes `\u00E9` cook to the two UTF-8 bytes of `é`:
the whole scalar can refer to the whole escape, but its interior cannot acquire
a fictitious authored byte by proportional interpolation. Removed continuation
and indentation bytes are elided preimages, not empty mapped runs. An anchor
exists only when explicitly declared by the producing policy.

Interpolation holes are separate, ordered authored-expression regions with
their own lineage and ranges. Profile policy admits, rejects or classifies the
hole; neutral geometry never evaluates it. Adapter-generated placeholders are
synthetic and keep their declared hole association; their characters are not
editable expression bytes. A range crossing a hole is refused for a single
contiguous edit unless the profile and every map stage prove a unique exact
preimage. Nested depth is bounded by an explicit admitted request/profile
budget; exhaustion returns typed partiality before recursive work escapes it.

## Exact composition and operation safety

`AuthoredMapChain` composes independently identified stages. Syntax decoding,
preprocessor expansion, compiler output, runtime-build maps and code-generation
provenance keep their stage kind and source/output identities. A V3 map's
line/UTF-16 columns convert at its adapter against its exact text snapshot;
internal stages remain checked UTF-8 bytes. An absent optional runtime map is
absence, not a zero identity or evidence that syntax positions are exact.

Composition joins only when an output basis equals the next input basis,
including source unit, content, domain and revision. It carries all contributing
source identities, base URIs, attachment contexts and ordered provenance.
Reordered discovery cannot change map answers: segments order by output range,
then declared stage order and canonical source/region/attachment identities.
Coincident segments preserve the producer's declared order; disagreement is
ambiguity, never last writer wins.

An admitted query returns exact authored UTF-8 geometry or a typed
`Unmappable` reason (synthetic output, elided source, non-invertible expansion,
hole, ambiguous preimage, missing map, stale/mismatched basis or invalid
boundary). These are reason categories for the implementation, not permission
to turn a complete result into a silent empty answer. Discontiguous or repeated
preimages can be returned as ordered provenance for display; they cannot be
flattened into a guessed contiguous editable span. Mapping range endpoints
independently is insufficient: the whole range needs one compatible exact
component and a unique preimage for an edit.

Rename and safe fixes require reversible source identity and an exact unique
editable preimage through every stage, including the profile's declared reverse
encoding for a cooked replacement. An exact blame span alone does not certify
that inserting unescaped cooked text is a valid authored edit.
Diagnostics, debugging and coverage may
surface an unmappable result with its reason and generated/derived basis; they
must not blame the nearest authored token. A debug/coverage projection can
retain multiple exact origins without acquiring edit authority. A syntax map
does not certify runtime provenance, and a runtime map does not certify safe
edits. No universal projection mask grants all operations together.

| Amended consumer                   | Exactness requirement                                                                                          | Receiving owner/acceptance             |
| ---------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------- |
| Pug                                | Expansion/mixin origins remain ordered; repeated output has no guessed unique edit origin                      | DIAL2, `DIAL2-AC2`                     |
| Stylesheet dialects/custom PostCSS | Transform output missing trustworthy maps disables authored automatic edits; retain syntax versus build stages | DIAL7, `DIAL7-AC2`; STP38, `STP38-AC1` |
| Lit                                | Escape/CRLF/hole expression maps compose through INT-TT; Lit owns activation/dialect policy                    | INT-TT, `INT-TT-AC1`; LIT1, `LIT1-AC2` |
| MDX                                | ESM/expression/JSX nested regions retain exact ranges and attachment identities                                | MDX1M, `MDX1M-AC1`–`MDX1M-AC2`         |
| Angular external resources         | Inline escapes and external source/base identity; shared resource has distinct component contexts              | ANG1M, `ANG1M-AC1`–`ANG1M-AC3`         |
| Glimmer/GJS/GTS                    | Template-tag/co-located regions preserve exact authored lineage                                                | GLM1M, `GLM1M-AC1`                     |
| Marko                              | Embedded script/style/expression regions preserve authored identity                                            | MRK1M, `MRK1M-AC1`                     |
| Astro                              | Frontmatter/expression/script/style geometry stays exact; per-region invalidation                              | AST1M, `AST1M-AC1`–`AST1M-AC4`         |

Profiles share primitives only when their authored semantics are expressible
by neutral policies. If that would require a language branch in the neutral
codec, abort sharing for that language and obtain the owning-contract amendment;
do not add a silent fallback or privately implement the same authority.

Stylesheet dialects here include SCSS, Less, indented Sass and Stylus, as
inventoried by DIAL0. Their syntax/fact owners produce authored geometry;
STP38 owns external transform identity and DIAL7 owns custom transform-stage
adapters. The map contract does not replace their language parsers, external
Sass/Less/PostCSS runtimes or the CSS printer. Each transform output, including
one with no trustworthy map, is represented truthfully in the chain.

## Migration and deletion disposition

This decision deletes no production route. The three required categories refer
to the existing [authority inventory](authority-inventory.md) and
[coordinate inventory](coordinates.md), with their existing deletion owners:

- Central framework switches: UAK0 `D01`–`D07` and `D08`'s conflated identity
  stay with CPF1; embedded policies cannot introduce another switch.
- Untagged coordinate/public identity: UAK0 `D16`/`D17` and ENC0's concrete
  boundary rows stay with ENCF0/ENCL0; codec domain types come from ENC0T.
- Duplicate component information authority: UAK0 `D12`–`D15` stay with TIF1,
  and `D18` stays with IDX0; an embedded attachment does not own TypeInfo.

There is no public embedded codec, cooked-source identity, Angular/Lit literal
decoder or family-neutral map composer at the described head. Those displaced
embedded-only populations are empty now. The route ledger enumerates the
consumer-local rejection owner for every future private geometry route; that
owner adopts shared primitives and removes any superseded decoder only after
byte/map equivalence. EAK1 specifically owns any superseded Vue bespoke literal
path under `EAK1-AC1`/`EAK1-AC2`. EMB0I implements primitives but switches no
consumer. No compatibility bridge or dual authority is authorized.

`CodeTransform` mapping products, `ContentMapper`, the canonical boundary
converters, `QualifiedBlockContentSourceMap`, the Vue SSR HTML-entity decoder
and Svelte runtime constant folding retain their owners. Entity interpretation
and runtime folding are language semantics, not authored embedded-region
geometry; their presence is not evidence of a duplicate embedded codec. The
current census in the inventory records concrete files and symbols so later
reconciliation checks the code rather than a historical commit.

## Corpus and acceptance evidence

The case table specifies distinct regression boundaries, not new tests here.
EMB0I implements its corpus in
`crates/verter_language/tests/cases/embedded_codec.rs`. Family integration cases
remain with their receiving consumers. Each case records input, required and
forbidden result, owner/path and receiving acceptance. The corpus includes:

- Vue-, Angular- and Lit-shaped literals using the same primitives with
  different profile-owned activation, grammar and hole dispositions;
- Lone high/low surrogates, valid surrogate pairs, absent cooked values from
  invalid tagged escapes, raw admission, escaped delimiters and astral text;
- LF/CRLF/lone-CR policies, line continuations, indentation, empty text,
  holes/nested holes and explicit depth exhaustion;
- Syntax → preprocessing → compiler → runtime maps, external base URIs,
  non-invertible expansions, synthetic spans and stale/mismatched stages;
- Incremental splice/edit/revert versus fresh decoding of bytes, maps,
  partiality, identities and ordering, including recovery around broken
  literals; stale/cancelled/degraded artifacts cannot warm complete results;
- Bounded fuzzing over the same policies and deterministic work/allocation
  counts or growth ratios, including inactive/unrequested zero work.

**EMB0-AC1:** the reviewed inventory and ledger supply the ownership contract;
UAO0 implements the validator later. Existing ancestor owners are referenced,
never reassigned. Direct successor membership and receiving acceptance clauses
were inspected in the controller plan; plan state is not shipped as a validator.

**EMB0-AC2:** existing evidence discriminates the authorities being preserved:

| Existing evidence                                                                                                                                                                                                                                                                                 | What it proves; limit                                                                           |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `verter_language/tests/cases/sealed_block_identity.rs`: `foreign_artifact_ref_fails_owner_validation`, `artifact_identity_is_content_addressed_not_generation_bound`                                                                                                                              | Artifact-bound geometry rejects foreign ownership; content identity is separate from generation |
| `verter_language/tests/cases/parse_identity.rs`: `identical_inputs_share_parse_keys_and_parse_affecting_changes_do_not`                                                                                                                                                                           | Exact parse-affecting inputs qualify identity; not cooked-literal coverage                      |
| `verter_session/tests/cases/content_mapper_projection.rs`: `rewritten_text_answers_at_region_granularity_and_refuses_inside_itself`, `relocated_preimages_with_a_gap_between_them_refuse_instead_of_spanning_it`, `a_carrier_position_answers_with_every_projection_in_ascending_projected_order` | Refusal for rewritten interiors/discontiguous origins and deterministic projection order        |
| Same file: `an_edit_changes_the_map_revision_while_the_carrier_unit_survives_it`                                                                                                                                                                                                                  | Map revision changes independently of source-unit lineage                                       |
| `verter_session/src/block_content.rs`: `valid_source_map_v3_rejects_a_decodable_map_with_an_out_of_bounds_token`, `supplied_change_during_compile_cannot_publish_stale_output`                                                                                                                    | Preprocessor map bounds and stale publication refusal                                           |
| `verter_compiler/src/code_transform/chain_tests.rs`: `a_sourceless_segment_is_a_barrier_at_every_lookup_after_it`, `a_column_splitting_a_surrogate_pair_is_refused`                                                                                                                               | No extrapolation through sourceless output; checked V3 UTF-16 columns                           |
| `verter_protocol/tests/cases/typeinfo_graph_export.rs`: `encode_is_deterministic_and_wire_roundtrips`, `by_design_opaque_degradations_carry_explicit_markers`                                                                                                                                     | Ordered wire projection and explicit degraded result markers                                    |

These tests do not prove the unimplemented `CookedText`, catalog `T05`,
surrogate ingress or cross-family integration. Those exact gaps are received
by EMB0I's acceptance and the named family consumers, not hidden by a docs pass.

**EMB0-AC3: not applicable to this diff.** No incremental/cache/cancellation,
partial-result or stale-publication authority changes. The case table binds
future splice equivalence and degraded non-warming to `EMB0I-AC4`; consumer
integration keeps its own state-safety acceptance.

**EMB0-AC4: not applicable to this diff.** No hot path changes, parse, resolve,
plan, emit, allocation or retained candidate is added. Future work is bounded
by validated input bytes, mapping segments, holes and admitted nesting depth;
the corpus requires deterministic invocation/allocation counts and growth
ratios, with a plain-carrier/unrequested control. No timing or RSS threshold,
counter, soak or benchmark is invented here.

For the implementation's work model, validate/decode each observed chunk once;
charge every transform stage against its own input/output bytes and segments,
not merely the original carrier size. A lookup visits at most the admitted
chain depth; retaining full prefix copies at each stage is not an acceptable
composition strategy. Report these counts against a fresh plain/raw control
and repeated warm demand, with no duplicate parse or resolution in a mapper.
