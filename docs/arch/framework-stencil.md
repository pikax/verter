# Stencil static tooling contract

This page specifies planned Stencil tooling. The reviewed data in
[the contract manifest](../../tests/framework-stencil/STN0/manifest.json) and
[rejection cases](../../tests/framework-stencil/STN0/cases.md) do not claim shipped
support. No adapter, validator, compiler invocation or dependency change lands
with this contract. STN1 adds the lock validator under STN1-ACV; REG0 discovers it
without a root test script edit.

## Release and activation

[The version lock](../../tests/framework-stencil/STN0/products/stencil-version-lock.json)
admits resolved stable `@stencil/core` major 4. Its exact conformance reference is
4.45.2, with the [official npm artifact](https://registry.npmjs.org/@stencil%2fcore/4.45.2)
and published integrity. Reference pins agree across products. A supported user's
resolved stable 4.x patch may differ from this oracle patch. Floating pins,
`latest`, legacy majors, canary, dev, nightly and prereleases are excluded.
Major 5 is not admitted by this lock: a near-future announcement requires explicit
ratification of an exact reference release before admission.

[Activation](../../tests/framework-stencil/STN0/products/stencil-activation.json)
comes only from FWA1's resolved package role and file record. Stencil component
classes in `.tsx` carry decorators canonically resolved to `@stencil/core`.
Imported aliases and re-exports retain symbol identity; a local same-named
decorator does not qualify. A React, Preact, Solid or Qwik file is never claimed
as Stencil. Extension, package presence or decorator spelling alone is
insufficient. `frameworks.stencil = off` does zero work; `on` cannot admit an
excluded version. No user code or config is executed to discover identity.

## Delivery and ownership

[The capability matrix](../../tests/framework-stencil/STN0/products/stencil-capability-matrix.json)
binds each operation/host/profile to one producer and receiving acceptance.
[The host table](../../tests/framework-stencil/STN0/products/stencil-host-table.json)
records tsgo operations and limitations per cell. All editor cells are
`lsp-enhancement` on `stencil-tsx`, through LSPX11. The shared kernel remains the
sole owner of parsing, resolution, types, indexing, authored maps, occurrences,
edits, lint, format and public envelopes. Stencil contributes versioned facts;
there is no new parser, type cache or parallel edit path.

| Operation                                 | Producer / receiving acceptance | Source of added result                                   |
| ----------------------------------------- | ------------------------------- | -------------------------------------------------------- |
| Custom-element tag hover section          | STN5 / STN5-AC1                 | Attributes, events, slots and encapsulation              |
| Extra tag definition                      | STN5 / STN5-AC2                 | Component class, never the generated interface           |
| Watch/listen literal definitions          | STN5 / STN5-AC3                 | Canonically linked prop/event                            |
| Verter diagnostics and fixes              | STN8 / STN8-AC3                 | Framework diagnostics; never edit generated declarations |
| Neutral standards/CEM facts for consumers | HWC3 / HWC3-AC2                 | STN3-owned evidence projected by HWC3                    |

Verter never re-provides ordinary TS hover, completion, definition, references,
symbols or rename. There are no TS-plugin cells. LSPX11 checks position ownership
against the tsgo probe, COXD2 and the user's capability setting before work;
Unknown means Verter owns. No follow-up is stacked on another tool's result.
Client result-merging compatibility remains LSPX11's verification obligation.

STN5R-AC1 owns watch/listen literal and dash-cased JSX attribute rename as one
LSO8 transaction, only at uncovered framework positions. At TS member positions
tsgo's rename is final, with no follow-up. Collisions, explicit attribute
overrides, decorator names and node_modules symbols are refused. Generated
declarations stay untouched with regenerate-required (STN5R-AC3).

tsgo supplies type/symbol-at-position, display strings, diagnostics and source
files. When TypeInfo refuses an EventEmitter payload and tsgo cannot supply the
type argument, retain `Unavailable(tsgoLimitation: typeArguments)` and notify
`$/verter/tsgoLimitation` once per operation/project. Never parse `typeToString`.

## Facets and standards handoff

The [official Component API](https://stenciljs.com/docs/api) describes the
decorator metadata. [Render JSX slots](https://stenciljs.com/docs/templating-jsx)
supply the slot evidence.

| Facet          | Authoritative evidence                             | Projection owner          |
| -------------- | -------------------------------------------------- | ------------------------- |
| Props          | Canonical @Prop; attribute/reflect/mutable binding | STN2 binds; STN3 projects |
| Emits          | Canonical @Event and EventEmitter payload          | STN3                      |
| Slots          | Literal slot elements/names in render JSX          | STN3                      |
| Expose         | Canonical @Method                                  | STN3                      |
| Model, Options | UNSUPPORTED                                        | No mapped facet           |

Authoritative provenance does not make an unresolved member type exact.
Approximate, conditional, unknown, unavailable, refusal, partial and cancelled
facts retain their status. A computed tag or option is unknown, never guessed
from a class name. `FRAMEWORK_TAG_STENCIL = 8` lands with STN1's registered,
carrier-less adapter; `OPEN_CANONICAL` cannot identify it.

STN3-AC2 hands HWC3 neutral tag, attribute, event, slot and method facts with
scope, presence, completeness and Stencil provenance. HWC3-AC2 owns standards
projection and CEM import/export. It neither interprets decorators nor recovers
EventEmitter types. Stencil emits no CEM file. This is a data projection boundary,
not a claim that CEM export is an LSP request. HWC3's acceptance explicitly
allows framework-owned evidence without framework semantics; the handoff needs
no additional Stencil-specific authority there.

## Static inputs and exclusions

STN1 owns component identity/options. STN2 owns canonical member bindings,
attribute names and watch/listen links. STN3 owns facets and HWC evidence. These
are fact producers used by the editor cells, not separate editor hosts.

The Stencil compiler, output-target builds, hydrate builds and runtime are
excluded. Checked-in `src/components.d.ts` is an ordinary type input: stale or
missing declarations are reported, never regenerated. STN2 takes `outputTargets`
from CENV1C's static configuration capture, keeping distinct distribution target
registration/hydration identities. Computed targets remain unknown. Static facts
never establish actual runtime registration, hydration or global reachability.

Legacy deletion is not applicable: the baseline has no Stencil adapter/tag or
Stencil-owned route to displace. The cases table inventories every rejection
required of the later validator; no acceptance rests on a Git object, stored
transcript, result count, syntax colouring or architecture proof alone.
