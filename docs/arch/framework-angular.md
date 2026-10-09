# Angular delivery and exact-version contract

This is the Angular stage-0 contract: the exact release Verter supports, how
Angular is switched on, which parser each template sublanguage gets, which host
answers each editor operation, how components map onto the TypeInfo facets,
and the wire tag. Every later Angular node, the `lint.angular` packs and the
Angular semantic-facts successors build against it.

It describes the repository at `docs(arch): define the TypeInfo selector,
descriptor and (#815)`, 2026-10-09. It is contract text only: it changes no
production code and adds no check (see the docs-only rule in
[kernel/README.md](kernel/README.md)).

## Machine-readable products

The reviewed data lives in `tests/framework-angular/ANG0/`:

| File | Holds |
| ---- | ----- |
| `manifest.json` | the products, the acceptance IDs, the validator's owner and the consumers |
| `products/angular-version-lock.json` | the admitted release with sources and integrity, excluded releases and modes, the diverged pin inventory, file associations, the wire tag and the oracles |
| `products/angular-capability-matrix.json` | parser decisions, hosts, the facet mapping, every operation cell with its producer and receiving acceptance, and the exclusions |
| `products/angular-activation-policy.json` | the FWA1 activation row, zero-work rule, `.html` neutrality and the host policy |
| `cases.md` | every planted row the validator must reject, with its reason |

The validator (`contract.ts` and `angular-lock.spec.ts`) is not here. ANG1G
writes it (`ANG1G-ACV`) and `scripts/run-framework-locks.mjs` (`REG0-AC6`) runs
it. Until then the products are read as data by FWA1, REG0 and FCH1 and are not
CI-checked. ANG1M adds `products/angular-displaced-routes.json` (`ANG1M-AC6`).

## Release

Admitted: `@angular/core`, `@angular/compiler-cli` and `@angular/compiler`
**22.2.1** (published 2026-09-30, MIT), as the single release
`angular@22.2.1` with the profile `angular-22.2.1-standalone`. The lock records
each package's registry source and the integrity of the two release packages.

A release is exact (identities `R04`–`R08`). Any installed version other than
22.2.1 is `unsupported-version`, including 22.2.2 (published 2026-10-08) and
the 22.3.0 next builds; each needs a re-pin of the lock.

**Angular 23 next/rc is excluded.** On 2026-10-09 the npm registry lists no
23.x version of `@angular/core` or `@angular/compiler-cli`; the `next` dist-tag
points at 22.3.0-next.1. There is no exact version to pin, and pinning the
channel would be a floating tag. A later lock amendment that names an exact
published 23.0.0 next or rc version admits it as a second profile. LNG1 names a
23 channel too; lint packs run only where FWA1 admits a release from this lock,
so that channel stays inactive until then.

Excluded modes:

- **NgModule scope.** A component declared in `@NgModule({ declarations })`, or
  any `declarations` array, is a legacy mode (ruling 2). It gets a typed
  unsupported-profile result, never a guessed scope (`ANG2-AC5`, `ANG5-AC2`).
- **`*ngIf`/`*ngFor`/`*ngSwitch`.** Superseded by built-in control flow. They
  are a typed unsupported-profile reject (`ANG1-AC3`). The official migration is
  `ng generate @angular/core:control-flow`; Verter never rewrites it
  (`ANG8-AC3`). A custom `*dir` microsyntax such as `*appRepeat="let x of xs"`
  stays admitted.

The WDX1 fixture `tests/web-product/WDX1/fixtures/mixed-framework/case.json`
pins `angular 20.0.0`. That is a diverged pin. It is inventoried in the lock,
is never evidence for any cell, and ANG9 re-pins it to 22.2.1.

Test-only oracles: `@angular/compiler-cli` 22.2.1 (`ngc` `strictTemplates`) and
`@angular/language-service` 22.2.1 (type answers), both owned by ANG9. Prettier
is not admitted as an oracle, because it is not an official Angular source.
Feature sources are the `angular/angular` `v22.2.1` compiler and
compiler-cli test files and the angular.dev guide pages the lock lists (MIT).

## Activation

