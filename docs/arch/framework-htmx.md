# htmx delivery and exact-version contract

This reviewed contract fixes the htmx attribute overlay's releases,
activation, vocabulary, producers and delivery boundaries. It adds no
production code, executable validator, test, schema enforcement or CI gate.
It establishes no shipped support: the architecture capability inventory's
`htmx-4-target` remains `required-planned`. HTML counterfixtures are parser
evidence, not htmx product qualification.

## Contract products

The data lives in `tests/framework-htmx/HTX0/`:

| File                                   | Contract                                                                                          |
| -------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `manifest.json`                        | Four acceptance groups, planted twins, and HTX1's validator handoff                               |
| `products/htmx-version-lock.json`      | Exact releases, registry integrity, licences and static feature sources                           |
| `products/htmx-vocabulary.json`        | Independent attributes, inheritance, trigger/swap grammars, DOM events and extensions per profile |
| `products/htmx-activation-policy.json` | FWA1's package dependency and explicit-on row                                                     |
| `products/htmx-capability-matrix.json` | Operation × host × profile cells, one producer and acceptance per cell, and exclusions            |
| `cases.md`                             | Clean controls and every planted row with its expected rejection reason                           |

REG0, FWA1 and FCH1 consume these products as data. HTX1-ACV adds
`contract.ts` and `htmx-lock.spec.ts` here and implements every planted twin.
REG0's discovery runner finds that spec; this change edits no root script.
Until HTX1 lands, the products have reviewed case definitions rather than
executable rejection coverage. The future lock command is
`node --test tests/framework-htmx/HTX0/htmx-lock.spec.ts`.

The docs-only contract tree is explicitly CI-inert in `scripts/ci-impact.mjs`:
no current CI lane executes its validation. When HTX1-ACV lands the validator,
it must assign the validator and every consumed contract product to the
executing lane's `PATH_FILTERS`; this inert classification does not replace
that ownership. This follows the ratified `htmx-contract-ci-classification`
scope exception and adds no gate here.

## Profiles and provenance

| Profile  | Exact reference release | Registry selection at ratification | Inheritance                                                    |
| -------- | ----------------------- | ---------------------------------- | -------------------------------------------------------------- |
| `htmx-2` | `htmx.org` 2.0.11       | `latest`                           | Implicit only for the profile's inheritable attributes         |
| `htmx-4` | `htmx.org` 4.0.0        | `next`                             | Explicit `:inherited`; `:append` retains its own authored part |

