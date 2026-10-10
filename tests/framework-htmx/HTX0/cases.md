# htmx lock acceptance cases

These are reviewed case definitions, not executed tests. HTX1-ACV implements
all clean controls and planted twins against these products. The command
`node --test tests/framework-htmx/HTX0/htmx-lock.spec.ts` belongs to HTX1;
no spec or validator is present here. REG0 discovers it without a root script edit.

## HTX0-AC1: pinned profiles

Clean control: both exact profiles retain their separate sources, the matrix
has one producer and receiving acceptance per admitted cell, all facets are
absent with reasons, and activation follows resolved/named release evidence.
This control must pass the future validator.

| Planted row                              | Expected failure reason                                         |
| ---------------------------------------- | --------------------------------------------------------------- |
| floating-pin                             | A range is not an exact release.                                |
| dist-tag-pin                             | latest/next is selection metadata, never a pin.                 |
| canary-nightly-pin                       | Only the two ratified releases are admitted.                    |
| legacy-1-profile                         | 1.x is excluded.                                                |
| diverged-pin                             | Version, artifact integrity and vocabulary release must agree.  |
| missing-source                           | Every profile needs its own exact official artifact source.     |
| shared-vocabulary-without-profile-source | Shared spelling does not supply provenance for another profile. |
| shared-inheritance-or-events             | Each major owns independent rules and event names.              |
| two-releases-one-profile                 | Each manifest profile has one exact release.                    |

Exact-source controls for HTX1-ACV (static authored syntax only):

| Clean control | Planted twin | Expected failure reason |
| --- | --- | --- |
| 4.0.0 `<div hx-history-elt>` is a local presence marker. | four-history-marker-inherited | `<div hx-history-elt:inherited="true">` is not a history marker; a descendant does not inherit the ancestor's bare marker. |
| 4.0.0 `<div hx-swap-oob="outerHTML">` supplies its own authored swap specification. | four-swap-oob-inherited | `<div hx-swap-oob:inherited="outerHTML">` is not an out-of-band marker; an ancestor's bare marker does not supply a descendant value. |
| 2.0.11 `hx-on:click="doA()"` and `hx-on-click="doA()"` are recognized; `hx-on::before-request` and `hx-on--before-request` abbreviate htmx events. | two-unsuffixed-on | Both `hx-on="click: doA()"` and `hx-on="click -> doA()"` are ignored by this pin. |
| 4.0.0 `hx-on:click="doA()"` and `hx-on::before:request="doA()"` are recognized under the default colon meta character. | four-dash-on-default | `hx-on-click="doA()"` is ignored by default; a computed meta character cannot establish support. |
| 4.0.0 `hx-on="click once -> doA(); blur -> doB()"` attaches event specifications and unevaluated JavaScript regions. | four-arrow-on-missing | Omitting the bare arrow grammar rejects supported 4.x authored syntax. |
| 4.0.0 `hx-status:404="target:#errors"`, `hx-status:40x="swap:none"`, and `hx-status:4xx='{"target":"#errors"}'` are independently claimed HCON/JSON configuration regions. | four-status-family-missing | The core family must be inventoried with the exact pin, selector decoding and explicit inheritance; `hx-status:404:inherited="target:#errors"` supplies authored descendant configuration, unlike the structural markers. |
| 2.0.11 leaves `hx-status:404="target:#errors"` plain HTML. | two-status-family | A 4.x family does not supply a 2.x claim or inheritance rule. |
| 4.0.0 admits only three digits, two digits plus `x`, or one digit plus `xx` after `hx-status:`. | four-status-invalid-suffix | Bare `hx-status`, `hx-status:xxx`, and `hx-status:404x` are not core status patterns. |

These controls compare authored claims and regions, never handler execution,
history restoration, response observation or live swaps. The sources are the
integrity-pinned `dist/htmx.js` functions named in the vocabulary entries.

## HTX0-AC2: owned matrix

Clean control: both exact profiles retain their separate sources, the matrix
has one producer and receiving acceptance per admitted cell, all facets are
absent with reasons, and activation follows resolved/named release evidence.
This control must pass the future validator.

| Planted row                 | Expected failure reason                                             |
| --------------------------- | ------------------------------------------------------------------- |
| unowned-cell                | Every required-planned cell has one producer.                       |
| missing-or-wrong-acceptance | A cell names one acceptance of its producer.                        |
| cancelled-HTX3              | No type projection producer exists.                                 |
| cancelled-HTX5R             | HTX5 contributes occurrences; no separate rename plan.              |
| cancelled-HTX6              | No build producer exists.                                           |
| build-support               | Served as authored; no transform or map chain.                      |
| typed-endpoint-without-HRF  | Only HRF1/HRF2 with OAPI3 identities owns request/response linkage. |
| URL-as-endpoint             | A literal URL alone gives no server operation.                      |
| exclusion-without-reason    | Exclusions must explain the unavailable operation.                  |
| fabricated-four-hx-ext      | Pinned 4.0.0 has registerExtension, no defineExtension/hx-ext.      |

## HTX0-AC3: facets absent and no runtime

Clean control: both exact profiles retain their separate sources, the matrix
has one producer and receiving acceptance per admitted cell, all facets are
absent with reasons, and activation follows resolved/named release evidence.
This control must pass the future validator.

| Planted row                   | Expected failure reason                                        |
| ----------------------------- | -------------------------------------------------------------- |
| hx-vals-as-props              | Request data does not create a component.                      |
| DOM-events-as-facet           | htmx:\* names are DOM-event facts, not component events.       |
| slots-or-expose-facet         | No component surface exists.                                   |
| missing-facet-reason          | Each of the four facets has an explicit does-not-exist reason. |
| request-class-fact            | htmx-request is a runtime effect, excluded.                    |
| history-cache-fact            | Live history state needs runtime observation, excluded.        |
| swap-settle-phase-fact        | Decode authored swap syntax only; no runtime phase fact.       |
| TS-plugin-cell                | Delivery is LSP enhancement only.                              |
| tsgo-reprovided-cell          | Existing TS answers stay with tsgo.                            |
| registration-hover-completion | JS/TS registration rows add only locations.                    |
| invented-wire-tag             | HTML attribute overlays have no FrameworkTag.                  |

## HTX0-AC4: activation

Clean control: both exact profiles retain their separate sources, the matrix
has one producer and receiving acceptance per admitted cell, all facets are
absent with reasons, and activation follows resolved/named release evidence.
This control must pass the future validator.

| Planted row                    | Expected failure reason                                               |
| ------------------------------ | --------------------------------------------------------------------- |
| CDN-URL-activation             | Script URLs do not activate.                                          |
| hx-spelling-activation         | hx-/data-hx- attributes alone do not activate.                        |
| importmap-activation           | Importmaps are not admitted evidence.                                 |
| profile-without-resolved-major | Auto reads exact PM resolution and its major, never a declared range. |
| on-without-named-release       | Non-npm on must name one admitted release and therefore its profile.  |
| on-unadmitted-release          | Explicit on never overrides version admission.                        |
| off-still-produces-facts       | off disables the entire vertical with zero work.                      |
| inactive-changes-neutral-HTML  | HWC facts stay unchanged and attributes remain plain HTML.            |

No existing htmx validator covers these rows. They are not applicable to
execution in this docs-only change: the frozen amendment transfers every
rejection proof to HTX1-ACV. HWC2 counterfixtures establish no product support.
