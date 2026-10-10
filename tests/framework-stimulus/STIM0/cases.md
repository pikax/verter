# Stimulus and static Turbo contract cases

This is a reviewed case table, not executed coverage. The products describe planned
delivery, not shipped features. STIM1-ACV owns `contract.ts`,
`stimulus-lock.spec.ts` and every rejection below, using
`node --test tests/framework-stimulus/STIM0/stimulus-lock.spec.ts`. REG0's single
discovery runner picks up that spec when it exists. This contract adds no validator,
test, root script or CI gate.

## Acceptance and evidence

| Acceptance                 | Reviewed evidence here                                                                                                                                                 | Executable receiving obligation                                                           |
| -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| STIM0-AC1: pinned profiles | `products/stimulus-version-lock.json`: Stimulus 3.2.2, Turbo 8.0.23, exact wrapper release, two asset-header-backed gem mappings                                       | STIM1-ACV: every pin and mapping mutation below                                           |
| STIM0-AC2: owned matrix    | `products/stimulus-capability-matrix.json`: one producer and receiving acceptance per cell; explicit exclusions, stage set, selectors, coexistence and DX1 descriptors | STIM1-ACV: matrix mutations; STIM9-AC1: passing fixtures per admitted cell                |
| STIM0-AC3: activation      | `products/stimulus-activation-policy.json`: independent FWA1 records, PM roles, resolved releases, Gemfile.lock and explicit-on evidence                               | STIM1-ACV: activation mutations; FWA1-AC7 and STIM4T-AC3: independent activation behavior |
| STIM0-AC4: facets absent   | Matrix records props, events, slots and expose as does-not-exist, each with a reason                                                                                   | STIM1-ACV: facet mutations                                                                |

Existing executable Stimulus coverage is not claimed: the implementation nodes are
receiving obligations. Incremental, cancellation, lifetime and performance tests
are not applicable to this data-only change. Their production obligations remain
with STIM1–STIM9. No framework/runtime oracle is installed or run here.

## Clean controls

| Input                                                              | Required outcome                                                                         |
| ------------------------------------------------------------------ | ---------------------------------------------------------------------------------------- |
| Resolved `@hotwired/stimulus` 3.2.2, no Turbo evidence             | Only Stimulus active; no Turbo claims                                                    |
| Resolved `@hotwired/turbo` 8.0.23, no Stimulus evidence            | Only Turbo active; no Stimulus claims                                                    |
| Resolved `@hotwired/turbo-rails` 8.0.23 with installed core 8.0.23 | Turbo active from PM proof, not from its declared dependency range                       |
| Gemfile.lock has stimulus-rails 1.3.4 and turbo-rails 2.0.23       | Two records: Stimulus 3.2.2 and Turbo 8.0.23, each with its own sourced vendored mapping |
| No dependency evidence; explicit `on` naming 3.2.2 or 8.0.23       | Only the named framework active, with explicit provenance                                |
| Both admitted dependencies; `turbo=off`, `stimulus=auto`           | Stimulus unchanged; zero Turbo demands, facts, operations and rules                      |
| Both admitted dependencies; `stimulus=off`, `turbo=auto`           | Turbo unchanged; zero Stimulus demands, facts, operations and rules                      |
| Both records off, with framework-looking HTML attributes           | Plain shared HTML; zero overlay work                                                     |
| `keydown.enter@window->search#submit:prevent`                      | Exact event, key filter, event target, identifier, method and option spans               |
| `click->users--list#select`                                        | Identifier remains `users--list`; its declaration identity comes from registry facts     |
| `<form data-action="x#save">` versus `<div data-action="x#save">`  | Default submit for form; typed missing-default fact for div                              |
| `data-x-count-value="3a"` versus `"1_000"`                         | Non-coercible NaN fact versus number 1000; no user code evaluation                       |
| `data-action="click-><%= id %>#open"`                              | ERBH1 supplies the hole; click/open exact, identifier unknown                            |
| `<turbo-stream action="replace" target="<%= dom_id(x) %>">`        | Replace action exact, target unknown; no Ruby evaluation                                 |

## Planted rows and expected failure reasons

Each row is a required future negative control. A validator accepting any planted
row fails STIM1-ACV; writing this table does not claim those experiments ran.

