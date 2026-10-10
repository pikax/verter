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
