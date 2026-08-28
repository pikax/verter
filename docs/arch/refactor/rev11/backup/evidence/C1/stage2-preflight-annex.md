> **SUPERSEDED — HISTORICAL EVIDENCE, NO NORMATIVE FORCE.**
> The single normative artifact for Stage 2 is
> `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-25-C1-STAGE2-CUTOVER.md`.
> Four constructions below were deleted rather than repaired and must not be revived: the S1–S7
> sweep apparatus with its counts and density tables; §A4's rule that any inventory difference
> invalidates ratification; §A5's lane-selection table and its lower-bound proof, including the
> claim that its truth is independent of the codebase; and the illustrative residual list as a plan
> component. The abort procedure survives, in the ruling's §8.

# Stage 2 preflight annex — live-ref baseline, non-exhaustive inventory, build-gate completeness

**Status: PROPOSED — NOT AUTHORITY.** A design annex to `stage2-execution-plan.md`, offered for
architecture-seat ratification. **It deliberately does not assert how it becomes authority.**

An earlier revision claimed that seat ratification plus a `[[document]]` row in
`authority-registry.toml` would activate it. That mechanism does not exist for this file, verified
directly: `scripts/validate-program-state.mjs:2234` admits only `CHARTER`, `AMENDMENT` and `RULING`,
and `:2236` confines each kind to a subdirectory of `docs/arch/refactor/rev11/`. This annex and the
plan both live under `docs/arch/refactor/rev11/evidence/C1/`, so **neither can be registered as
written.** Separately, a document row alone never authorizes execution: a block that has left
`LOCKED` also requires exactly one `[[authorization]]` record.

Resolving that is **governance infrastructure outside C1**, owned by whoever owns
`authority-registry.toml` and `scripts/validate-program-state.mjs`, and it is escalated there. Until
it resolves, the honest statement is: this is a ratifiable design with **no recorded activation
path**, and no Stage-2 work may be dispatched against it. This branch carries no authority-registry
delta and writes no ledger state row.

**Scope of change.** This annex REPLACES the plan's §1 re-pin mechanism (§A1), REPLACES its §2
inventory (§A3, now explicitly NON-EXHAUSTIVE), REPLACES its §5 step-0 amendment permission with a
re-ratification trigger (§A4), and ADDS the completeness criterion the plan never had (§A5). It does
**not** touch the plan's §4 abort conditions: the pre-start tag plus hard reset give a consistent
pre-landing escape, and the single final landing is the irreversible boundary. Those stand
unamended, and AB-1 fires off §A1's live comparison rather than the plan's self-referential one.

**What changed in this revision, and why it is a rescope rather than another repair.** Three
successive attempts to prove the inventory exhaustive by static search failed, and the third
disproved the premise the method rested on (§A2). An exhaustive static inventory is not achievable
for this cutover in a name-sweep shape. Completeness therefore moves off the sweep entirely and onto
semantic/compiler evidence and a build gate (§A5); the textual inventory is retained, with its real
job, and labelled non-exhaustive. This is cheaper as well as sound — it stops paying for a guarantee
a grep cannot give, and takes one a compiler gives for free.

---
## A1 — Baseline is a LIVE REF resolved at execution time, never a recorded SHA

The plan bound baseline `f593b24c8…` and then told the executor to verify drift by comparing against
`f593b24c8…`. **A drift check anchored to the value it is checking cannot fail.** It returned zero
while trunk was two commits ahead, and it would return zero at any distance. This is the defect class
`CLAUDE.md` already names under Verification Must Prove Execution: a check that matches against the
same source it validates proves nothing, and a self-declared universe is not evidence.

Demonstration, not hypothesis: at the plan's authoring trunk was `f593b24c8`; at review it was
`7ca35fd04`; at this annex it is `0b235e230`. Three values in one working session. **No recorded SHA
survives contact with a moving trunk, so the baseline must be a REF, resolved at preflight.**

**Binding preflight procedure.** At the start of Stage-2 execution, and never earlier:

```
BASE=$(git rev-parse program/architecture-lock)     # LIVE ref — the authority
BEHIND=$(git rev-list --count HEAD..$BASE)          # must be 0
AHEAD=$(git rev-list --count $BASE..HEAD)
```

- `$BASE` is resolved from the ref **at that moment**. It is recorded into the preflight record as
  an OBSERVATION of where trunk was, never re-read as the thing to compare against.
- `BEHIND != 0` ⇒ rebase onto `$BASE`, re-run §A2's sweeps in full, and re-enter §A4's
  re-ratification. It is **AB-1** and it aborts the run.
- The comparison is invalid if it names any literal SHA. A preflight record whose command contains a
  hex SHA in the compared position is itself a finding.
