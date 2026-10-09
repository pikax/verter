# Declarative Verter and captured ecosystem configuration

This decision defines the hermetic base configuration authority: the
versioned `verter.config.jsonc` envelope, the precedence and provenance of its
layers, opaque product sections whose schemas stay with their translators,
fail-closed outcomes, read sets, and the prepared inputs that hosts without a
filesystem supply. It also defines how ecosystem configuration (tsconfig,
package.json, ESLint, Prettier, Vite, Nuxt, SvelteKit and similar files) is
captured: statically as data, or dynamically only through an explicitly
authorized execution service that yields a new captured snapshot.

Today, framework-shaped host/session registries and untagged public boundaries
own configuration: the LSP `ProjectRegistry`, the `.verterrc.json` lint reader,
the Rust Vite-config analyser, client `initializationOptions`, and host
construction options. The final and sole owner is the typed immutable
universal catalog and the demand-selected kernel services.

It describes the repository at `fix(core): walk a class's extends chain
iteratively for inherited (#797)`, 2026-10-09. It follows the docs-only rule
in [README.md](README.md): it changes no production route and adds no check.
It keys profile sections by the identities of [identities.md](identities.md)
and does not re-own anything the [authority inventory](authority-inventory.md),
the [constitution](constitution.md) or the identity decision assign.

## Machine-readable products

The reviewed contract data lives in `tests/kernel/CFG0/products/`:

| File | Holds |
| ---- | ----- |
| `configuration-inventory.v1.json` | Contract rules `CR01`–`CR24`, outcomes `CF-O01`–`CF-O14`, consumers `CF-C01`–`CF-C19`, retained host inputs `CF-H01`–`CF-H04`, displaced routes `CF-D01`–`CF-D10`, the UAK0 routes it references, coverage of each deletion category, empty populations, the open binding gap `G01`, findings and transferred obligations |
| `configuration-case-table.v1.json` | Cases `CC01`–`CC17`: input, required and forbidden outcome, the rules each case exercises, existing evidence, and the node whose test makes it executable |

Every `successorPath` starts at CFG0 and follows predecessor edges in the
controller-owned plan. UAK0, UAK1 and VID0 rows keep their owners; CFG0 only
references them.

## Envelope v1

One file name is recognised: `verter.config.jsonc` (JSON with comments and
trailing commas). It is data. Nothing in it is executed, and no code path
evaluates JavaScript to read it.

- **CR01.** The envelope is versioned. `version` is required and must be `1`.
  A missing or unknown version is `Invalid(UnsupportedSchemaVersion)`, never
  read as the nearest known version.
- **CR02.** The top-level key set is closed. It is the union of the envelope
  keys (`$schema`, `version`, `root`, `extends`, `overrides`, `profiles`), the
  base sections whose schema this decision owns (`frameworks`, `embedded`,
  `coexistence`), and the product sections registered in the catalog. Any
  other key is `Invalid(UnknownKey)`. `$schema` is an editor hint; it is never
  fetched.
- **CR03.** A product section is an opaque typed slot. Its schema belongs to
  exactly one downstream translator. The envelope knows its name and owner,
  never its fields. The sections known at this decision are:

  | Section | Schema owner |
  | ------- | ------------ |
  | `lint` | LNTCFG0 |
  | `assist` | LK3 |
  | `format` | FCFG0 |
  | `build` | TSB4X (first row: `build.solid`) |
  | `mcp` | AGM5 (per-tool and per-family switches) |

  Registering a section name is a catalog row (CAT0), not an envelope change,
  so adding one does not bump `version`. UAO0 reconciles the catalog rows with
  this table.
- **CR04.** Spellings decode once, at capture, into typed identities. A
  framework family key, a release, a profile key, an embedded-language kind, a
  dialect and a coexistence capability cell each decode through the catalog
  or the owning registry. A spelling that does not decode is `Invalid`; it is
  never kept as a string and never matched later (VID0 `R03`, `R07`).
  Admission is not decoding. A release spelling that decodes to a `ReleaseId`
  the catalog does not admit stays in the configuration, and activation
  yields `unsupported-version` (VID0 `R06`). Rows the decode consulted are
  part of the query identity (`CR14`).