| Case  | AC  | Plant                                                                                   | Expected failure reason                                                                                                      |
| ----- | --- | --------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| PIN01 | AC1 | Replace an exact release with `3.2.x`, `8.0.x`, `^3.2.2`, `latest`, canary or nightly   | Admission requires an exact sourced release, not a range or channel                                                          |
| PIN02 | AC1 | Add a Stimulus 2.x row                                                                  | Legacy major is excluded                                                                                                     |
| PIN03 | AC1 | Add Turbo 7.x or a public next-major profile                                            | Legacy and unratified next major are excluded                                                                                |
| PIN04 | AC1 | Change a profile release without its source/vocabulary or diverge wrapper/core releases | Exact profile and resolved core provenance no longer agree                                                                   |
| PIN05 | AC1 | Add a gem-to-JS mapping with no asset source/header or gem version source               | Unsourced vendored mapping cannot establish admission                                                                        |
| PIN06 | AC1 | Infer turbo-rails 2.0.20 → Turbo 8.0.20 from package.json                               | That tag's vendored header is 8.0.19; a dependency range is not vendored-version proof, and neither release is admitted here |
| PIN07 | AC1 | Admit an unlisted gem by matching its major or nearest table row                        | Gem table is an exact allowlist; unknown releases stay unsupported-version                                                   |
| MAT01 | AC2 | Delete a cell's producer or receiving acceptance                                        | Every cell needs one producer and one receiving AC                                                                           |
| MAT02 | AC2 | Add two producers to one cell, or name cancelled STIM6                                  | Ownership must be singular and the stage must exist                                                                          |
| MAT03 | AC2 | Add a build, bundle or transform cell                                                   | Served as authored; no Verter transform and no map chain                                                                     |
| MAT04 | AC2 | Add a TS plugin host cell                                                               | Reduced overlay admits no TS plugin delivery                                                                                 |
| MAT05 | AC2 | Add TS/JS-document hover or completion                                                  | tsgo/TS owns these answers; controller files admit additive locations and explicit assists only                              |
| MAT06 | AC2 | Add TS/JS-document rename or language diagnostics                                       | Language operations remain with tsgo; authored rename starts from HTML or the convention file transaction                    |
| MAT07 | AC2 | Add runtime/navigation/fetch/cache/morph execution                                      | Static facts cannot promise runtime integration                                                                              |
| MAT08 | AC2 | Promote an ERB cell without ERBH1 facts or before ERBT                                  | Overlay does not parse ERB and terminal delivery requires the shipped carrier                                                |
| MAT09 | AC2 | Remove an overlapping capability's COXD1 row or compute before step-down                | Each capability/position needs ownership gating and zero demands when stepped down                                           |
| MAT10 | AC2 | Mark planned cells as shipped or use syntax highlighting as product proof               | Only STIM9 passing fixture coverage and STIMT promotion establish delivery                                                   |
| MAT11 | AC2 | Add a second descriptor parser, selector parser or formatter                            | Codecs consume HWC facts; shared CSS and formatting owners remain authoritative                                              |
| ACT01 | AC3 | Activate from an importmap pin                                                          | Importmap pins are never read as activation evidence                                                                         |
| ACT02 | AC3 | Activate from a script/CDN URL or host                                                  | No resolved package or gem evidence; explicit on with exact release is required                                              |
| ACT03 | AC3 | Activate from `data-controller` or `data-turbo` spelling                                | Attribute spelling is not FWA1 evidence                                                                                      |
| ACT04 | AC3 | Derive the Turbo record/release/switch from Stimulus                                    | Two independent records are required, including gem-vendored projects                                                        |
| ACT05 | AC3 | Derive Stimulus from Turbo or collapse both into Hotwire                                | Reverse derivation and aggregate state violate the same independence rule                                                    |
| ACT06 | AC3 | Treat a declared package range as the installed version                                 | PM's resolved installed release is the admission authority                                                                   |
| ACT07 | AC3 | Use explicit on to admit an unsupported release, or do work while off                   | On cannot widen the lock; off requires zero work                                                                             |
| ACT08 | AC3 | Execute Gemfile, bundle, Ruby, config or a loader                                       | Detection is static data only; computed registrations retain incomplete facts                                                |
| FAC01 | AC4 | Invent a props facet from static values                                                 | Values are typed controller members, not component props                                                                     |
| FAC02 | AC4 | Invent an events facet from this.dispatch or turbo events                               | These are DOM-event facts                                                                                                    |
| FAC03 | AC4 | Invent slots from targets/outlets/templates                                             | DOM relations and HTML content are not component slots                                                                       |
| FAC04 | AC4 | Invent expose from controller members or add a FrameworkTag                             | Controller TypeInfo facts are not a component descriptor or carrier adapter                                                  |
| FAC05 | AC4 | Remove any facet disposition/reason or represent it as empty supported                  | All four explicitly do not exist; an empty supported facet fabricates a contract                                             |