- `git fetch` the ref first if the branch's remote-tracking copy could be stale; a local ref that
  nothing updated is a recorded SHA wearing a ref's name.

**Current observation (informational, expires immediately):** trunk `0b235e230`, branch 3 behind. The
branch is deliberately NOT rebased by this annex — rebasing now only relocates the staleness, and the
trunk-side charter landing is in flight, so the branch's charter commit is expected to become a no-op
at its next rebase. Freshness is §A1's job at execution time, not a state to maintain by hand.

## A2 — Enumeration method, and its proven limits

### The lexical premise is disproved, not narrowly missed

The prior revision rested completeness on one property: *every reference spells the item's name, and
renaming at import is the only escape.* That property is false. Verified first-hand in this tree:

```
verter_lsp/src/sync_coordinator.rs:744        resolver: published.snapshot.resolver.clone(),
verter_lsp/src/sync_coordinator.rs:876        resolver: &snapshot.resolver,
verter_lsp/src/background_drain_owner_loss.rs:110
verter_lsp/src/project_resolver.rs:1          pub use verter_semantic::analysis::project_resolver::*;
```

The first three reach `ProjectResolver` **by inference** through the `WorkspaceSnapshot.resolver`
field without ever writing the type. The fourth **glob-re-exports** resolver items without naming any
of them, so which items flow through it cannot be read off the line. The `sync_coordinator` sites
appear in no sweep output — confirmed against the union file, not assumed.

**Inference and glob re-export are two further escapes, and they evade S1–S7 by construction. A
textual sweep cannot see a type that is never written.** No additional pattern closes this: the limit
is the model, not its coverage.

The progression is worth recording, because it is the reusable part:

| Cycle | Repair made | Why it still could not be complete |
|---|---|---|
| 1 | searched one SHAPE (call sites) and reported it as the whole | missed stored state (`workspace_snapshot.rs:46`) |
| 2 | all four name-resolution SHAPES, but two executed over a narrower DOMAIN than asserted | missed a whole crate (`verter_mcp`) |
| 3 | domain corrected to the full tree for every name | the MODEL is lexical; it cannot see what is never spelled |

Each repair was correct and none could have reached completeness. **A static name sweep is not an
exhaustive reference oracle for Rust, and no revision of one becomes one.**

### The sweeps are retained, relabelled, and re-purposed

They are **NON-EXHAUSTIVE by construction**. They answer: what work exists, where it is, and how each
item is dispositioned. They do **not** answer: is this all of it. That question moves to §A5.

Sweeps are named, never counted — a count in prose goes stale exactly as `§A4`'s "five" and this
section's own former "six" did while the table held seven rows. Both are removed.

| Sweep | Command | Domain |
|---|---|---|
| S1 | `grep -rn "crate::resolver" crates/verter_workspace/src/ --include="*.rs"` | `verter_workspace` — inherent: `crate::` names only same-crate items |
| S2 | `grep -rn "verter_workspace::resolver" crates/ --include="*.rs"` | all crates |
| S3 | `grep -rn "verter_workspace" crates/verter_semantic/src/ --include="*.rs"` | `verter_semantic` — answers the Cargo-edge-blocker question (§A3.1), not the consumer question |
| S4 | `grep -rn "crate::project_resolver\|project_resolver::" crates/verter_lsp/src/`; `grep -rn "analysis::project_resolver" crates/`; `grep -rn "use .*project_resolver::\*" crates/` | all crates |
| S5 | `grep -rnw "ProjectResolver\|NativeProjectResolver" crates/ --include="*.rs"` | all crates |
| S6 | for each name in the re-export block at `verter_workspace/src/lib.rs:183-190`: `grep -rnw "<name>" crates/ --include="*.rs"` | all crates |
| S7 | `grep -rn "use .*<name>.* as " crates/ --include="*.rs"` | all crates |

The S6 name list is re-read from the re-export block at step 0, never hand-kept.

### Named residual blindness

Recorded as a property of the method, not as a caveat to be waved through:

1. **Inference through field access** — a use whose type is never written (`snapshot.resolver`).
2. **Glob re-export** — forwards items without naming them; the forwarded set is not readable from
   the line.
3. **Expansion-time identifier synthesis** — one `paste::paste!` site,
   `verter_lsp/src/test_harness.rs:1628`, test-harness only.
4. **Multi-line `use … as …`** — S7's regex matches single-line imports only. A broader search also
   returned empty in the current tree, but the regex limit is a property of the sweep and is stated.

`include!` is NOT a residual: its two sites
(`verter_semantic/src/analysis/html_intrinsics.rs:49`, `verter_lsp/src/features/auto_close_tag.rs:256`)
include `.rs` files already inside `crates/` and therefore already swept.

Items 1 and 2 are closed by §A5 and by nothing else.

