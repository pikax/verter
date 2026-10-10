# Stimulus and static Turbo delivery contract

This page ratifies an HTML attribute overlay, not a compiler or runtime. The
machine-readable contract is in
[`tests/framework-stimulus/STIM0/manifest.json`](../../tests/framework-stimulus/STIM0/manifest.json),
with five products and a [reviewed case table](../../tests/framework-stimulus/STIM0/cases.md).
These are planned capabilities. The descriptor-codec implementation owns the
future lock validator; this documentation adds no executable test or CI gate.

## Exact releases and activation

| Framework profile        | Exact npm release                                                         | Admitted gem → vendored JS release      |
| ------------------------ | ------------------------------------------------------------------------- | --------------------------------------- |
| Stimulus                 | `@hotwired/stimulus` 3.2.2                                                | `stimulus-rails` 1.3.4 → Stimulus 3.2.2 |
| Turbo, static facts only | `@hotwired/turbo` 8.0.23; optional `@hotwired/turbo-rails` 8.0.23 wrapper | `turbo-rails` 2.0.23 → Turbo 8.0.23     |

The release sources are the tagged official
[Stimulus package](https://github.com/hotwired/stimulus/blob/v3.2.2/package.json),
[Turbo package](https://github.com/hotwired/turbo/blob/v8.0.23/package.json) and
[Turbo Rails package](https://github.com/hotwired/turbo-rails/blob/v2.0.23/package.json).
Gem mappings use the actual vendored asset headers:
[Stimulus Rails](https://github.com/hotwired/stimulus-rails/blob/v1.3.4/app/assets/javascripts/stimulus.js)
and [Turbo Rails](https://github.com/hotwired/turbo-rails/blob/v2.0.23/app/assets/javascripts/turbo.js),
joined to the gem version files listed in the lock. A gem version or npm
dependency range alone cannot prove which JavaScript release the asset vendors.

The 3.2.x and 8.0.x labels describe lines; they are not admission ranges. Each
profile identifies one exact release. Unlisted patches, gem releases, legacy
majors, prereleases and next majors are unsupported. No next major is ratified.
The vendored table exhausts admitted gems, not every historical Rails release.
New releases require separate sourced rows and a vocabulary review.

FWA1 is the sole activation owner. It publishes independent `stimulus` and
`turbo` records per package and file, each with state, admitted release and
provenance. Automatic evidence is PM's resolved installed package identity and
release, or package-root `Gemfile.lock` resolved specs joined to the exact
vendored table. Dependency roles include dependencies, dev, peer and optional
dependencies when actually resolved. The Turbo Rails npm wrapper must resolve
its installed core dependency; its declared range is not the core release.

`frameworks.stimulus` and `frameworks.turbo` independently select `auto`, `on` or
`off`. With no dependency evidence, explicit on must name the admitted release;
auto remains inactive. On cannot override unsupported installed evidence. Off
does zero overlay work. A pair of gems yields two records, never one inferred
Hotwire state. Conflicting release evidence is retained and refused until an
unambiguous exact release is established. Importmap pins, CDN/script URLs and
attribute spellings never activate either record. Ruby, bundle, loaders and user
config are never executed.

## Facts, hosts and ownership

The pinned Stimulus vocabulary records controller/action/target/value/class/
outlet/parameter attribute families, descriptor grammar and source spans, options,
key filters, default events and the five value types. Tagged implementation
sources govern version-specific behavior; the official reference supplies
examples. Dotted non-keyboard events remain event names. Custom schemas, loader
names or declarations needing evaluation remain unknown or incomplete.
Action parameters come from the element carrying the action descriptor, as the
pinned [parameter getter](https://raw.githubusercontent.com/hotwired/stimulus/v3.2.2/src/core/action.ts)
and [event preparation](https://raw.githubusercontent.com/hotwired/stimulus/v3.2.2/src/core/binding.ts)
establish. A form submit action reads the form's parameters; a button click
action reads the button's. The case table hands this pair to STIM1.

Turbo contributes frame, stream and stream-source declarations; the eight stream
actions; frame-id relations; `data-turbo*` attributes and `turbo-*` meta facts.
`_top`, `_self` and `_parent` are frame keywords. The pinned
[frame controller](https://raw.githubusercontent.com/hotwired/turbo/v8.0.23/src/core/frames/frame_controller.js)
looks up a literal `turbo-frame id="_self"` before falling back to the current
frame. An enabled literal target preserves its authored definition/reference/
rename relation; without a literal frame, `_self` selects the current frame even when that
frame has `target="_top"`. A disabled literal target prevents interception
instead of falling back. For document-origin navigation, `_self` selects only
an enabled literal frame; without one there is no current-frame fallback.
The case table hands these controls to STIM4T. In the pinned release, `_parent`
uses ancestor lookup for frame-origin navigation. Without an ancestor, the
current frame controller does not intercept; this is not a missing frame-id
relation. For document-origin navigation, the pinned
[frame redirector](https://raw.githubusercontent.com/hotwired/turbo/v8.0.23/src/core/frames/frame_redirector.js)
can select an enabled literal `turbo-frame id="_parent"`; preserve its authored
definition/reference/rename relation. The selected controller then resolves its
ancestor, falling back to the selected frame when no ancestor exists. An absent
or disabled literal target is not selected by the redirector. The case table
hands both origins and their controls to STIM4T.
Stimulus action lists use ECMAScript whitespace and line terminators,
including NBSP and vertical tab, retaining ordered tokens and source spans.
Selectors go to the shared selector parser.
No navigation, fetch, stream execution, morphing or cache behavior is simulated.
Attributes set by Turbo at runtime are vocabulary observations, not claims that
Verter observes live runtime state.

| Host                              | Admitted delivery                                                                                                             | Shared authority and restriction                                                                                       |
| --------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| HTML                              | Framework completion, hover, definition, references, authored rename, diagnostics and assists; Stimulus descriptor formatting | HWCI0 `html` selector and capability masks over HWC2; no standard tags, attributes or emmet duplication                |
| ERB-produced HTML                 | Same overlay operations over literal/partial HWC attributes                                                                   | ERBH1 facts via ERB2 and templated-fact producer; no template parse or island reads; terminal promotion waits for ERBT |
| JavaScript/TypeScript controllers | Additional framework definition/reference locations; TypeScript declare-members assist                                        | LSPX11 additions only; no TS/JS hover, completion, rename or language diagnostics; tsgo's locations are not duplicated |
| TypeScript plugin                 | Excluded                                                                                                                      | This reduced overlay has no plugin cell                                                                                |
| Build/runtime                     | Excluded                                                                                                                      | Served as authored; no Verter transform and no map chain                                                               |

The [capability matrix](../../tests/framework-stimulus/STIM0/products/stimulus-capability-matrix.json)
expands each operation × selector × profile cell, with exactly one producer,
receiving acceptance, tsgo requirements and exclusion rationale. Missing type
operations produce `Unavailable(tsgoLimitation: op)` and
`$/verter/tsgoLimitation`; nobody parses `typeToString`. Controller generated
members are TypeInfo facts, never injected declarations in the tsgo program.
Explicitly requested declare-member assists use the shared edit transaction path.

Props, events, slots and expose all **do not exist**, with individual reasons in
the matrix. Static values are members, targets/outlets are DOM relations and
`this.dispatch` produces DOM-event facts. A controller is not a component; there
is no descriptor surface or `FrameworkTag`.

Template holes retain their unknown tokens. Opaque Ruby-built attributes and
frame helpers make scope/populations uncertain. Missing-controller/frame findings
require complete evidence. A rename or assist crossing a hole refuses the entire
transaction; it never edits only the known fragments.

## Delivery and coexistence

The stage set is STIM1–STIM5, STIM5R, STIM7–STIM9 and STIMT, plus STIM1E for
templated facts, STIM4T for static Turbo, STIM8F for shared formatting and STIMA for
assists. There is no stage 6. The matrix names each producer and its acceptance;
STIM9 supplies per-cell conformance/incremental coverage, and STIMT promotes only
those covered cells. `stimulus.controllers` and all seven assist descriptors are
explicit DX1/MCP receiving obligations, not currently registered tools.

Each potentially overlapping editor cell has a COXD1 Stimulus LSP entry, resolved
by capability and position. LSPX11 probes tsgo, then COXD2, then the user's
per-capability choice; Unknown means Verter owns. Step-down happens before demand
or compute, with no stacked follow-up. Extension presence alone is not proof it
implements every capability. Another enabled capability on the same HWC facts
continues independently. Turbo is gated by its own activation even where the
editor operation is shared with Stimulus.

This contract displaces no production route, parser, formatter, cache or state
owner. It makes no timing, allocation or runtime correctness claim. Qualification
compares answer classes and work growth, retaining timing evidence as advisory.