### Base sections

The envelope owns these three schemas. No other base section exists.

- **`frameworks`** — `{ "<family>": "auto" | "off" | { "state": "on",
  "release": "<exact release>" } }`. `auto` is the default. `on` must name a
  spelling that decodes to a `ReleaseId` (VID0 `I04`). A decoded `ReleaseId`
  that is not admitted is a valid configuration value; FWA1 reports
  `unsupported-version` and does not coerce it (VID0 `R06`). A floating tag,
  a range, a versions array or any other spelling that does not decode is
  `Invalid(UnknownFrameworkRelease)` (VID0 `R07`). An unknown family is
  `Invalid(UnknownKey)`. FWA1 owns what the switch does. `frameworks` may
  appear at the top level and in `overrides`, never inside `profiles`:
  profile selection depends on activation, so a profile-scoped activation
  key would be a cycle (`Invalid(ActivationKeyInProfile)`).
- **`embedded`** — tag bindings and default dialects for embedded-language
  recognition. `embedded.tags` is a list of `{ "kind", "module", "export",
  "dialect"? }`: a tag is bound to an import source and export, never to a
  spelling alone. `embedded.dialects` maps a kind to its default dialect
  (`{ "sql": "postgresql" }`). This covers SQL dialect and tags, Cypher tags,
  and Vue/Svelte embedded-document tags. INT5 owns activation from tags, SQL2
  owns dialect resolution and EDOC1 owns embedded documents.
- **`coexistence`** — persisted per-capability ownership choices
  (`{ "<capability cell>": "verter" | "official" | "per-feature" }`). It is
  allowed only in the user layer and in the workspace-root file's own body.
  Anywhere else the key is `Invalid(ScopeRestrictedKey)` and that file is
  `Invalid`. A file the workspace-root file `extends` is not the
  workspace-root file: a coexistence key there is not inherited. A nested
  `"root": true` file stops the chain (`CR06`), so files under it never
  read the workspace-root coexistence section. A coexistence key in that
  nested root file is `Invalid(ScopeRestrictedKey)` and makes the file
  `Invalid`; every scope that reads it is `Invalid` and does not fall back
  to the user layer or to defaults. A nested root file with no coexistence
  key is valid, and files under it take workspace-scope choices only from
  the user layer. There is no per-package and no per-root coexistence
  scope. COXD2 is the sole
  choice authority. The core never writes a choice; a host writes one only
  with consent (COXD3L).

## Precedence and provenance

- **CR05.** Layers apply in one total order, lowest first:
  1. catalog defaults (provenance `default`);
  2. the user layer, supplied explicitly by the host (provenance `user`);
  3. the configuration chain, outermost file first.
- **CR06.** The chain is every `verter.config.jsonc` from the stop directory
  down to the directory of the file being configured. The stop directory is
  the nearest file with `"root": true`, or the workspace root, whichever is
  nearer. The chain never reads above the workspace root.
- **CR07.** A file expands to its `extends` targets in declared order, each
  expanded depth-first, followed by the file's own body. The body wins over
  everything it extends (provenance `inherited` for extended values).
