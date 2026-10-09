# ERB delivery and exact-version contract

This is the stage-0 contract every later ERB node builds against, and the
ERB-hosted STIM1E and HRF1-ERB with them. It fixes the engine profiles, the
`erb` activation row FWA1 reads, the file associations, the operation × host ×
profile matrix with one producer per cell, and the coexistence cells for Ruby
LSP and Herb.

It describes the repository at `chore(*): compile dependencies optimised in
the dev and test profiles (#825)`, 2026-10-09. At that head the repository has
no ERB, HTML-carrier or Ruby code, and `CarrierGrammarConfig`
(`crates/verter_language/src/carrier_grammar.rs`) has only the `Vue` and
`Svelte` variants.

It follows the docs-only rule in [the kernel README](kernel/README.md): it
writes contract data and text, and adds no production code, validator, test or
CI lane. It builds on the claim and routing law (WDX0), the ERB1 parser
admission (PAR0 `CL16`: island scanner plus a minimal Ruby boundary lexer, no
Ruby grammar, home `verter_erb_syntax`; see
[parser ownership](kernel/parser-ownership.md)) and the exact-release law
(VID0; see [identities](kernel/identities.md)), and re-owns nothing they assign.

## Machine-readable products

The reviewed contract data lives in `tests/framework-erb/ERB0/`:

| File | Holds |
| ---- | ----- |
| `products/erb-version-lock.json` | Exact releases with sources, the two engine profiles (tag set, trim modes, close rule, escaping), the excluded releases and the next-major rule |
| `products/erb-activation-policy.json` | The `erb` activation row: the lockfile source and its selection order, the explicit `on`, the activation states, forbidden sources and file associations |
| `products/erb-capability-matrix.json` | The single host, 27 operations, one cell per operation × profile (54 cells) with producer and acceptance items, the exclusions and the consumers |
| `products/erb-coexistence.json` | The Ruby LSP and Herb tool entries and one COXD1 cell per capability a competitor also provides |
| `cases.md` | Every planted row the lock validator must reject, with its expected reason, and the positive cases |
| `manifest.json` | The lock's products, mandatory cases and validator binding |

FWA1, FCH1 and REG0 read these files as data. They are not CI-checked until
ERB1G lands the validator (`contract.ts`, `erb-lock.spec.ts`; `ERB1G-ACV`),
which REG0's lock runner discovers.

## Decisions

### Profiles

One profile per engine, each holding exactly one release (VID0 `R04`, `R05`):

| Profile | Engine | Pin | Admitted line | Host |
| ------- | ------ | --- | ------------- | ---- |
| `erb.erubi-1.13` | Erubi | 1.13.1 | 1.13 | Action View 8.1.4 (line 8.1) |
| `erb.erb-6.0` | `erb` gem | 6.0.7 | 6.0 | none: Action View 8.1 compiles templates with Erubi |

The pins are the charter's starting pins. A resolved version in a profile's
admitted line activates that profile; the pin is the release its conformance
expectations are written against. Anything else is `unsupported-version`
(`R06`): Rails 7.x is legacy, prereleases are never admitted, and a range,
`latest` or a `Gemfile` requirement is never a release (`R07`, `R08`). A next
major becomes a second profile only once announced and ratified in this lock;
none is announced.

What each profile records, from the engines' own sources:

- **Action View's handler** defaults `erb_implementation` to Erubi and
  `erb_trim_mode` to `"-"`, and builds Erubi with `trim` on and `escape` on only
  for `text/plain` templates. So in Action View `<%=` is escaped through the
  output buffer and `<%==` is raw, inverted for `text/plain`.
- **Erubi 1.13.1** recognises `<%`, `<%-` (same as `<%`), `<%=`, `<%==`, `<%#`
  and `<%%` (emits the tag text literally). The tag body is the shortest run up
  to the first `%>`. `%%>` is not recognised. With `trim` on, a code or comment
  tag alone on its line drops that line's surrounding whitespace. Outside
  Action View its escaping is an application option Verter cannot read, so it
  is unknown.
- **The `erb` gem 6.0** recognises `<%`, `<%=`, `<%#`, the literal `<%%`, and
  `%%>` as a literal `%>` inside a tag. `<%==` is not a tag. Trim modes are
  none, `%`, `<>`, `>`, `-`, `%<>`, `%>` and `%-`; `<%-`/`-%>` are delimiters
  only under `-`, and `%` lines only under a mode containing `%`. The mode is a
  constructor argument the calling code chooses, so without configuration it is
  unknown and never guessed. `<%=` is never escaped.
- **Both engines close a tag at the first `%>`**, whatever Ruby string or
  comment it falls in. ERB1 reports that break (`ERB1-AC1`); ERB4 diagnoses it
  (`ERB4-AC3`). The `%%>` fix applies only to the `erb` gem profile; for Erubi,
  ERB4 offers a fix that profile reads or records why there is none.
- **turbo-rails and stimulus-rails helpers** are opaque Ruby calls. Their
  vendored JS releases are admitted by STIM0, not here.

### Activation

FWA1's `FrameworkActivation` record is the only activation source (`FWA1-AC7`).
The `erb` row reads `Gemfile.lock` at the package root as data, taking the
resolved specs: `actionview` first, else `erubi`, else `erb`. The first present
gem decides; a present gem outside its admitted line is `unsupported-version`
and never falls back to a later gem. `Gemfile`, `bundle` and Ruby are never
read or run.

Without a lockfile, only `frameworks.erb = on` naming a profile id activates;
`on` without a profile activates nothing and says why. `off` disables the
vertical and an inactive `erb` does zero work (`ERB2-AC5`). A file name, a
`<script src>` URL, a `Gemfile` requirement or an importmap pin never
activates.