### Crate exclusions — corrected, and the withdrawn claim named

`verter_compiler` and `verter_language` match S6 on `CARRIER_API_*` / `path_is_carrier` and are
excluded from the totals.

**The exclusion stands. The evidence previously cited for it does not.** The prior revision said
`verter_compiler`'s constants are held by a "byte-equality assertion pinning them at
`src/ide/mod.rs:623-640`". Verified and withdrawn: that test
(`carrier_api_suffix_matches_workspace_naming`, `:629-633`) compares its constants against **string
literals** — `".verter.ts"` / `".verter.js"` — not against `verter_workspace`'s constants. A search
for any test comparing the two crates' constants directly returns **none**.

The exclusion rests instead on manifest evidence, which is stronger than the claim withdrawn:
**neither `crates/verter_compiler/Cargo.toml` nor `crates/verter_language/Cargo.toml` declares a
`verter_workspace` dependency**, so neither can reference the re-exports at all. `verter_compiler`
owns separate same-valued constants; `verter_language`'s match is prose.

Noted for the owning team and out of scope here: `verter_compiler/src/ide/mod.rs:621-625` states the
cross-crate equality is "guarded cross-crate by `virtual_file_naming_characterization` in
`verter_session`". That test does not reference these constants at all. The comment's claim is
unsupported — a small real defect in production documentation, surfaced by this audit, not fixed by
it.
## A3 — The inventory (derived from the tree, NON-EXHAUSTIVE)

**This inventory is not a completeness claim and must not be cited as one.** It is planning
evidence: scope, location, route attribution, per-item disposition. Its blind spots are named in §A2
and closed only by §A5. Read the totals as a **floor**, never as a bound.

Floor: **1,782 coupled lines / 209 files / 10 crates**; of those **800 lines / 94 files / 8 crates
are production**.

| Crate | Production files | Nature |
|---|---|---|
| `verter_semantic` | 24 | Cargo-edge blocker set — §A3.1 |
| `verter_session` | 23 | DTO, glob-chain and root-re-export consumers |
| `verter_lsp` | 22 | shim chain, storage fields, provider/sync consumers |
| `verter_workspace` | 19 | owner crate: module, re-exports, storage, consumers |
| `verter_napi` | 2 | `analysis::project_resolver` entry points + DTO consumers |
| **`verter_mcp`** | **2** | **root re-export: `verter_workspace::path_is_carrier` at `scanner.rs:66`, `server.rs:2761` — the crate the first revision missed entirely** |
| `verter_wasm` | 1 | `analysis::project_resolver` entry points |
| `verter_tsc` | 1 | `is_relative_specifier` |

Test/bench only: `verter_bench` (4 files), `verter_source_policy_gate` (1). Excluded with reason
(§A2): `verter_compiler`, `verter_language`.

### A3.0 — The 13 root-exposed names, with reach and disposition

Reach is repo-wide word-boundary occurrences (S6). Route attribution splits each name's reach into
`module` (`crate::resolver::N` / `verter_workspace::resolver::N` — paths into the module being
DELETED, which break unconditionally) and `root+bare` (`verter_workspace::N`, or a bare identifier
after a `use` — which survive **if and only if** `verter_workspace` keeps the crate-root VALUE
re-exports F23 ratified).

| Name | Reach | module | root+bare | Disposition |
|---|---|---|---|---|
| `IdeProjectConfig` | 375 | 18 | 357 | move → semantic; workspace value re-export |
| `ProjectResolver` | 189 | 10 | 179 | **DELETE — every occurrence changes** |
| `IdeProjectCompilerOptions` | 185 | 14 | 171 | move → semantic; workspace value re-export |
| `ProjectMembership` | 137 | 0 | 137 | **stays workspace-owned — no change** |
| `path_is_carrier` | 81 | 15 | 66 | move → semantic; workspace value re-export |
| `NativeProjectResolver` | 75 | 0 | 75 | **DELETE — alias, no forwarding alias permitted** |
| `WorkspaceAlias` | 70 | 0 | 70 | move → semantic; workspace value re-export |
| `CARRIER_API_VIRTUAL_SUFFIX` | 32 | 2 | 30 | move → semantic; workspace value re-export |
| `carrier_ide_provider_path` | 31 | 2 | 29 | move → semantic; workspace value re-export |
| `strip_carrier_extension` | 19 | 0 | 19 | move → semantic; workspace value re-export |
| `carrier_api_provider_path` | 11 | 0 | 11 | move → semantic; workspace value re-export |
| `CARRIER_API_MODULE_SPECIFIER_SUFFIX` | 9 | 1 | 8 | move → semantic; workspace value re-export |
| `carrier_source_extensions` | 5 | 0 | 5 | move → semantic; workspace value re-export |