- **CR08.** An `overrides` entry is
  `{ "files": ["<glob>", ...], "excludedFiles"?: ["<glob>", ...], ...sections }`.
  `files` is required and non-empty. Each glob is relative: `*` is one path
  segment, `**` crosses segments, `?` is one character other than `/`. There
  is no brace expansion, no absolute path, no `..` segment and no negation
  inside a pattern; exclusion is only `excludedFiles`. A pattern that breaks
  that rule is `Invalid(InvalidOverrideSelector)`. A configured file matches
  an entry when its path matches any `files` glob and no `excludedFiles`
  glob. Every glob is anchored at the directory of the source file that
  authored the entry, including an entry reached through `extends`. The
  anchor is not the workspace root, not the extending file and not the
  configured file's directory. Paths compare as workspace-root-relative
  paths with `/` separators. Inside one body the rank is: top-level
  sections, then `profiles` for the active project profile, then `profiles`
  for the active framework profile, then each matching `overrides` entry in
  array order (an entry's own `profiles` rank directly after that entry). A
  nested file's lowest rank beats its parent's highest. Discovery order,
  load order, registration order and pattern order never decide.
- **CR09.** `extends` entries are relative paths or package specifiers. A
  package specifier resolves through the project model's module resolution
  (PM2) to a `.jsonc` file. A relative target is outside the workspace when
  its normalized workspace-root-relative path leaves the root. A package
  specifier is outside when the canonical file identity PM2 returns lies
  outside the workspace root. The comparison uses that identity, not the
  unresolved specifier text and not a realpath computed beside PM2. A
  `link:` or workspace symlink is `Invalid(OutsideWorkspace)` when that
  identity is outside, and inside when that identity is inside. A missing
  target is `Invalid(MissingExtends)` when absence is proven and
  `NeedInputs` when it is not (`CR23`). An extended file may itself extend,
  may not declare `root` (`Invalid(RootInExtended)`), and a cycle is
  `Cycle { chain }`.
- **CR10.** `profiles` keys are exact profile spellings that decode to a
  `ProjectProfileId` or a `FrameworkProfileId` (VID0 `I07`, `I05`). A profile
  section applies only where that profile is active in the demand plan. Two
  framework profiles active on one region are VID0's typed ambiguity (`R10`),
  never a merge of both sections.
- **CR11.** Base sections merge by key. Objects merge recursively; scalars and
  arrays replace. Every leaf records the winning contribution and the
  contributions it shadowed, each as source unit, source revision, JSON
  pointer and span.
- **CR12.** Product sections are not merged by the envelope. The consumer
  receives the ordered list of contributions for its section, each with rank,
  source unit, source revision, content, JSON pointer, span and the exact
  authored value. The translator applies its own merge and reports every
  contribution as applied, shadowed or rejected. Nothing is dropped silently.

Provenance kinds are `default`, `user`, `source`, `inherited`, `override`,
`profile` and `captured`. The effective configuration is immutable: a change
produces a new effective configuration, never an edit of the old one.

## Identity and read sets

- **CR13.** A configuration file is a source unit like any other: it is
  identified by `SourceUnitId` and `ContentId` (VID0 `I01`, `I02`). No path
  string is a configuration identity. `SourceRevision` is that unit's
  revision authority (VID0 `I02`) and joins `ContentId` in every source-text
  cache key (`CR14`); it does not stand in for the bytes.
- **CR14.** An effective-configuration query is keyed by the chain's ordered
  `(SourceUnitId, SourceRevision, ContentId)` triples, including the
  `extends` closure (VID0 `R12`); the user layer's host-supplied revision;
  the workspace-root-relative path of the configured file and of each
  declaring config in the chain (override anchors; paths are match inputs,
  not source identities); the active profile identities; the section; and
  the identity of each catalog or registry row the decode consulted
  (product-section registration, and each decoded family, release, profile,
  kind, dialect and capability cell; VID0 `I09`). The key does not carry the
  whole `CatalogSnapshot`. A catalog edit that changes no consulted row
  leaves the key unchanged; one that changes a consulted row misses the
  cache. An admission row is not a row the decode consulted (`CR04`).
  Withdrawing admission of a `ReleaseId` the spelling still decodes to
  leaves the configuration key unchanged and the configuration `Complete`.
  Activation observes that admission row and reports `unsupported-version`
  (FWA1-AC6). Two revisions of one unit with identical bytes do not alias. The
  key is encoded through the canonical tagged encoding (VID0 `R12`, `R13`).
  It is independent of `ConfiguredProjectId`: configuration scope follows
  directories; PM1 binds it to projects. Value fingerprints (`CR16`) stay
  content-addressed and omit `SourceRevision`.
- **CR15.** Every result records its read set: each probed path as present
  (with `ContentId` and `SourceRevision`) or proven absent, each
  package-specifier resolution proof, the user-layer revision, the
  `CatalogSnapshot` identity, the consulted catalog or registry row
  identities, and each captured snapshot it consumed.
- **CR16.** Invalidation is per section and per profile, on two
  content-addressed digests. The value fingerprint covers the ordered
  decoded contributions (rank, source unit, JSON pointer, exact authored
  value) and excludes spans, comments and `SourceRevision`. The span anchor
  covers those contributions' spans. A consumer recomputes only when a
  value fingerprint it read changes. A consumer that publishes diagnostics
  or provenance re-anchors spans when a span anchor it read changes, and
  does not recompute. Editing `format` does not invalidate lint consumers;
  editing a Svelte profile section does not invalidate a Vue-only scope. A
  comment changes `ContentId` and that unit's `SourceRevision`. It changes
  a span anchor only for sections whose contribution spans move, and it
  never changes a value fingerprint. A consumer that has published
  provenance or diagnostics re-stamps `SourceRevision` and `ContentId` on
  those records when either identity changes, including when neither digest
  changes, and does not recompute in order to do so. Span anchors still
  move only when a contribution span moves. Edit and revert restore both
  digests.
- **CR17.** `Invalid`, `Cycle`, `NeedInputs`, `Uncaptured`, cancelled and
  superseded results return to their caller and are never cached as warm
  results (the existing `ReturnOnly` law).
- **CR18.** Profile identities and configuration values are not threaded
  into `HostConfig`, `CompileProfile` or `CodegenOptions` (VID0 `R16`;
  UAK0 seam `S13`). Consumers read configuration on demand.

## Captured ecosystem configuration

- **CR19.** Static capture parses data formats as data (tsconfig and
  jsconfig, package.json, JSON ESLint and Prettier files). For an executable
  config (`*.config.{js,ts,mjs,cjs,mts,cts}`, `svelte.config.js`), static
  capture may only extract literal forms through that tool's fact owner. It
  never executes the file and never spawns a runtime. A file whose extracted
  forms are all literal is `Complete`. A file with both literal facts and
  non-literal forms is `Partial`, and the non-literal forms are named. A
  file with no extractable literal form is `Uncaptured { reason }`, not empty.
- **CR20.** Dynamic evaluation happens only through an explicitly authorized
  execution service (contract XEC0, implementation CENV5). It produces a new
  captured snapshot that records the service, the authorization, the
  declared inputs, the exact tool version, the environment binding and the
  observed read set. It never replaces or mutates the static snapshot. A read
  outside the declared inputs prevents a hermetic-complete claim. Without
  authorization the outcome stays `Uncaptured { ExecutionNotAuthorized }`.
- **CR21.** Every captured snapshot names its tool row, source unit, capture
  mode (`Static` or `Executed`), completeness (`Complete`, `Partial` or
  `Uncaptured`) and an opaque payload whose schema belongs to the tool's fact
  owner: PM1 for tsconfig and package.json, SM1 for Vite, NUX0 and SKT0 for
  Nuxt and SvelteKit, LNTCFG0 for ESLint, Stylelint and TS-ESLint, FCFG0 for
  Prettier. CENV1C adapts them into typed facts with inheritance edges.

## Hosts, environments and secrets

- **CR22.** The user layer is an explicit host input: editor user settings
  sent by the client, or a path the user passes on the command line. Nothing
  reads `$HOME`, XDG directories or other ambient global configuration. The
  workspace root bounds every read.
- **CR23.** A host without a filesystem (WASM, browser, an embedder with its
  own VFS) supplies prepared inputs: the workspace root, each configuration
  file as bytes or as proven absent, the user layer and any captured
  snapshots. The core runs the same parser and merger; JavaScript never
  pre-merges Verter configuration. A path that is neither supplied nor
  proven absent is `NeedInputs { paths }`, never treated as absent and never
  an empty success. NAPI hosts with a real filesystem read through the
  overlay-aware workspace VFS.
- **CR24.** No configuration value holds a secret. A section whose schema
  admits an environment reference stores `{ "$env": "<NAME>" }` as a secret
  handle: the key identity, its scope, its classification and its
  provenance. The value stays host-owned and is resolved only under a grant
  (CENV2). Logs, audit records, fingerprints and read sets use the handle and
  a host-supplied opaque revision, never the value or a hash of it. Base
  sections admit no environment reference. An effective configuration
  captured under an environment (for example a Vite mode) carries that
  environment binding beside its identity; v1 envelope keys are not
  environment-conditional.

## Outcomes

Every effective-configuration query returns one of:

| Outcome | Meaning |
| ------- | ------- |
| `Complete` | Every layer read, decoded and merged |
| `Invalid { diagnostics }` | Syntax error, unknown or scope-restricted key, unsupported version, unknown framework release or profile key, root in an extended file, a target outside the workspace |
| `Cycle { chain }` | An `extends` cycle |
| `NeedInputs { paths }` | A prepared-input host did not supply a file and did not prove it absent |
| `Uncaptured { reason }` | An executable ecosystem config that static capture cannot read and no authorized execution captured |

An `Invalid` file makes every scope that reads it `Invalid`. It never falls
back to defaults, and scopes that do not read it are unaffected.

## Displaced routes recorded here

UAK0 and VID0 do not cover these ten. Each has one production-capable
deletion owner.

| Route | Unit | Disposition | Deletion owner |
| ----- | ---- | ----------- | -------------- |
| `CF-D01` | `.verterrc.json` project reader (`discover_lint_config`, `load_verterrc`, `VerterProjectConfig`) in `verter_diagnostics`, used by the LSP and MCP | replace with the `lint` section; never merged beside the envelope | LNTCFG0 (`LNTCFG0-AC1`) |
| `CF-D02` | Legacy ESLint JSON reader (`load_eslint_config`: `.eslintrc.json`, package.json `eslintConfig`, `vue/` rules only) | replace with captured ESLint snapshots and the static translator | LNTCFG0 (`LNTCFG0-AC1`) |
| `CF-D03` | Lint settings from client and process inputs: `merge_init_options` over `initializationOptions.lint`, `verter.lint.*` and `verter.mcp.lintPreset`, MCP `McpServerConfig.lint_preset` | replace with the user layer and the `lint` section | LNTCFG0 (`LNTCFG0-AC1`) |
| `CF-D04` | Rust-side Vite config capture: `analyze_vite_config`, `execute_trusted_vite_config` (spawns Node from the LSP host), `LKG_CACHE`, `ViteConfigOptions`, `$/verter/viteConfigTrustRequired` | replace with JS-host capture adapters and `HostAliasFact` | SM1 (`SM1-AC1`) |
| `CF-D05` | SSR and Nuxt detection by file-name probe (`detect_ssr_project`: `nuxt.config.*`, `.nuxt/`; unplugin `detectNuxt`) | replace with Nuxt profile applicability from captured facts | NUX0 (`NUX0-AC1`) |
| `CF-D06` | Duplicate tsconfig readers: `verter_workspace::config` and `virtual_config` (a second `extends` walk that follows only a string `extends`), `verter_tsc::tsconfig`, component-meta `parseTsconfig` (no `extends`) | replace with one captured tsconfig lineage and discovery read set | PM1 (`PM1-AC3`) |
| `CF-D07` | Whole-workspace re-initialisation on any configuration-file change (`is_config_file` → `trigger_registry_rebuild`; `WorkspaceChange::ConfigChanged` graph rebuild) | replace with read-set invalidation and atomic snapshot publication | PM3 (`PM3-AC3`) |
| `CF-D08` | Host-config wire that accepts unknown fields silently (`FfiHostConfig` has no `deny_unknown_fields`; `WasmHostConfigWire` flattens it) | reject unknown fields through the versioned public envelope | PUB0T (`PUB0T-AC4`) |
| `CF-D09` | unplugin `template.compilerOptions.isCustomElement`, documented but not forwarded (`typedRenderRequest` sends `isCustomElement: []`) | capture the consumer policy and invalidate on change | VCE0 (`VCE0-AC2`) |
| `CF-D10` | `verter.lint.*` client changes neither restart nor notify the server, so lint settings stay stale until restart | replace with user-layer revisions under `CR16` | LNTCFG0 (`LNTCFG0-AC3`) |

CFG0 references these routes owned elsewhere:

| Route | Owner | Concern |
| ----- | ----- | ------- |
| UAK0 `D09` | COX0 | Framework admission through `--frameworks`, `FrameworkOptions` and the NAPI/WASM `frameworks` option; its configuration successor is the `frameworks` base section read by FWA1. `NapiMetaProject` ignores `frameworks`, which belongs to the same population |
| UAK0 `D10` | COX0 | The `vue` configuration section the client sends and the server never reads (`initializationOptions.configuration`) |
| UAK0 `D19` | PM1 | `ProjectRegistry` and `ProjectConfig` as the per-project configuration authority |
| VID0 `V-D05` | FWA1 | Svelte release admission by string prefix |

Deletion-category coverage:

| Category | Recorded here | Referenced |
| -------- | ------------- | ---------- |
| central framework switch | `CF-D05` | UAK0 `D10` |
| untagged coordinate/public identity | `CF-D08` | UAK0 `D09`, `D19`, VID0 `V-D05` |
| duplicate component information authority | none: no configuration route carries component information | — |
| configuration authority (this decision's own population) | `CF-D01`–`CF-D04`, `CF-D06`, `CF-D07`, `CF-D09`, `CF-D10` | — |

### Retained host inputs

These are host or session settings, not configuration layers. They stay with
their owners and never enter an effective configuration:

- `CF-H01` — construction options (`HostConfig`, `FfiHostConfig` fields,
  budgets, audit, scheduler): UAK0 seam `S13`.
- `CF-H02` — client presentation settings (statistics, tracing, log level,
  hover, inlay hints, decorations, MCP port, provider recommendations).
- `CF-H03` — engine provisioning (`verter.typeProvider`,
  `verter.typescript.tsdk`, `verter.lspBinaryPath`, `VERTER_TSGO_BIN`, and
  `HOME`/`PATH` probes that locate Node): EPR4 through EPR1.
- `CF-H04` — experimental compile flags (`experimental.strictSlots`,
  `experimental.conditionalRootNarrowing`, MCP `strict_slots`) feeding
  `CompileProfile`. Their option vocabulary is CMP0's; CMP0-F assigns any
  later move to a profile-scoped key.

## Open binding gap

**`G01` — the envelope loader has no implementation owner.** No production
code reads `verter.config.jsonc` at this head, and no plan node owns the
loader: envelope parsing (`CR01`–`CR04`), the base-section schemas, the
precedence merge (`CR05`–`CR12`), the effective-configuration key and read set
(`CR13`–`CR17`), the user layer and prepared inputs (`CR22`, `CR23`). The
charter's 2026-10-02 amendment records this finite gap. It covers `CF-O01`,
`CF-O02`, `CF-O03`, `CF-O04`, `CF-O06`, `CF-O07` and `CF-O14`. Their
implementation ownership is unresolved under that exception. UAO0 reports a
missing loader as a finding against this binding table and does not implement
it. Every other outcome has one owner.

## Findings recorded for the receiving owners

- **Rust executes trusted Vite configs.** `execute_trusted_vite_config`
  spawns Node from the LSP host once a user trusts the file. UAK1 records
  "project configuration is not executed by Rust or WASM at this head"; that
  holds for embedded engines, not for this child process. The route is
  `CF-D04` (SM1). CENV1C-AC2 still proves UAK1 `U01`.
- **No server-side configuration change handler.** The server has no
  `workspace/didChangeConfiguration` handler. The VS Code client restarts the
  server for some settings and does nothing for others (`CF-D10`).
- **The `.verterrc.json` `ssr` key has no successor key.** `CF-D01`'s lint
  and ignore keys move to `lint`; its `ssr` key waits on `G01`.
- **Settings sent and never read.** `initializationOptions.frameworks` (UAK0
  `D09`) and `initializationOptions.configuration` (UAK0 `D10`).

## Empty populations

- **No `verter.config.*` loader.** The only mention is a design note in
  `docs/plans/framework-plugin-system.md`. The gap is `G01`; this head has
  none to delete.
- **No ambient global configuration.** No production code reads
  `dirs::home_dir`, XDG directories or `~/.config`. `HOME` is read only to
  locate Node (`CF-H03`).
- **No framework section in tsconfig.** No `vueCompilerOptions` or similar
  key is read.
- **No configured embedded-language tags, dialects or coexistence choices.**
  INT5, SQL2, EDOC1 and COXD2 start from an empty population.
- **No ecosystem config read for Svelte, Prettier, EditorConfig or Biome.**
  `svelte.config.js`, `.prettierrc`, `.editorconfig` and `biome.json` are not
  read.
- **No configuration value carries a secret.** No production path resolves an
  environment reference inside a configuration value.

## Acceptance evidence

Evidence selection: the change adds contract text and data only, so existing
coverage and bounded inspection discriminate it. The diff adds no test.

- **AC1 — ownership contract.** The inventory binds every outcome, consumer
  and displaced route to one existing plan node, a successor path from CFG0
  and a receiving acceptance ID, except the loader outcomes held by `G01`
  (`CF-O01`, `CF-O02`, `CF-O03`, `CF-O04`, `CF-O06`, `CF-O07`, `CF-O14`).
  Their implementation ownership is unresolved. UAO0 reports that gap and
  does not implement the loader.
  The executable validator is UAO0's (`UAO0-AC-R1`), and so are the
  precedence, fail-closed, provenance and invalidation fixtures: `CC01`–`CC04`,
  `CC06`, `CC07` and `CC13` (`UAO0-AC-R2`). Every other case names its own
  executable owner.
- **AC2 — positive contract.** Existing coverage pins the boundaries this
  decision names and the behavior it displaces:
  - fail-closed admission names: `unknown_name_is_rejected_naming_the_supported_set`,
    `duplicate_name_is_rejected` and `empty_admission_is_rejected` in
    `verter_session` `framework::options`, and
    `wasm_frameworks_key_narrows_the_constructed_host` in `verter_wasm`;
  - configuration lineage and identity in `verter_workspace`:
    `virtual_identity_advances_on_extends_ancestor_change`,
    `project_identity_changes_when_tsconfig_path_changes`,
    `extends_only_alternate_config_is_not_registered_as_file_owner_and_package_wins`
    and `overlapping_tsconfigs_returns_ambiguous`;
  - the displaced readers: `discover_lint_config_verterrc` and
    `discover_lint_config_eslintrc` in `verter_diagnostics`;
    `registry_per_project_lint_config`,
    `fallback_project_complex_config_not_trusted` and
    `disabled_vite_fallback_has_no_aliases` in `verter_lsp` `config`;
  - the request boundary: the `typeinfo_request_validation` cases in
    `verter_session`, and `typeinfo_proto_roundtrip` and
    `typeinfo_proto_ts_contract` in `crates/verter_protocol/tests/cases/`.

  New tests for the envelope and the case table belong to UAO0 and to the
  owners named per case.
- **AC3 — incremental equivalence: not applicable.** No cache, cancellation,
  stale-publication or partial-result authority is touched, and no production
  byte changes. `CR16`, `CR17`, `CC07`, `CC08` and `CC17` bind the later proof.
- **AC4 — bounded work: not applicable.** No hot path changes. `CC07` fixes
  the zero-recompute expectation for irrelevant edits; DEM0's zero-work
  fixtures stay UAO0's.