### Associations

| Pattern | Claim |
| ------- | ----- |
| `*.html.erb`, `*.turbo_stream.erb` | gated: HTML host while `erb` is active |
| `*.erb` | HTML host only with an explicit configuration entry (key bound by ERBP0) |
| `*.js.erb`, `*.text.erb`, other non-HTML formats | never claimed |
| `*.html+<variant>.erb` | not ratified: the charter's association decision does not name Action View variant spellings; claiming them needs an amendment to this lock |

ERBP0 owns the associations, document selectors and CLI globs
(`ERBP0-AC1`, `ERBP0-AC3`).

### Matrix

There is one host, `verter-lsp.erb-document`: the Verter LSP serves the ERB
document itself. It needs no tsgo operation, has no tsgo limitation and has no
TS projection or `packages/typescript-plugin` route. There is no `FrameworkTag`
and no facet mapping: partials are carrier facts, not ComponentInfo.

| Producer | Operations | Acceptance |
| -------- | ---------- | ---------- |
| ERB1 | `scan.islands`, `scan.block-structure`, `scan.recovery`, `scan.incremental` | `ERB1-AC1`, `AC3`, `AC2`, `AC5` |
| ERBH1 | `compose.templated-html` | `ERBH1-AC1`–`AC4` |
| ERB2 | `compose.regions`, `compose.hosts`, `activation.inactive-zero-work` | `ERB2-AC1`, `AC2`, `AC4`; `AC3`; `AC5` |
| ERB3 | `op.block-pairs`, `op.linked-editing`, `op.html-features`, `op.semantic-tokens`, `op.structure` | `ERB3-AC1`–`AC5` |
| ERB4S | `sinks.raw-output` | `ERB4S-AC1`–`AC3` |
| ERB5F | `format.document` | `ERB5F-AC1`–`AC3` |
| ERB4 | `lint.markup`, `lint.percent-close-in-ruby-string` | `ERB4-AC1`, `AC2`, `AC4`; `AC3` |
| ERBA | `assists.structure` | `ERBA-AC1`–`AC4` |
| HRF1-ERB | `fragments.html-response` | `HRF1-ERB-AC1`–`AC4` |
| ERBP0 | `client.registration` | `ERBP0-AC1`–`AC3` |
| ERB9 | `qualification.conformance`, `qualification.incremental`, `qualification.exposure` | `ERB9-AC1`, `AC2`; `AC3`; `AC4` |
| ERBT | `terminal.capability-truth`, `terminal.step-down`, `terminal.mcp`, `terminal.docs` | `ERBT-AC1`–`AC4` |

Every operation has one cell per profile with the same producer; profile
differences (`<%==`, `%%>`, escaping) are recorded on the cell. Truthful
exclusions with no producer: rendering or evaluating a template; running Ruby,
Bundler or Rails; Ruby semantics inside an island; any tsgo or TS-plugin route;
any non-HTML host; build integration; facets or a wire tag; and the meaning of
helper calls. STIM1E reads only ERBH1's templated-HTML facts, never island
text.

### Coexistence

Ruby LSP (`Shopify.ruby-lsp`) answers Ruby features inside islands and
delegates HTML requests to the HTML language service. Herb
(`marcoroth.herb-lsp`; `@herb-tools/core`, `@herb-tools/linter`,
`@herb-tools/formatter`) is an HTML+ERB parser, a language server with
diagnostics, hovers and formatting, a linter and an experimental formatter.

| Operation | Capability | Tool |
| --------- | ---------- | ---- |
| `op.html-features` | completion, hover | Ruby LSP (delegated) |
| `op.html-features` | hover | Herb |
| `lint.markup`, `lint.percent-close-in-ruby-string` | diagnostics | Herb |
| `format.document` | formatting | Herb |
| `op.semantic-tokens` | semantic tokens | Ruby LSP |
| `assists.structure` | code actions | Ruby LSP |
| `op.structure` | document structure | Herb (parser) |

Every other operation is listed as having no known competitor. COXD2 resolves
ownership per capability, and Unknown means Verter owns. COXD1 binds each
capability name against the COX0 vocabulary (`COXD1-AC1`); the conflict classes
outside COXD1's four seed classes are proposals COXD1 binds or rejects.

## Acceptance

Each acceptance item is met here by the reviewed products and `cases.md`. Its
executable proof, `node --test tests/framework-erb/ERB0/erb-lock.spec.ts`,
moves unchanged to ERB1G (`ERB1G-ACV`).

| Item | Met by | Planted rows |
| ---- | ------ | ------------ |
| `ERB0-AC1` pinned profiles | `erb-version-lock.json` | `P01`–`P12` |
| `ERB0-AC2` owned matrix | `erb-capability-matrix.json` | `M01`–`M14` |
| `ERB0-AC3` activation and associations | `erb-activation-policy.json` | `A01`–`A11` |
| `ERB0-AC4` coexistence declared | `erb-coexistence.json` | `C01`–`C07` |

The abort condition does not apply: FWA1's charter carries a gem-derived
release from `Gemfile.lock` (`FWA1-AC7`) and an explicit `on` naming an
admitted release (`FWA1-AC6`).

## Sources

- Action View ERB handler: `actionview/lib/action_view/template/handlers/erb.rb` in `rails/rails`.
- Erubi 1.13.1: `lib/erubi.rb` in `jeremyevans/erubi`.
- The `erb` gem: `lib/erb.rb` and `lib/erb/compiler.rb` in `ruby/erb`.
- Ruby LSP features: <https://shopify.github.io/ruby-lsp/#features>.
- Herb: <https://github.com/marcoroth/herb>.