Two consequences the flat count hides:

1. **264 occurrences must change no matter what** — the two DELETED names (264 = 189 + 75), across
   every crate that reaches them. This is a FLOOR, not a total: the inferred consumers of the same
   deleted type (§A2 — `sync_coordinator.rs:744,876`, `background_drain_owner_loss.rs:110`) change
   too and are not counted here, because no sweep can count them.
2. **The 51 `module`-path occurrences on the ten moving names break unconditionally**; their ~1,240
   `root+bare` siblings survive ONLY on the ratified value-re-export decision. **If that decision is
   revisited, the repointing surface grows by roughly an order of magnitude in one step.** That
   dependency was invisible in the previous inventory and is the single largest scope risk in Stage
   2; it belongs in the preflight record, not in an executor's head.

### A3.1 — `verter_semantic → verter_workspace` (the edge-reversal blocker)

Unchanged and independently reproduced by the confirming seat: **291 production reference lines
across 23 files** (255 code, 75 comment; counts overlap per line). Densest:
`provider_projection_resolution.rs` (45), `source_id_resolution.rs` (37), `node_modules_resolution.rs`
(30), `package_target_resolution.rs` (29), `project_ownership_resolution.rs` (23),
`tsconfig_paths_resolution.rs` (21), `attempt_outcome.rs` (17), `module_resolver_core.rs` (14),
`resolver_attempt_view.rs` (12), `preferred_specifier_resolution.rs` (11).

~35 distinct workspace symbols, by weight: `resolver::normalize_canonical_id` (37),
`IdeProjectConfig` (37), `ResolutionContext` (30), `resolver::join_paths` (14),
`resolver::is_absolute_specifier` (11), `ResolveResult` (10), `ResolutionKind` (8),
`resolver::path_is_carrier` (6), `resolver::parent_dir` (6), `ResolutionWorldId` (6),
`resolution_currency::PathProbe` (5), `resolver::is_relative_specifier` (4), `ResolveRequest` (4),
`ResolvePhase` (4), `ProjectStableKey` (4), `ProjectOwnership` (4), `AmbientSymbolHit` (4),
`fact_cache::FactVersionRef` (3), `WorkspaceAuthorityId` (3), `ResolveRequestKind` (3),
`ResolutionPopulation` (3), `ProviderTarget` (3), `types::PackageManifest` (2), `WorkspaceAlias` (2),
`resolver::CARRIER_API_VIRTUAL_SUFFIX` (2), plus singles (`resolver::probe_path_for_context`,
`resolver::carrier_ide_provider_path`, `resolution_currency::ResolutionFactKey`,
`fact_cache::AggregateStamp`, `fact_registry`, `fact_read_set`).

**Only ONE production CODE reference to the resolver TYPE survives in semantic**:
`analysis/project_resolver.rs:20`. The other 39 `ProjectResolver` mentions are doc comments — `///
Mirrors ProjectResolver::…` — which reference a deleted type after the cutover. Not a build break
(backticked prose, not an intra-doc link), but a disposition row, not silence.

### A3.2 — Owner crate: module, re-exports, storage

| Site | Kind | Disposition |
|---|---|---|
| `verter_workspace/src/lib.rs:103` | `pub mod resolver;` | DELETE |
| `verter_workspace/src/lib.rs:183-190` | root re-export of the 13 names (§A3.0) | becomes VALUE re-exports of semantic-owned items; `ProjectResolver`/`NativeProjectResolver` DELETED with no alias |
| `verter_workspace/src/resolver.rs:130` | `pub type NativeProjectResolver = ProjectResolver;` | DELETE (75 occurrences / 18 files) |
| `verter_workspace/src/resolver.rs:132` | `impl ProjectResolver { … }` — sole inherent impl; no trait impls on either type | methods relocate to `ModuleResolverCore` |
| **`verter_workspace/src/workspace_snapshot.rs:46`** | **`pub resolver: ProjectResolver` — live STORAGE** | retype to `ModuleResolverCore` |
| `verter_workspace/src/workspace_snapshot.rs:22` | `use crate::resolver::{IdeProjectCompilerOptions, ProjectResolver, WorkspaceAlias};` | repoint |
| `verter_workspace/src/traits.rs:1222` | `impl ResolverSnapshot for EmptyResolverSnapshot` | verify unaffected; `ResolverSnapshot`/`WorkspaceRead` stay workspace-owned (Fork 2) |

`crate::resolver` inside `verter_workspace` (S1): **116 lines / 33 files — 81 production across 16
files**: `engine.rs` (32), `resolution_currency.rs` (18), `config.rs` (4), `filesystem.rs` (4),
`snapshot_builder.rs` (4), `project_graph.rs` (3), `relative_path.rs` (3), `workspace_snapshot.rs`
(3), `membership.rs` (2), `vite_config.rs` (2), singles in `carrier_discovery.rs`, `env_hash.rs`,
`memory.rs`, `project_key.rs`, `traits.rs`, `virtual_config.rs`. 35 lines across 17 `*_tests.rs`
files are the separate test disposition.

