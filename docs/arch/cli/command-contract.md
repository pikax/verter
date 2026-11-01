# Unified command and package contract

This is the semantic lock for the planned CLI, not documentation of a shipped
`verter` executable. The reviewed machine inventory is
[command-contract.v1.json](command-contract.v1.json).
`node --test tests/cli/contract.test.mjs` validates its ownership schema.
No production byte or dependency changes in this contract.

## Package and authority

Publish `@verter/cli` from `packages/verter`, with the single `verter` bin.
The private workspace root remains named `verter`; it is never published and
must never be used as an application dependency. Workspace filters and release
selection use `@verter/cli` explicitly. Existing platform binary packages remain
implementation dependencies until the packaging owner proves their retirement.

The sole application owner is `ApplicationServices` in `packages/verter/src/app`.
It captures one workspace/configuration and service invocation per request.
`SelectionPlan` records canonical subjects, project/configuration identity,
selection provenance and deterministic order; ambiguous or missing targets are
typed failures. `CommandCapability` is a projection of truthful registered
service capabilities, not an independent analyzer or a promise of future support.
`Reporter` serializes the service result without semantic recovery or re-query.
`WriteTransaction` validates every edit's source revision, map, encoding and
conflicts before an atomic multi-file commit; refused, cancelled or stale plans
write nothing. No command writes files around this boundary.

These are application boundaries over the existing
[public result authority](../kernel/public-envelope.md),
[coordinate law](../kernel/coordinates.md) and
[rule/action authority](../kernel/rules-actions.md). They add no semantic cache,
parser, resolver, fallback engine or process-global mutable session.

## Grammar and effects

`verter <command> [options] [--] [targets...]` is the batch grammar. Common
options are `--project <path>`, `--config <path>`, `--reporter human|json|sarif|quiet`,
`--help` and `--version`. The latter two also work without a command and perform
no project loading. No command means usage and exit 2, never an implicit check.
Unknown commands/options, incompatible flags and ambiguous selection fail before
work. `--` terminates option parsing; targets retain authored spelling as
presentation data while selection uses canonical identities.

| Command | Meaning | Mutation |
| --- | --- | --- |
| `typecheck` | Verter-native project diagnostics | None; rejects emit flags |
| `tsc` | TypeScript-compatible project checking and supported declaration emit | Only explicit compatible emit flags |
| `compile` | Registered carrier backend compile, selected target and maps | Explicit `--out-dir`; otherwise result payload |
| `type-info` | Typed selector query; positions declare encoding | None |
| `lsp`, `mcp` | Host the corresponding existing service | Service protocol governs operations |
| `format` | Formatting check by default | Explicit `--write` via transaction |
| `lint` | Lint diagnostics | None; edits belong to `fix` |
| `check` | Aggregate available checking services, reports coverage | None; rejects emit/write flags |
| `fix` | Aggregate safe edit plans | Explicit `--write`; default previews |
| `build` | Native TypeScript project build | Separate compiler train; explicit build request |

`tsc` reserves its compatible option grammar and exit behavior; it does not
reinterpret TypeScript flags as shell flags. It accepts no generic reporter flag
unless the compatible adapter explicitly declares that extension. The compiler
disposition vocabulary is exactly `Supported | FutureSeparateTrain |
NotApplicable`: `compile` and `tsc` are Supported *design routes*, not claims of
all-target engine maturity; `build` is FutureSeparateTrain; other commands are
NotApplicable. Each actual carrier/backend/target cell still needs truthful
capability evidence. Missing engines are omitted from help, discovery and
registration; explicit invocation returns Unsupported and never success/no-op.
A late formatter, lint or build service does not block base CLI packaging.

## Results, streams and exits

Batch exits are 0 for a complete successful outcome, 1 for reported findings
(including formatting differences), 2 for usage/configuration/unsupported or
incomplete/refused work, 3 for infrastructure failure, and 130 for cancellation.
A partial result cannot exit 0. `tsc` preserves TypeScript's 0/1/2 distinction
including diagnostics with skipped versus generated outputs. Protocol hosts
preserve their service lifecycle exit semantics instead of batch diagnostics.

Human diagnostics/results use stdout; operational errors, progress and warnings
use stderr. JSON mode writes exactly one UTF-8 JSON value followed by newline;
SARIF writes one SARIF 2.1.0 log only for services with a truthful diagnostic
mapping. Unsupported reporters fail before invocation. Quiet mode suppresses
ordinary output while preserving fatal stderr and exit status. LSP/MCP stdio is
exclusively protocol traffic; no banner, warning, batch envelope or reporter is
injected. HTTP readiness retains the existing MCP service record and is not a
batch JSON response.

JSON schema epoch 1 has fields `schemaVersion: 1`, `command`, `selection`,
`result` and `exitCode`. `selection` carries the SelectionPlan above; `result`
is the shared versioned public envelope, not a second command-local DTO.
Negotiation and unknown-epoch refusal follow PE01. Preserve exact producer
identities, bases, maps, per-child provenance, completeness and open causes
(PE02–PE04); canonical typed keys determine ordering (PE05). Reporters cannot
promote partial data or infer completeness from an empty diagnostic list.
Serializers may change presentation only, never query identity. Concrete shared
payload types and generated schemas remain PUB0T's; CLI1 owns the invocation
wrapper and serializers. A command-specific payload is versioned by its service.

## Migration inventory and retirement