## Route inventory and handoff

The pinned action parameter getter, binding event preparation, tokenizer, frame controller and document frame redirector linked in the vocabulary products own
these version-specific boundaries. The following are future codec/relation
verification obligations, not executed tests in this contract:

| Input | Required outcome | Receiving obligation |
| ----- | ---------------- | -------------------- |
| `<form data-controller="x" data-action="submit->x#save" data-x-count-param="7"><button data-x-count-param="9">Save</button></form>`, submitted by the button | `event.params.count` is the number `7`, read from the action-bearing form; the submitter's `9` is not used | STIM1 parameter codec/semantic verification |
| `<form data-controller="x" data-x-count-param="7"><button data-action="click->x#save" data-x-count-param="9">Save</button></form>`, clicked on the button | `event.params.count` is the number `9`, read from the action-bearing button; the ancestor form's `7` is not inherited | STIM1 parameter codec/semantic verification |
| `click->x#a click->x#b`, with U+0020 between descriptors | Two ordered action tokens with exact source spans | STIM1 descriptor codec verification |
| The same pair separated by U+00A0 (NBSP) or U+000B (vertical tab), including leading/trailing ECMAScript whitespace | The same two ordered tokens; an HTML ASCII-only splitter fails this boundary | STIM1 descriptor codec verification |
| `data-turbo-frame="_parent"` inside a frame with no ancestor frame | Reserved target; no missing-frame diagnostic or frame-id rename relation | STIM4T static frame relation verification |
| The same target inside a nested frame | Relation to the nearest ancestor frame, conditional on its disabled state; no runtime navigation | STIM4T static frame relation verification |
| A frame-origin link targeting `_parent`, with a literal `turbo-frame id="_parent"` elsewhere, with and without a real ancestor frame | Literal ID has no precedence in the current frame controller: resolve the ancestor or retain the no-ancestor disposition | STIM4T static frame relation verification |
| `<a href="/next" data-turbo-frame="_parent">` outside every frame, alongside an enabled document-level `<turbo-frame id="_parent">` | Document redirector selects the literal ID; retain its authored definition/reference/rename relation. The selected controller has no ancestor and falls back to that frame; no runtime navigation | STIM4T static frame relation verification |
| The same document-origin link with no literal `_parent` frame | Redirector selects no frame; no ancestor relation, missing reserved-target diagnostic or authored ID rename relation | STIM4T static frame relation verification |
| The same document-origin link with a disabled literal `_parent` frame | Redirector excludes the disabled frame; retain the disabled-state condition and do not claim an enabled target or ancestor relation | STIM4T static frame relation verification |
| The same document-origin link with an enabled literal `_parent` frame nested inside another frame | Redirector selects the literal ID; retain that authored ID relation separately from the selected controller's subsequent ancestor resolution | STIM4T static frame relation verification |
| An ordinary frame ID matching an authored frame | Preserve the ordinary authored ID relation | STIM4T static frame relation verification |
| `<turbo-frame id="messages" target="_top"><a href="/next" data-turbo-frame="_self">Next</a></turbo-frame>`, alongside an enabled `<turbo-frame id="_self">` | Literal `_self` takes precedence over the current frame; preserve its authored definition/reference/rename relation | STIM4T static frame relation verification |
| The same frame-origin link with no literal `_self` frame | Fall back to `messages` despite its `_top` target; retain the current-frame relation, with no missing-frame diagnostic or literal `_self` ID rename relation | STIM4T static frame relation verification |
| The same frame-origin link with a disabled literal `_self` frame | The literal target prevents interception; preserve its disabled-state condition and do not substitute `messages` as an enabled target | STIM4T static frame relation verification |
| `<a href="/next" data-turbo-frame="_self">` outside every frame, with and without an enabled literal `<turbo-frame id="_self">` | The document redirector selects the enabled literal ID and preserves its authored relation; without that literal frame it selects no frame, with no current-frame fallback | STIM4T static frame relation verification |

No existing production route is displaced by this contract; there is no legacy
deletion. HWC2 owns HTML, ERBH1 owns template facts, the shared CSS selector parser
owns selectors, FWA1 owns activation, TypeInfo owns generated members, tsgo owns
language answers, and LSO8 owns transactions. Planned producers are listed in the
matrix, including STIM5R and STIM8F; no stage 6 exists. FWA1's record key includes
the framework and its charter explicitly adds both `stimulus` and `turbo` gem
sources, so two independent records are expressible and the charter abort does
not apply. The stage-0 contract introduces no substitute state owner.