The [npm registry](https://registry.npmjs.org/htmx.org) supplied these exact
releases on 2026-10-10. The [4.0.0 release announcement](https://four.htmx.org/announcements/2026-08-28-htmx-4.0.0-is-released)
explains why 2.x remains `latest` while released 4.x remains `next`. Neither
registry tag is an admission input. This lock admits only the listed exact
releases; 1.x, prereleases and unlisted releases are unsupported. A later
release requires a reviewed lock update, never reuse of another release's
tables. Profiles retain their exact release identity under VID0.

The vocabulary's authority is each published tarball's `dist/htmx.js`,
joined to its integrity pin, declarations and BSD-0-Clause licence. Official
[2.x reference](https://htmx.org/reference/) and
[4.x reference](https://four.htmx.org/reference/) explain the tables; live
web pages do not replace pinned bytes. In particular, 4.0.0 uses HCON for
trigger modifiers and swap configuration, has different event names, and
supports `innerMorph`/`outerMorph`. A 2.x rule never supplies a 4.x answer.
Script filters attach through EMB0 and stay unevaluated. Template holes
retain unknown parts and cannot produce complete negative answers.

The handler grammars also differ: 2.0.11 recognizes event-suffixed colon and
dash forms but ignores bare `hx-on`. 4.0.0 admits bare `hx-on="click -> code"`
and event suffixes using the configured meta character (colon by default);
it ignores dash suffixes under that default. Its `hx-status:<status>` family
accepts exact three-digit codes and `40x`/`4xx`-style patterns with authored
HCON/JSON response configuration, including selector regions. These values
use explicit inheritance. In contrast, `hx-history-elt` and `hx-swap-oob`
are local structural markers: neither their `:inherited` spellings nor an
ancestor marker supplies an effective descendant value. This records static
syntax, without observing handlers, history, responses or swaps.

The 2.x core supports authored `defineExtension` names and `hx-ext` uses,
but separately distributed extension implementations have no admitted pin
here. Their semantics are unsupported. The 4.0.0 tarball bundles extensions
under `dist/ext/`; each listed extension has that same exact source pin.
Its attributes are conditional on a statically resolved authored extension
registration/import. Computed registrations and implementations stay unknown.
No extension is loaded or executed by Verter.

The published 4.0.0 core exports `registerExtension`, with extension-owned
attributes rather than `defineExtension`/`hx-ext`. The latter operation is
therefore admitted only for 2.x. The operator ruling
`htmx-four-extension-registration` retains this operation for 2.x and
truthfully excludes it for 4.0.0; replacement registration navigation would
require successor-charter ratification. Both profiles keep all other admitted
cells.

HTX9 uses the pinned official grammar and reads upstream release-tag HTML
fixtures as static feature inputs. It vendors selected fixtures with their
licence before hermetic checks. It never executes the upstream test runner,
htmx, a server, or a request. There is no browser-runtime oracle here.

## Activation and ownership

FWA1's `FrameworkActivation` is the only activation authority. With `auto`,
the package must resolve `htmx.org` in `dependencies` through PM facts.
The resolved major selects the profile and the exact release must pass the
lock. A declared version range, private transitive dependency, script URL,
CDN host, importmap, HTML extension or `hx-` spelling activates nothing.

A non-npm server workspace uses CFG0's declarative shape
`frameworks.htmx = { state: on, release: <exact admitted release> }`.
Naming 2.0.11 or 4.0.0 uniquely names the profile that FWA1 carries in `mode`.
`off` disables the whole vertical; explicit `on` cannot admit an unsupported
release. The switch outranks automatic evidence. Conflicting exact release
claims need explicit selection rather than an order or nesting guess.

FWA1's Switch contract explicitly permits a non-manifest `on` with an
admitted release; its record contains both `admittedRelease` and `mode`,
and FWA1-AC6 owns that activation check. The charter's missing-profile abort
does not apply. This is predecessor-contract evidence, not a claim that
FWA1 has shipped.

HWC2 owns neutral HTML parsing and facts; PAR0 admits no htmx parser.
HTX1 claims active attributes and attaches authored regions through EMB0.
CSS3 owns selectors; HTX2 binds them over HWC facts and records literal
request identities. HTX4 supplies effective inheritance, defaults and
response-overridable facts. HRF1 and HRF2, using OAPI3 identities, own
request/response and fragment linkage. A URL alone is never a typed endpoint.

## Hosts, facets and exclusions

All editor cells use the Verter LSP. HTML cells use HWCI0's `html` selector,
capability masks and LSPX11 ownership seam. HTX5 adds authored navigation,
hover, completion and LSO4 occurrence roles. HTML id/class rename remains
HWCI0/LSO8-owned; HTX5 builds no edit plan. Attribute names, URL text,
trigger/swap keywords and DOM event names are not renameable occurrences.

Only the 2.x registration-reference cell also uses `javascript`/`typescript`:
it adds HTML `hx-ext` uses beside tsgo's references. It gives no registration
hover or completion and never re-provides TS symbols, types or references.
No htmx cell needs a tsgo type operation. Computed registration identity is
unknown; `typeToString` is never parsed. Ownership checks apply before work,
and inactive or stepped-down capabilities perform zero work. Standard HTML
tags, attributes and emmet stay with the editor/HTML owner.

All four component facets—props, events, slots and expose—are recorded as
`does-not-exist` with reasons. htmx has no component; `hx-vals` is request
data. `htmx:*` events are DOM-event facts, not component events. There is no
descriptor, framework wire tag or new HTML carrier. HTX7 records static
keyboard activation and authored live regions, supplies `NoStyleRecords`
to CSS7, and supplies no root, slot or fallthrough records to AX5.

Authored HTML is served as authored: no Verter transform and no map chain.
Verter never executes, hosts or observes htmx, issues requests, inspects
live responses, or models request classes, swaps, settle phases or history
cache state. HTX8 owns profile-scoped lint integration and LSO8 fixes;
HTX8F uses FMTH0 and preserves every claimed attribute value byte for byte.
`packages/typescript-plugin` gets no htmx work.

## Delivery and acceptance

The retained stages are HTX0, HTX1, HTX2, HTX4, HTX5, HTX7, HTX8, HTX8F,
HTX9 and HTX10. HTX3 is cancelled because there are no types to project;
HTX5R is cancelled because HTX5 supplies occurrences to neutral rename;
HTX6 is cancelled because there is no build. No matrix cell names these
cancelled producers.

Each matrix cell is `required-planned`, has one producer and receiving
acceptance, and is qualified per profile by HTX9. Excluded cells carry
concrete reasons. HTX9 joins coverage, grammar agreement, incremental/fresh
equivalence, edit/revert, cancellation and browser/native/HTML-editor
exposure. Work/performance evidence is advisory. HTX10 alone promotes support
claims after qualification; HRF2 and NL-HTMX/LHX consumers use these same
profile identities rather than inventing htmx component types.

The four acceptance groups are satisfied here by reviewed products and
`cases.md`: pins and sources (AC1), owned cells and exclusions (AC2), absent
facets and runtime exclusions (AC3), and activation evidence (AC4). Every
floating/shared/cancelled/build/runtime/TS-plugin/false-activation planted
row remains a required HTX1-ACV rejection proof. No proof has been waived.