`WorkspaceSnapshot.resolver` consumers: production `engine.rs` (6),
`verter_lsp/src/server/provider_state.rs` (4), `verter_lsp/src/workspace_scanner.rs` (4),
`verter_session/src/host_construction.rs` (1), `resolution_currency.rs` (1); test
`resolution_currency_contract_tests.rs` (1).

### A3.3 — The two-hop glob chain

`verter_lsp/src/project_resolver.rs` is one line — `pub use
verter_semantic::analysis::project_resolver::*;` — over a module that itself glob-re-exports
`verter_workspace::resolver::*`. **149 lines across 18 LSP files (77 production) resolve through
it**, and deleting the semantic `:1-30` re-export half breaks all of them simultaneously; the plan
listed the shim repoint and the re-export deletion as two unrelated rows when they are one event.
Production consumers: `config.rs` (24), `server_utils.rs` (23), `workspace_scanner.rs` (18),
`server/sync_orchestration.rs` (6), `server/mod.rs` (2), `test_utils.rs`, `provider_sync.rs`,
`external_ts/carrier_sync.rs`, `carrier_provider_projection.rs`.

A second glob-import site exists outside LSP: `verter_session/src/cross_file.rs:634`
(`use verter_semantic::analysis::project_resolver::*;`, function-scoped).

`analysis::project_resolver` consumers beyond LSP: `verter_napi/src/lib.rs` (7), `verter_napi/src/
meta.rs` (1), `verter_wasm/src/lib.rs` (2), production `verter_session` (`meta.rs`,
`host_lifecycle.rs`, `component_meta_host.rs`, `cross_file.rs`, `external_ts/mod.rs`,
`external_ts/resolver.rs`), ~35 session test files, 4 `verter_bench` targets.

### A3.4 — Storage fields outside the owner crate

`verter_lsp/src/server/mod.rs:199` (`pub(crate) resolver: crate::project_resolver::
NativeProjectResolver` — `ServerState`); `verter_lsp/src/external_ts/carrier_sync.rs:99`
(`pub resolver: &'a NativeProjectResolver` — `CarrierSyncRequest`);
`verter_lsp/src/workspace_state.rs:285` and `workspace_scanner.rs:2218,2243,2278`
(`ProjectResolver::default()` construction); `verter_lsp/src/test_utils.rs:190,211`.
Reference-typed parameters at `server_utils.rs:276,285,292,859` and `provider_sync.rs:482,534` — the
intentionally-unused `_resolver` ones are deleted with their call-site arguments, not retyped.

### A3.5 — Test-side disposition

Test weight concentrates in `verter_workspace/src/resolver_tests.rs` (76 `ProjectResolver` lines,
3,929-line file — move and parameterize) and `resolution_dual_runner_tests.rs` (8 — deleted with the
cutover). `resolution_witness_contract_tests.rs` (3) is PRESERVED as public-boundary
characterization. `verter_session/tests/cases/compile-fail/raw_resolver_entry_points_are_private.rs`
(2) is RETARGETED to the new private attempt boundary.

Out of scope, confirmed distinct types sharing the substring: `WorkspaceProjectResolver<'a>`
(`verter_session/src/external_ts/resolver.rs:341`), `LspProjectResolverReader<'a>`
(`verter_lsp/src/server_utils.rs:363`), `ExternalTsProjectResolver` (Project-Bound External-TS
Contract). A substring rather than word-boundary match would have pulled 103 lines of unrelated work
into the cutover.
## A4 — An inventory change invalidates ratification

§A3 is ratified content, not a working note.

1. Step 0 re-reads the re-export block at `verter_workspace/src/lib.rs:183-190` (the S6 name list is
   derived, never hand-kept), then re-runs **every sweep named in §A2's table — S1 through S7, by
   name, not by count** — and diffs the result against §A3.
2. **Any difference — a new file, a new symbol, a changed production/test classification, a changed
   count, or a name added to the re-export block — invalidates this annex's ratification.** Execution
   STOPS. It is not a step-0 amendment and not an executor judgment call.
3. Resuming requires: §A3 updated to the observed tree, a NEW `sha256`, and re-ratification by the
   same architect seat under whatever activation path the escalated governance question settles on
   (see Status). The superseded digest is never re-used, and the absence of a registration mechanism
   today is a reason this annex cannot yet activate — never a reason to skip re-ratification.