FWA1's `FrameworkActivation` record is the only activation source. Angular is
active for a workspace package only when FWA1 resolves `@angular/core` there
from the installed version and that version is the admitted release.
`frameworks.angular = auto | on | off` follows framework-common: `off` disables
the vertical, and `on` never admits an unadmitted release. FWA1's record is
keyed by package, so the abort condition ("FWA1 cannot express a per-package
`@angular/core` activation") does not apply.

Inactive, `unsupported-version` and `off` mean zero Angular work for the
package: no parse, attachment, fact, projection, navigation, lint-host or
format work (`ANG1M-AC5`, `ANG1S-AC5`, `ANG8-AC1`).

Not activation sources: LK6 (it only selects lint packs where Angular is
already active), the ANG1G TextMate grammars, `@angular/language-service`,
`angular.json` and similar config files, file extensions, and name matches.

A generic `.html` file stays neutral HTML until a resolved `templateUrl` in an
active package associates it with a component (ANGP findings, consumed at
ANG1M). Only then does the FWA1-gated `LanguageRow::gated` row classify it as
`FileLanguage::FrameworkTemplate`. No new language id is added: external
templates stay `html` and components stay `typescript`.

## Parser decisions (PAR0 `CL12`)

| Sublanguage | Decision | Home |
| ----------- | -------- | ---- |
| Template markup: elements, attributes, bindings, blocks, ICU, `ng-template`/`ng-container`/`ng-content` | ForkAndSpecialize of the `CL10` neutral HTML tokenizer lineage | `H02` `crates/verter_angular_syntax` (ANG1) |
| Binding and interpolation expressions, event statements, pipes, `*dir` microsyntax | NewParser: a dedicated Angular expression grammar | `H02` (ANG1) |
| Component TypeScript host | Reuse OXC (`CL01`) | OXC choke point |
| Component styles | Reuse `verter_css_syntax` (`CL04`), SCSS/Less as admitted dialects | `verter_css_syntax` |
| Unassociated `.html` | Reuse neutral HTML (`CL10`) | `H08` |

The same grammar serves inline and external templates. The neutral HTML parser
gains no Angular branch: the fork lives in `H02`.

The expressions get a dedicated grammar, not an OXC specialisation, because
the Angular expression language is not an ECMAScript subset OXC can parse. `|`
is the pipe operator with `:` arguments and every bitwise operator is
unsupported. Declarations, arrow functions, `new` and the comma operator are
rejected. Event bindings are `;`-separated statements, and microsyntax and the
`$any`/`$safeNavigationMigration` forms are Angular-only. Type answers still
come from tsgo over the ANG6 companion, so the grammar holds no TS semantics.

## Hosts (ruling 1)

- **TS regions** (expressions in inline and external templates) are answered by
  tsgo through the Verter LSP's `TsgoCompositeProvider`: shared tsgo
  (`TsgoSharedProvider`) first, managed tsgo (`TsgoOwnedBackend` over
  `crates/verter_tsgo_api`) second. The route runs over the ANG6 companion,
  project-bound through `ProjectBinding`/`BoundProject`, and maps back through
  `ProviderPositionMapper`; generated-only spans are suppressed. Verter never
  computes a TS-region answer itself.
- **Carrier-only operations** (selector navigation and rename, alias rename,
  carrier completion, external-template structure) are Verter LSP
  enhancements. They step down where LSPX11's `owns(capability, position)` is
  false.
- **External templates** reach the Verter LSP through a document-selector row
  gated by the FWA1 capability bit (ANG6).
- **`@verter/typescript-plugin`** serves the companion through its existing
  carrier-generic carrier-store path, with no Angular-specific row (`ANG6-AC6`).
- **`@angular/language-service`** is an oracle only.

No operation needs a TypeProvider route other than the existing tsgo composite,
so the rescope trigger does not fire.

## Facets (ruling 3)

| Facet | Sources | Provenance | Proof |
| ----- | ------- | ---------- | ----- |
| props | `input()`, `input.required()`, `@Input`, the input half of `model()` | native | `ANG3-AC1` |
| events | `output()`, `@Output`, `outputFromObservable` | native | `ANG3-AC1` |
| events | the `model()` change event `xChange` | `derived(model-change)` | `ANG3-AC3` |
| slots | `<ng-content select>` | `derived(content-projection)` | `ANG3-AC3` |
| expose | `exportAs` and public members reachable through template refs | `derived(export-as)` | `ANG3-AC3` |
| options, model | none | unsupported | `ANG3-AC5` |

## Operation matrix

The matrix has 58 cells, `ANG-C01` to `ANG-C58`, over the single profile and
the two template locations. Each cell names its host, the tsgo operations it
needs or the tsgo limitation that makes it carrier-only, one producer node and
one receiving acceptance. In summary:

| Area | Producer | Host |
| ---- | -------- | ---- |
| Grammar, recovery, microsyntax, wire tag | ANG1 | kernel |
| Activation gate, attachment and maps, displaced routes | ANG1M | kernel |
| External-template syntax diagnostics, symbols, folding, carrier tokens | ANG1S | Verter LSP |
| Scope binding, template references, index contributions | ANG2 | kernel |
| Component info and facets | ANG3 | kernel |
| Control-flow, signal, hydration-annotation and realm facts | ANG4 | kernel |
| TS-region hover, diagnostics, companion sync, TS-plugin compatibility | ANG6 | tsgo composite (generic TS plugin for compatibility) |
| TS-region navigation; selector references; carrier completion | ANG5 | tsgo composite; Verter LSP for carrier-only |
| TS rename mapping and refusal; selector and alias rename plans; step-down | ANG5R | tsgo composite; Verter LSP for carrier-only |
| Style and accessibility facts | ANG7 | kernel |
| Lint host | ANG8 | LNT2 lint host |
| Formatting, including inline templates inside their literal | ANG8F | formatter |

Inline templates are formatted inside their string literal through the ANG1M
escape-aware maps (`ANG8F-AC4`).

Two cell groups have no acceptance in their producer's charter, and an operator
question (`ang0-unacked-cells`) is open on them:

- TS-region completion (`ANG-C38`) and carrier-only completion (`ANG-C41`) are
  claimed by the ANG5 and ANG6 outcomes. Their receiving acceptance is
  `ANG9-AC1`, the coverage join every admitted cell must pass.
- Carrier-structure features inside inline templates (`ANG-X14`) are recorded
  as an exclusion. ANG1S serves external templates only and LSPX11 registers no
  Angular contributor.

ANG9 joins every admitted cell to a passing fixture (`ANG9-AC1`). ANG10
promotes Angular only when every cell has an ANG9 pass or is a ratified
exclusion (`ANG10-AC1`).

## Exclusions

| Id | Row | Reason |
| -- | --- | ------ |
| `ANG-X01` | consuming `ng build`/`ngc` output or its maps | ruling 4: no BND consumption |
| `ANG-X02` | a DBG, TST or WPF map promise | ruling 4 |
| `ANG-X03` | rendering, hydrating, serving or executing Angular code | no runtime in verticals |
| `ANG-X04` | NgModule `declarations` scope | ruling 2 |
| `ANG-X05` | `*ngIf`/`*ngFor`/`*ngSwitch` | superseded by built-in control flow |
| `ANG-X06` | Angular LS as authority or route | oracle only |
| `ANG-X07` | the Angular 23 next/rc profile | no published version to pin |
| `ANG-X08` | a Verter-computed TS-region answer | tsgo answers TS regions |
| `ANG-X09` | an Angular-specific TS-plugin row | generic compatibility only |
| `ANG-X10` | activation from `.html`, LK6, `angular.json` or a name match | FWA1 only |
| `ANG-X11` | OPTIONS and MODEL surface kinds | not used by the class-B facet table |
| `ANG-X12` | Prettier as formatting oracle | not an official source |
| `ANG-X13` | TextMate colouring as support evidence | never product evidence |
| `ANG-X14` | carrier structure inside inline templates | outside the chartered scope |

An architecture proof (ANGP), an installed compiler and syntax highlighting
never satisfy a product claim.

## Wire tag

`FRAMEWORK_TAG_ANGULAR = 11`, from the template allocation (class A holds 6–9,
class B 10–14). REG0 pre-allocates it as `DeferredVertical` (`REG0-AC2`), and
ANG1 registers the adapter descriptor and flips `tag_disposition` to
`Registered` (`ANG1-AC5`). `FRAMEWORK_TAG_OPEN_CANONICAL` (5) is a structural
non-tag (`tag_disposition` returns `None`) and is rejected as an adapter tag.

## Baseline

At the described head:

- `FileLanguage::FrameworkTemplate { adapter_id, owner_hint }` exists
  (`crates/verter_language/src/language.rs`), but only test fixtures produce it;
  no built-in `LanguageRow` does.
- There is no Angular `LanguageRow`, no `CarrierGrammarConfig` variant (only
  `Vue` and `Svelte`) and no `FrameworkTag` value (the enum ends at
  `OPEN_CANONICAL = 5`).
- `verter_napi` refuses the `angular` compile variant
  (`an_unknown_framework_is_refused` in `host_compile_request_tests.rs`). That
  refusal stays: Verter does not compile Angular.
- The WDX1 `mixed-framework` fixture pins `angular 20.0.0` (diverged, above).