Each machine inventory row has one implementation/deletion owner, successor
production path, a path from CLI0 through existing successor nodes, and the
owner's receiving AC1. The validator rejects omissions, duplicate/conflicting
assignments, unknown owners, absent paths and invalid receiving acceptance IDs.
It validates this reviewed population, not controller readiness or production
completion. Controller DAG evidence confirms PUB0 and BR0 implemented; no local
ledger or history pin is introduced.

Current routes inspected: binary-launcher `runLauncherCli` inherits stdio;
`verter-lsp` exports `serverBinaryPath`/`serverBinaryCandidates` and its bin delegates
to that shim; `verter-tsc/bin/run.js` has its own platform resolution and invokes
`verter-tsc`; `verter-mcp` uses the shared launcher; native
`verter-mcp-server` calls `verter_mcp::run::run`, as does `verter-mcp`.
Editors and agent hosts may resolve native paths directly and are not forced
through a Node per-message proxy. Keep those resolution APIs until their
receiving packaging owner provides parity.

CLI1 rejects command-local semantic engines, non-atomic multi-file writes and
untagged offset inputs at the application boundary. This does not claim today's
launchers contain analyzers or transactions. CLITS0 retires the separate tsc
launcher logic on its compatible adapter cutover; CLI4 owns MCP/native alias
routing; CLI5 owns LSP and package/bin retirement. These assignments partition
physical routes; the shared application owner remains singular.

Compatibility lasts exactly one published unified-CLI release: old executable
names delegate to the corresponding command, preserve compatible args/exits and
protocol streams, and warn on stderr only when protocol-safe. No semantic dual
run or indefinite alias. CLI5 records the first unified release version and the
next release's deletion receipt with each wrapper path removed or structurally
rejected. CLITS0 and CLI4 supply their route parity/deletion evidence to CLI5;
the package terminal cannot silently keep a wrapper because its owner is late.
Native development/test entry points that remain must be explicitly classified
as non-public service entries, not a second application authority.

## Verification and rationale

CLI0-AC1 uses the inventory schema and its omission/conflict/unknown/pathless
negative cases. CLI0-AC2 uses existing binary-launcher CLI tests for argument,
exit, stdout, stderr, self-spawn and inherited-protocol behavior. Future service
adapters must prove shared-envelope preservation rather than treating this
contract as evidence they already do. CLI0-AC3 is not applicable: no incremental,
cache, cancellation or publication authority changes. CLI0-AC4 is not applicable:
no hot path changes, so no counters, timing thresholds or soak are added.

Security gates for CLI1/CLI5 retain launcher self-spawn rejection, explicit root
selection, no shell interpolation of targets, protocol stream isolation and
transaction validation. Service-host authentication remains the service's owner.
CLI1 owns startup/no-work work-count evidence; CLI5 owns bounded watch retention;
CLI3 owns the `cli3-cli-resources` evidence-run manifest. Timing, allocation and
RSS observations are advisory under the standing measurement rule; none is a
new machine-specific acceptance threshold.

The added validator's concern is correctness: omission or double ownership of a
retirement route is plausible during independent adapter implementation. Existing
launcher CI tests exercise runtime delegation, not future migration assignments
(`notCoveredBecause`). Transaction/encoding/envelope requirements prevent partial
writes, mislocated edits and false-complete results; existing launchers cannot
check those planned application boundaries. They are receiving obligations of
the named owners, not additional CLI0 runtime mechanisms.

Consumer coverage includes the VS Code LSP launcher and MCP lifecycle manager,
Lapce and Zed's direct native launch contracts, benchmark LSP spawning and
programmatic resolution APIs. Their inventory owners own compatibility evidence
at the package boundary, not edits to those consumers outside their charter.
JetBrains discovers via its existing companion/health integration rather than
an npm launcher import; no CLI route replaces that integration in this lock.

Selector grammar for `type-info` is `--file <path> --position <integer>
--encoding utf8|utf16`; offsets are zero-based code units in the selected source
revision and validated at the coordinate boundary. Missing encoding, split
surrogates, invalid boundaries or stale maps fail as typed errors. Compile
requires `--target <registered-target-id>` and may select `--out-dir <path>`;
unsupported target/carrier combinations refuse before emitting. Service hosts
forward their service's existing arguments after `--`; the shell does not
reinterpret protocol configuration as file targets.

| Contract requirement | concern | prevents | notCoveredBecause |
| --- | --- | --- | --- |
| Scoped package and explicit release selection | process | Plausible accidental selection of the private root as the distributable | Existing per-binary release tests do not know the planned package |
| Complete, single-owner migration inventory | correctness | Plausible omission or conflicting ownership of a displaced route | Runtime launcher tests do not validate future route assignments |
| Captured selection, tagged offsets, unchanged public result identity and quality | correctness | Wrong-project answers, mislocated edits or false-complete results at new adapters | Current launchers only delegate; shared envelope convergence is a separate implementation |
| Transactional writes and distinct read/emit command effects | correctness | Partial multi-file writes or an aggregate check unexpectedly emitting | No unified application transaction or aggregate command ships today |
| Root selection, argv forwarding, self-spawn rejection and protocol-only stdout | security | Untrusted target shell interpretation, recursive spawning or corrupted protocol frames | Existing guards cover launcher spawning; new application adapters must preserve them |
| One-release wrapper retirement | process | Aliases becoming permanent competing application entry points | Current release wiring validates today's packages, not the future removal release |
| Startup/no-work and watch work-count evidence | performance | Duplicate semantic work or retained watch candidates after adapter introduction | No unified shell/watch implementation exists to measure in this docs-only lock |