4. A step-0 run that finds no difference is recorded with the sweep outputs and the `$BASE` observed
   under §A1. That record is the evidence Stage 2 was executed against the ratified inventory.
5. Because the tree moves, §A3 is expected to drift. Drift is normal and is not a defect; **executing
   against drifted content is the defect.** The rule converts inventory drift into a stop, not into a
   silent edit.
6. **This clause binds the procedure §A2 actually defines.** A re-ratification trigger that names a
   different or stale procedure — a wrong sweep count, a sweep set the annex no longer contains — is
   a gate on nothing. If §A2's sweep set changes, this clause changes in the same revision.

§A4 governs the INVENTORY. It does not govern completeness, which §A5 owns, and a green §A5 gate
never substitutes for a §A4 re-ratification.

## A5 — Completeness rests on the build, not on the sweep

### The reframing

Successive revisions were spent trying to make a static artifact carry a completeness guarantee —
first a name sweep, then a table of build lanes. Neither can, and — this is the part that was
missed — **neither ever had to.**

The plan treated a proven-complete inventory as a PRECONDITION for an irreversible step. But the
irreversibility is in the **landing**, not in the editing. §4 already establishes that every
intermediate state is unlanded scratch and that `git reset --hard c1-stage2-prestart` returns the
tree to known-good. Therefore:

> **A missed reference is a compile error in a scratch tree. It costs rework, not damage.**

That is the whole dissolution. The inventory's job is effort estimation and sequencing. **Safety
already comes from the abort procedure; completeness comes from the build.**

### The proof obligation, stated negatively

Stage 2 does not need to enumerate every reference in advance. It needs to prove, **at the end**,
that none remains. That proof is a green build at execution time, with the old surface deleted.

Everything in this section is subordinate to that sentence. The lanes below are not the proof and
are not an approximation of it; they are the work an executor can do *before* execution to make the
first build rounds cheap.

### What the lanes state, and what they deliberately do not

**Positively, and this is the whole claim:** each lane's selection is **exact and checkable** — its
triple, its target kinds, and its package set are all determinable by reading the invocation, with
no judgement required. Their union is therefore a **precise lower bound on what compiles before
execution.** A lower bound is a genuinely useful object: it tells an executor what is already
guaranteed to have been compiled by the time the first build round starts, and it is verifiable by
anyone, at any time, without running anything.

**What the table does NOT do is claim coverage.** A cargo invocation selects a point in
`triple × kinds × packages`; a static composition of invocations spans a subset of that product, and
no amount of adding lanes closes it. The governing rescope ruling already settled this shape:
completeness rests on semantic/compiler reference analysis and build gates, with the static artifact
explicitly non-exhaustive. **A lane table asserting it selects every compiled cell is the struck-down
claim in a different currency** — where the lexical sweep claimed to enumerate every reference, a
coverage-claiming table would claim to select every configuration. The table below is written as a
statement of selection precisely so it cannot be read that way.

| Lane | Triple | Target kinds selected | Packages selected |
|---|---|---|---|
| `cargo check --workspace --all-targets` | host | lib, bins, unit tests, integration tests, benches, examples | every workspace member |
| `cargo clippy --target wasm32-unknown-unknown -p verter_wasm --all-targets -- -D warnings` | `wasm32-unknown-unknown` | all kinds **for `verter_wasm` only**; every dependency compiles as **lib only** | `verter_wasm` selected; `verter_session`, `verter_workspace`, `verter_semantic` reached as dependency libraries |
| `cargo clippy --workspace --all-targets -- -D warnings` | host | lib, bins, unit tests, integration tests, benches, examples | every workspace member |
| `cargo check --workspace --release` | host, release cfg | lib and bins only — no `--all-targets` | every workspace member |
| `node scripts/gate.mjs` | host | every workspace test target in the nextest archive | every workspace member |
| the Cargo-metadata production-closure gate (`crates/verter_identity/tests/cases/workspace_dependency_layers.rs`, A5-DD1 row deleted) | host | none of its own — an integration test compiled by the lanes above; it reads `cargo metadata` rather than compiling the graph | — |

The closure gate is listed for completeness of the *procedure*, not of the compiled universe: it
establishes that the `verter_semantic → verter_workspace` edge is gone semantically and that the
positive `workspace → semantic` edge exists.

### The worked example of why coverage cannot be claimed

This is the clearest demonstration in the tree, and it belongs in the record as the reason the
table is shaped the way it is:

```
crates/verter_session/src/meta_tests.rs:999    #[cfg(target_arch = "wasm32")] #[test]
crates/verter_session/src/meta_tests.rs:1030   crate::resolver_core::FactVersionRef::FileWholeHash
```

`FactVersionRef` is a symbol Stage 2 expressly relocates (§A3.1; the F26 disposition moves its whole
immutable value graph into `verter_semantic::facts`). That reference is:

- **excluded from every host lane** by `#[cfg(target_arch = "wasm32")]`; and
- **excluded from the wasm lane** because `--all-targets` applies to `-p verter_wasm`, while
  `verter_session` is reached as a dependency **library** — its test targets are never built.

So a real reference to a relocated symbol is selected by no lane. It is not an oversight in the
table; it is a cell of `triple × kinds × packages` that no composition of these invocations reaches.
And it is not a singleton. Derivation, so this is checkable in one step rather than taken on trust:

```
grep -rn 'cfg(target_arch = "wasm32")' crates/verter_session --include='*_tests.rs' | wc -l   # 28
grep -rln 'cfg(target_arch = "wasm32")' crates/verter_session --include='*_tests.rs' | wc -l  # 5 files
grep -rn 'cfg(target_arch = "wasm32")' crates/verter_session/tests --include='*.rs' | wc -l   # 3
```

**28 sites across 5 `*_tests.rs` files, plus 3 more under `crates/verter_session/tests/`.** Only a
small number of those directly gate a `#[test]` function — the worked example above is one; the rest
gate imports, helpers and inner blocks inside test modules. That distinction does not change which
cell they occupy: every one of them compiles only under `wasm32` **and** only when this crate's test
targets are built, which is exactly the intersection no lane selects.

### Why selection-as-lower-bound can carry its claim, when the previous two could not

Two components in this plan's history asserted completeness and failed the same way: the lexical
name sweep, and the coverage-claiming version of the lane table. Any component proposed in their
place owes an answer to the question neither of them was ever asked — **why can this one carry its
claim?** For the lane table as now written, the answer is a specific asymmetry, and it is checkable
rather than rhetorical.

**A coverage claim is universally quantified over a space nobody can enumerate.** "For every
reference in every configuration, some lane compiles it" ranges over
`triple × kinds × packages × features`. Verifying it requires enumerating that product; falsifying it
requires exactly one counterexample. **A claim that is cheap to falsify and infeasible to verify will
be falsified repeatedly and can never be established.** That is not a description of bad luck across
five rounds — it is the predictable behaviour of a claim with that logical shape, and both dead ends
had it. The sweep's counterexamples were `workspace_snapshot.rs:46`, `verter_mcp`, an inferred field
access, a glob re-export; the table's were a triple, then a kind, then a package. Different
currencies, one shape.

**A lower-bound claim inverts both costs.** "These invocations select at least these cells" is
verified by *reading the invocations* — finite, mechanical, no judgement, no enumeration of the
codebase at all. To falsify it you must show that a lane does not select what its row says, which is
a statement about the command, not about the repository.

Three consequences follow, and they are why this is a durable component rather than a better-worded
version of the same mistake:

1. **Its truth does not depend on the codebase.** A new `#[cfg(target_arch = "wasm32")]` test in
   `verter_session` instantly falsified the coverage claim. It does not touch the lower bound: "the
   wasm lane selects all kinds for `verter_wasm` and libs for its dependencies" remains exactly as
   true afterwards. **A claim whose truth is independent of what it describes cannot rot** — and
   rotting silently is precisely how the previous two components failed between one review and the
   next.
2. **It is verifiable by anyone, at any time, without running anything.** No build, no tooling
   availability question, no privileged access to a machine under memory pressure.
3. **It composes with the build-as-oracle principle instead of competing with it.** A coverage claim
   tries to *substitute* for the oracle by asserting statically what only compilation can establish.
   A lower bound *feeds* the oracle: it states what is already guaranteed compiled when the first
   build round begins, which is exactly the role a pre-execution artifact can legitimately hold.

The same test applied to §A3 gives the same verdict for the same reason: the inventory is retained
because "these sweeps found these occurrences" is checkable by re-running them, and it is labelled
non-exhaustive because "these are all the occurrences" is not. **The distinction that survives is
between what an artifact FOUND and what an artifact CLAIMS EXISTS** — the first is a lower bound, the
second is a coverage claim, and only the first can be carried by a static document.

### Known-uncovered cells — ILLUSTRATIVE EXAMPLES, NOT AN INVENTORY

**Read this heading literally.** What follows is a set of examples found incidentally while
constructing the lanes. **It is not a list of the residual, and its length carries no information
whatsoever about the residual's size.** Six items appear below because six were noticed, not because
six exist. Concluding "the gap is small because the list is short" would be exactly the inference
this section is written to prevent.

1. Test kinds of dependency packages under a non-host triple — the worked example above.
2. Crates outside `verter_wasm`'s wasm32 closure: `verter_lsp`, `verter_mcp`, `verter_napi`,
   `verter_tsc`.
3. The separate `extensions/lapce` and `extensions/zed` manifests (`wasm32-wasip1` / `wasip2`).
4. Non-default feature combinations (`currency_probe`, `hotpath`).
5. The `no-debug-assertions` profile lane, which `scripts/gate.mjs` currently skips.
6. Test, bench and example kinds under the release configuration — the release lane is lib+bins.

### The residual is unbounded until execution, by design

**This is a design property, not an omission, and not a shortfall to be closed later.** Because the
residual cannot be bounded statically, it is not bounded statically. The two things that make that
acceptable are already in place and do not depend on the table at all:

- **Completeness comes from the build at execution.** With the old surface deleted, any surviving
  reference in any configuration that actually gets compiled is a compile error. The compiler sees
  inference, glob re-export, and target-gated code by construction — they are how it resolves names.
- **Safety comes from §4's abort procedure.** Every intermediate state is unlanded scratch, and
  `git reset --hard c1-stage2-prestart` returns the tree to known-good. **A missed reference is a
  compile error in a scratch tree: rework, not damage.**

Do not attempt to close the residual by adding lanes, flags or packages. That move produced four
successive revisions of this section, each fixing one cell of `triple × kinds × packages` while
leaving the shape intact — and the sixth cell was found by the discipline itself, not by a reviewer,
which is what finally showed that the shape, not the coverage, was the defect.

### The engineering caveat that makes this a loop, not a command

`cargo` does not check dependents of a crate that failed. With the resolver surface deleted,
`verter_semantic` fails first and `verter_session` / `verter_lsp` / `verter_mcp` / `verter_napi` /
`verter_wasm` / `verter_tsc` are never reached. **So a single red run is not an enumeration — it
shows only the lowest failing layer.**

**This is corroborated in-tree, not merely reasoned.**
`docs/arch/architecture-lock/ledger/A1/command-proofs/02-cargo-clippy.txt` records a full-workspace
invocation terminating at `error: could not compile verter_session (lib) due to 7 previous errors`
with `EXIT: 101` — the run never reached `verter_session`'s leaf dependents. The cascade is an
observed property of this repository's build, so the fixpoint loop is a requirement rather than a
precaution.

The gate is therefore a **fixpoint loop**:

1. `cargo check --workspace --all-targets --message-format=json`, collect all diagnostics.
2. Repoint what that round revealed.
3. Repeat until green, then run the remaining lanes in the §A5 table — including the wasm32 lane,
   which is a separate invocation and cannot be folded into the workspace loop.

Errors surface in dependency order — semantic, then workspace, then session, then the leaf consumers.
**Termination is green across every lane named above.** That discharges this block's gate
obligations and the pre-execution lower bound; it is not by itself a completeness proof, for the
reason §A5 gives. No intermediate round claims completeness either.

One honest consequence, stated rather than left implicit: a configuration compiled by no lane here
AND by no CI job is a configuration **the project does not build at all**. A stale reference living
only there is invisible to the project as a whole, not merely to this plan — and it stays invisible
until someone adds that configuration to CI, at which point it surfaces as an ordinary build break.

This is precisely where §A3 earns its keep: it lets the executor repoint in bulk per round instead of
discovering one layer at a time. That is an **efficiency** argument, not a safety one, and the
distinction is the lesson of these revisions.

### Optional: semantic reference enumeration (planning aid, not proof)

A semantic reference query — rust-analyzer's `textDocument/references` driven headlessly over the 13
items plus the `WorkspaceSnapshot.resolver` field — would close §A2's inference blind spot *during
planning*, tightening the estimate before any edit. It is explicitly **not** the proof; §A5's gate
is. Whether the tooling is available here is an environment question, deliberately not assumed.

### Validation status of this section

Two claims, with different evidentiary standing, deliberately not merged:

- **The cascade premise is CORROBORATED** by the in-tree command proof cited above. It required no
  new build.
- **The loop's full operational behaviour is DESIGNED, NOT EXERCISED** — that per-round diagnostics
  arrive in dependency order across every layer, and that each named lane selects on this tree exactly
  what the table states. Exercising it end-to-end is Stage 2 execution itself, which governance places after
  ratification, so it is not a precondition of ratifying this design.

The wasm32 lane's dependency closure is accepted on MANIFEST evidence — `verter_wasm` depends on
`verter_session` and `verter_semantic`; `verter_session/Cargo.toml:132` depends on
`verter_workspace` — and its empirical confirmation is deferred to execution rather than made a
ratification precondition. Its target kinds are likewise read from the crate layout: `src/lib.rs`
plus `tests/main.rs`, with no bins, benches or examples declared.

No build is required before dispatch. Should one become necessary later, it **must be queued by the
orchestrator, not started opportunistically** — a build begun under host memory pressure is itself
an abort trigger (§4, AB-8).
