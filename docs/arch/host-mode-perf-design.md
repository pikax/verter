# Verter Host-Mode + verter-tsc `--api` Perf Design (RATIFIED, codex-validated)

Status: RATIFIED — binding design of record (user-ratified, codex-validated gpt-5.5 xhigh). PERF-0 lands the parity oracle this design mandates first.
Source: throwaway worktree `refactor/external-ts-engine` @ `0dde5f6228` (Block-5/6 tree with `verter_tsgo_api` + evolved `verter_tsc`).
Architecture authority: codex (gpt-5.5 xhigh), neutral/unprimed ruling — verdict and ten enumerated findings recorded in §6 (full review transcript archived off-tree).
Hard rule: Shared Optimized Codebase — ONE host/parse/resolve/cache substrate; the modes select PRESETS + resource policy off that one substrate, they do NOT fork an engine.

## 0. Problem statement (cost grounding)

- `vize`/Corsa (fastest TS7 batch reference) cold ≈ **1.77 s / 1060 files** (collect 361 ms, gen 183 ms, corsa 1.19 s). Consumed from the live baseline diagnostic (agent `ab355feab7daf435e`); not re-run.
- RAW `verter_compiler::compile` (rayon, no host) ≈ **4.6× faster than vize** cold → the compiler is NOT the bottleneck.
- The gap is (a) **host/session/scheduler overhead** (~96% of cold `compileMany` wall) and (b) the **verter-tsc temp-file harness** (~38–41% of cold verter-tsc wall: temp dir + `.vue.ts`/`.tsx` writes + `tsgo --project` subprocess + 100 ms poll).

Directed work: **Fix 1** (verter-tsc → in-memory tsgo `--api`, delete temp files), **Fix 3** (the host overhead — a 3-mode capability the user named `HostMode::{Full,Batch,Bare}`). Fix 2 (two-files surface) DEPRIORITIZED → re-check after 1+3 (PERF-5).

## 1. Substrate today (what the modes select over)

FOUR orthogonal per-request gating axes ALREADY exist — the modes must compose these, NOT add a redundant fifth:

| Axis | Where | Gates |
|---|---|---|
| `CompileTarget` (`compile/types.rs:17`) | per-compile | codegen steps — `BUNDLER`/`IDE`(=TSX)/`ANALYSIS` |
| `AnalysisScope` (`analysis/scope.rs:18`) | per-upsert | analysis passes — `BUILD`/`BUILD_OPTIMIZED`/`LSP`(=all)/`LINTER`/`ESSENTIAL`/`NONE`. Script bits = correctness; **note `BUILD` already includes `STYLE_VBIND`+`STYLE_SCOPED`** |
| `QueryProfile` (`profile.rs:16`) | per-session | prewarm/latency/cross-file policy — only `LspInteractive` defers cross-file. **`Build` profile OMITS style bits (`profile.rs:87`) — diverges from `AnalysisScope::BUILD`** |
| `ExecutionMode{Interactive,Batch}` (`meta.rs:413`) | per-session | component-meta FAN-OUT only (`HostCpuPool`/`HostBatchCoordinator`) — NOT a subsystem gate |

Host CONSTRUCTION cold cost is dominated by **unconditional thread-pool spawning** (P = `available_parallelism()`): driver(1) + `SchedulerCpuPool`(P) + `SchedulerIoPool`(4) + `HostCpuPool`(P, 8 MiB stacks) + `DeclLoweringService`(clamp(P/4,1,4), 8 MiB stacks) ≈ **2P+5+clamp** threads (~41 on 16-core), ~160 MiB stack VM. None mode-gated. WASM runs the scheduler INLINE (zero threads) — proof pools are separable from correctness. Scheduler+`SchedulerCpuPool`+`SchedulerIoPool`+`DeclLoweringService` are cross-file-correctness; `HostCpuPool` is throughput-only.

`HostConfig::default()` = `analysis_level: Full` ⇒ effective scope `LSP`(all) ⇒ full interactive analysis eagerly on EVERY upsert (verter-tsc inherits this). Eager whole-project stub (`get_public_api`-all) lives in the LSP layer (`workspace_scanner.rs:326`), NOT the host — `compile_many` is already demand-driven. Type-error diagnostic SET is produced by tsgo over Verter-generated TSX + `get_public_api` cross-component stubs — a SHARED path. With NO host, cross-component imports degrade to a `DefineComponent<{},{},any>` wildcard shim (compile-correct, NOT cross-file-typecheck-correct).

Two live drift bugs found during review (fold into PERF-2): `query_profile` is hardcoded to `Build` at `host_construction.rs:328`; `HostConfig::from_query_profile()` only sets `analysis_scope`, not the profile (its doc claims both).

## 2. The codex-validated design: presets + resource policy + a sibling Bare façade (NOT a stored enum)

The user named a `HostMode::{Full,Batch,Bare}` axis. The codex ruling **preserves the user's hard constraint (ONE substrate, no forked path) and the three conceptual modes, but corrects the MECHANISM**: do NOT store a `HostMode` enum as a fifth core axis (it would drift against the four existing ones). Instead:

- **Full** and **Batch** are **named `HostConfig` constructors/presets + explicit resource policy** over the existing axes (`AnalysisScope`, `QueryProfile`, `CompileTarget`, pool sizing, audit). Both build the SAME `VerterHost`/`ProjectTypeStore`/resolver/scheduler — same engine, different presets.
- **Bare** is **NOT a host mode**. It is a separate stateless **`BareCompiler` façade** over the lower owner `verter_compiler::compile`. `VerterHost`'s scheduler/cache/resolver/pool fields are unconditional (`lib.rs:445/670/685`); `Option`-izing them to fake a Bare host would poison every host method. Bare honors Shared-Optimized-Codebase precisely because it delegates DOWN to the existing no-host compiler path (already the 4.6×-vize raw path), not because it is a host variant.

### Construction API (binding)

```rust
// crates/verter_session/src/types.rs — presets, NOT a stored mode enum
impl HostConfig {
    pub fn lsp_interactive() -> Self;   // = today's Full: analysis_scope LSP(all), interactive query profile, full pools
    pub fn batch_typecheck() -> Self;   // analysis_scope = carrier-affecting facts (see §3), QueryProfile::Build,
                                        //   audit off, resource policy: lazy/right-sized HostCpuPool + decl-lowering
}
// Explicit RESOURCE POLICY fields (not mode checks): pool sizing + laziness.
pub struct HostResourcePolicy {
    pub host_cpu_pool: PoolPolicy,        // Eager(P) | RightSized(n) | LazyOnFirstUse
    pub decl_lowering: PoolPolicy,        // Lazy/configurable
    // scheduler correctness pools stay for any session-bearing host
}
```

```rust
// NEW sibling — crates/verter_compiler (lowest owner) or a thin crate; NOT VerterHost
pub struct BareCompiler { /* stateless */ }
impl BareCompiler {
    pub fn compile_sfc(&self, source: &str, profile: &CompileProfile) -> CompileResult; // -> verter_compiler::compile
    // REJECTS (compile-time / typed error): cross-file macro resolution, get_public_api,
    //   component-meta, type-provider diagnostics — anything reading another file.
}
```

`ExecutionMode{Interactive,Batch}` (`meta.rs:413`) is renamed/documented `MetaExecutionMode` (component-meta fan-out only); host/session profile selection threads through `HostConfig` + scheduler config, NOT this enum.

### Per-mode subsystem table (preset/policy view)

| Subsystem / lever | Full (`lsp_interactive`) | Batch (`batch_typecheck`) | Bare (`BareCompiler`) |
|---|---|---|---|
| `VerterHost` (one substrate) | yes | yes (SAME host) | **none** |
| Type/semantic resolution + cross-file frontier | yes | yes | **no** |
| Warm cache (`ProjectTypeStore`/`FileArtifactStore`) | yes | yes | **no** |
| `AnalysisScope` preset | `LSP` (all) | **carrier-affecting facts** (§3) | n/a |
| `QueryProfile` | interactive/background | `Build` | n/a |
| Eager interactive analysis (template/cross-render for hover/completion) | yes | **no** (on demand) | no |
| Style/v-bind facts (affect carrier bytes) | yes | **YES — kept** (§3) | n/a (single-file codegen keeps them) |
| Scheduler pools (driver+cpu+io) | yes (P) | yes (right-sized) | **none** |
| `HostCpuPool` | eager(P) | **lazy/right-sized** | none |
| `DeclLoweringService` | eager | **lazy/configurable** | none |
| In-process `TypeProvider` + sync machinery | yes | **no** (one-shot tsgo `--api`) | no |
| Eager whole-project stub | LSP layer | **no** (demand) | no |
| component-meta / fallthrough / `get_public_api` | yes | yes (on demand) | **no** (rejected) |

## 3. Correctness boundary (codex-corrected)

- **Full ↔ Batch — SAME carrier bytes AND SAME tsc-parity diagnostic SET.** Codex CORRECTION: Batch is NOT "drop style/template analysis." Style `v-bind` feeds the generated carrier (`virtual_file_pipeline.rs:1067`, `ide/script/setup.rs:725`) — dropping it changes carrier bytes ⇒ different diagnostics. **Batch keeps EVERY fact that affects carrier bytes OR TS diagnostics; it drops only purely-interactive analysis** (template-binding occurrences for hover/highlight, completion inputs) and the interactive provider/sync machinery. The `AnalysisScope::BUILD`(incl. `STYLE_VBIND`/`STYLE_SCOPED`) vs `QueryProfile::Build`(omits style) mismatch MUST be reconciled in PERF-2: the Batch preset uses a scope that includes all carrier-affecting facts. Guard: byte-parity tests on generated TSX + public-API stubs and diagnostic-set parity (the perf gate's axis-B "exit code + diagnostic set match" already exercises this; PERF-0 hardens it).
- **Bare — stateless single-SFC, correctness-safe IFF output is a pure function of (single SFC source + profile)** with NO imported-symbol resolution. Covered: bundler SFC→render fn (types erased) + standalone SFC→TSX fed to an external checker that does its OWN cross-file resolution. NOT covered (Bare rejects): `get_component_meta`, fallthrough, `resolve_type`, `get_public_api` with imported macro types. Precise Batch↔Bare line: with no host, cross-component imports degrade to the `DefineComponent<{},{},any>` wildcard shim ⇒ **Bare is compile-correct but NOT cross-file-typecheck-correct, so verter-tsc MUST run Batch, never Bare.**
- The LSP-published lint + component-usage diagnostics (`server_utils.rs:1256–1296`) are interactive IDE extras, NOT part of the tsc-parity set — Batch omitting them is not a regression vs `tsc`.

## 4. Fix 1: verter-tsc → in-memory tsgo `--api`, delete temp files

Current (`crates/verter_tsc/src/checker.rs`): `VerterHost::new_standalone(HostConfig::default())` → upsert all `.vue` → `generate_public_api_stubs` (`get_public_api` per file, `fs::write` `.vue.ts`) + `generate_all_tsx` (`fs::write` `.tsx`) + 3 ambient `.d.ts` + synthetic `extends` tsconfig (`write_temp_tsconfig`) → `tsgo --project` subprocess (`invoke_checker`, 100 ms poll) → `parse_tsc_output` → `remap_diagnostics` via inline sourcemap (line/col).

Target (codex-refined):
1. Run verter-tsc host as **Batch** (`HostConfig::batch_typecheck()`).
2. **Backend = `verter_tsgo_api::TsgoClient` (`--api`-only, `client.rs:46`) with `OverlaySnapshot` off-disk carriers** — NOT the owned dual-surface `TsgoOwnedProvider` (it spawns an unused `tsgo --lsp` interactive surface for a batch CLI, and its `semantic_diagnostics_for_carrier_in_project` returns SEMANTIC-only with documented per-carrier parity gaps, `owned.rs:14,168`). Collect ALL diagnostic categories `tsgo --project` surfaces for the project — semantic + syntactic (`get_semantic_diagnostics` + `get_syntactic_diagnostics` cover the per-file classes) PLUS any options/config/global diagnostics `tsgo --project` emits (these are NOT covered by the two per-file getters and must be collected separately). PERF-3 MUST prove FULL-category parity against the PERF-0 diagnostic-set oracle (which pins whatever the current `tsgo --project` emits) before landing.
3. Feed carriers (TSX + public-API stubs + ambient shims) as in-memory overlay snapshot entries — **no temp files** (typecheck path).
4. Project model: **RETAIN a synthetic tsconfig** (the same compiler-options overrides — `jsx: react-jsx`, `jsxImportSource: vue`, `allowImportingTsExtensions`, `include: []` + explicit `files`, `extends` the user config — that the temp-file path built, so membership + options are byte-identical to the pinned oracle), but serve it IN-MEMORY as an overlay entry at a deterministic in-project virtual path with deterministic in-project virtual carrier names. The overlay materialization REUSES `verter_workspace::tsgo_virtual_config` (`build_virtual_overlay_snapshot`, relocated there in PERF-3-pre). Keep `get_public_api` cross-component stubs; **do NOT fall back to the wildcard shim for in-project Vue imports**. (Correction: `augment_tsconfig_bytes` companion-injection alone is INSUFFICIENT — verter-tsc needs the compiler-options overrides, so it builds the full synthetic config via one shared `synthetic_tsconfig_value` builder and serves it through `build_virtual_overlay_snapshot`.)
5. Map the engine's diagnostics — **UTF-16 code-unit offsets** (`pos`/`end`, TS position semantics; NOT UTF-8 bytes — an em-dash `—` in the carriers' comments drifts a byte reading by a line) → generated (line,col) → original `.vue` via the existing inline-sourcemap decode (`error_map.rs`) + the SHARED UTF-16-offset conversion at the `verter_tsgo_api` boundary (`api_offset_to_line_col`, backed by `verter_span`; the former `verter_tsc::offset_map` duplicate is retired). Collect the WHOLE PROGRAM, not just `root_files`: one file-omitted `getSemanticDiagnostics` + one file-omitted `getSyntacticDiagnostics` (which reach every program file, INCLUDING non-root imported `.ts`) PLUS `getConfigFileParsingDiagnostics` (options/config/global). Each diagnostic is homed by its OWN `file_name`: an overlay carrier remaps through its source map; a real non-root file is a passthrough at its own position; a `fileName:None` global diagnostic is retained under a synthetic label. Config diagnostics are filtered through `strip_injected_root_diagnostics` (fail-closed: only KNOWN injected companions dropped; real-file + global diagnostics retained). This is whole-program parity with the old `tsgo --project --noEmit` (and vue-tsc).

**Engine:** the gated `TsgoClient` connects to the version-pinned GA `@typescript/typescript-<platform>` native engine (the wire-gate pin, `typescript@7.0.2`), NOT the native-preview `tsgo`. Discovery precedence: `VERTER_TSGO_BIN` override → the rc engine in the project's `node_modules` (shared `verter_tsgo_api::discover_tsgo`). There is NO tsc fallback for the typecheck path.

DELETE (typecheck path ONLY): the temp-file `--noEmit` flow — its `TempDir`, `fs::write` carrier/shim sites, and the `write_temp_tsconfig`/`invoke_checker`/`parse_tsc_output` calls **for validation** — plus the `--use-tsc` CLI flag and the `TypeCheckerBinary`/`ForceTsc` enum. New deps: `verter_tsgo_api` + `tokio` (one-shot `block_on`). **KEEP** (correction — the temp-file mechanics are NOT dropped): `write_temp_tsconfig`, `invoke_checker`, `parse_tsc_output`, `remap_diagnostics`, `TempDir`, and the `tempfile` dep are RETAINED for the `--declaration` emit stage, which stays on the temp-file `tsgo --project` path PERMANENTLY (tsgo `--api` exposes NO emit surface — spike-proven).
**GATE: land Fix 1 only AFTER PERF-0 parity passes.** If `--api` parity cannot match `tsgo --project`, KEEP the current checker until the API gap is closed (do not ship a regressed diagnostic set). **LANDED:** the in-memory `--api` backend runs whole-program on the rc engine — Rail B `PARITY OK: 71` (the 71st is a non-root imported-file diagnostic the earlier per-root path dropped) + exit 1; the Rail B preflight resolves + threads the rc engine via `VERTER_TSGO_BIN`. Whole-program collection (file-omitted semantic + syntactic + `getConfigFileParsingDiagnostics`) closed the earlier per-root scope gap and the config/options gap.

## 5. Other bottlenecks (owner layer + rough win)

1. **Eager thread-pool spawn at host build** — `host_construction.rs` (`HostCpuPool`:301, `DeclLoweringService`:377, scheduler pools:219). ~2P+5+clamp threads + ~160 MiB stack, unconditional, per verter-tsc invocation. Owner: `verter_session` + `verter_scheduler`. Fix: **lazy/right-sized via resource policy** (not mode checks) — lazy `HostCpuPool`, lazy/configurable decl-lowering; KEEP scheduler correctness pools for session-bearing hosts; do NOT collapse scheduler+host pools (deadlock class). #1 host-overhead lever.
2. **Default `Full` analysis on verter-tsc upsert** — `checker.rs:269` + `effective_scope()=LSP`. Discarded interactive analysis on every `.vue`. Owner: `verter_session` upsert + `HostConfig`. Fix: Batch preset (carrier-affecting facts only).
3. **verter-tsc temp-file harness** — Fix 1 (38–41% of cold verter-tsc wall).
4. **`get_public_api` per-file cross-file macro sync** — `virtual_file_pipeline.rs:1930` (`sync_transitive_macro_type_dependencies`). Cached by whole-hash; inherent to a full-project check but must run at Batch scope + stay demand-driven (one import level at a time).
5. **Per-file canonicalize / path normalization** — verter-tsc `\`→`/` per upsert + `.canonicalize()` in remap. Owner: verter-tsc + `verter_span`. Win: minor; confirm via baseline profile.
6. **Scheduler/IO pool cold setup vs a one-shot** — Bare needs none (WASM-inline precedent). Owner: `verter_scheduler`. Win: large for Bare.

## 6. Codex architecture verdict (binding ruling)

VERDICT: **Do NOT store `HostMode::{Full,Batch,Bare}` as a fifth core axis. `VerterHost` stays the single session/cache/resolve substrate; Full + Batch are named `HostConfig`/session PRESETS over existing axes + explicit resource policy; Bare is a SEPARATE stateless `verter_compiler::compile` entry point, not a host mode.** Batch is directionally correct but the boundary must be "all facts that affect carrier bytes or TS diagnostics" (style `v-bind` included), proven by parity tests. Fix 1 targets the lighter `--api` (`TsgoClient`) path, only after parity proof.

Enumerated findings (codex): (1) HostMode would drift — use `HostConfig::batch_typecheck()`/`lsp_interactive()`; fix the `query_profile`-hardcoded-`Build` + incomplete `from_query_profile` bugs. (2) `ExecutionMode` is fan-out-only — rename `MetaExecutionMode`, don't reuse for host construction. (3) Bare outside `VerterHost` (`BareCompiler` façade rejecting cross-file ops). (4) Thread pools via resource policy/laziness, not mode checks; keep scheduler pools; don't collapse pools. (5) Batch boundary unsound as first written — `AnalysisScope::BUILD` includes style facts, `QueryProfile::Build` omits them; style v-bind affects carrier bytes; add carrier-byte + diagnostic parity tests. (6) Bare can't be a narrowed host (upsert always targets scheduler analysis) — must bypass `VerterHost`. (7) Fix-1 backend = `TsgoClient` overlay snapshots after parity; else keep current checker. (8) Reuse `verter_workspace::tsgo_virtual_config` (`build_virtual_overlay_snapshot`, relocated there in PERF-3-pre) — but RETAIN the synthetic compiler-options config (served in-memory), not bare companion-injection; keep public-API stubs; no wildcard-shim fallback for in-project imports. (9) Shared-substrate satisfied for Full/Batch; Bare OK only by delegating to lower owner. (10) Reorder: PERF-0 parity tests FIRST.

Relationship to the user directive: the ruling KEEPS the user's intent (one host, three capability profiles, no forked engine) and only changes the MECHANISM (presets + a sibling façade instead of a stored enum + Option-ized host fields). The single point for explicit user ratification: **Bare is a sibling stateless entry, not a mode of `VerterHost`.**

## 7. Implementation-block decomposition + ordering (codex-reordered)

- **PERF-0 — parity characterization (FIRST, blocking).** Discriminating tests: generated TSX + public-API stub BYTE parity and tsgo diagnostic-SET parity, on a hermetic fixture, fail-before/pass-after. Gates PERF-1 and PERF-3. verter_session-touching: NONE (tests only).
- **PERF-1 — Batch preset + resource policy.** `HostConfig::batch_typecheck()`/`lsp_interactive()`, `HostResourcePolicy` (lazy/right-sized `HostCpuPool` + decl-lowering), reconcile `AnalysisScope`/`QueryProfile` for carrier-affecting facts, fix the two drift bugs, rename `MetaExecutionMode`. verter_session-touching: YES.
- **PERF-2 — verter-tsc adopts Batch (keep current checker backend).** Flip `checker.rs:269` to `HostConfig::batch_typecheck()`; temp-file path UNCHANGED. Depends PERF-0, PERF-1. Isolates the host-overhead win from the backend swap.
- **PERF-3 — verter-tsc `--noEmit` typecheck → in-memory tsgo `--api`.** `TsgoClient` overlay snapshots + a synthetic in-memory tsconfig (reuse `verter_workspace::tsgo_virtual_config::build_virtual_overlay_snapshot`) + the shared `verter_tsgo_api` UTF-16 offset→line/col conversion; delete the temp-file `--noEmit` flow + `--use-tsc`/`ForceTsc`. The `--declaration` emit stage KEEPS the temp-file `tsgo --project` path permanently (tsgo `--api` has no emit surface). Depends PERF-0 (parity), PERF-2. Lands ONLY if parity holds; else hold. (LANDED: whole-program `--api` typecheck — file-omitted semantic + syntactic + `getConfigFileParsingDiagnostics` — with shared offset normalization; Rail B `PARITY OK: 71` on the rc engine.)
- **PERF-4 — `BareCompiler` sibling façade.** Stateless single-SFC over `verter_compiler::compile`, rejecting cross-file ops; wire bundler/unplugin single-file transform. Independent of PERF-1/3.
- **PERF-5 — re-check Fix 2 (two-files surface)** after 1+3 with the refreshed baseline profile.

Ordering: PERF-0 → PERF-1 → PERF-2 → PERF-3 (gated on parity); PERF-4 parallelizable; PERF-5 last.

## 8. `verter_session`-touching scope (user-confirm gate)

The standing rule requires explicit user confirmation for `verter_session/src` edits. This campaign touches:
- `crates/verter_session/src/types.rs` — `HostConfig::batch_typecheck()`/`lsp_interactive()`, `HostResourcePolicy`, fix `from_query_profile`. (PERF-1)
- `crates/verter_session/src/host_construction.rs` — resource-policy-gated lazy/right-sized `HostCpuPool` + decl-lowering; fix `query_profile` default (line 328). (PERF-1)
- `crates/verter_session/src/host_upsert.rs` — Batch preset drives carrier-affecting-only analysis on upsert. (PERF-1)
- `crates/verter_session/src/meta.rs` — rename/document `ExecutionMode` → `MetaExecutionMode`. (PERF-1)
- `crates/verter_session/src/decl_lowering.rs` — lazy/configurable worker spawn. (PERF-1)
- NONE for PERF-0 (tests), PERF-3 (verter_tsc + verter_tsgo_api only), PERF-4 (verter_compiler + a new thin crate).

## 9. Deferred / debt ledger (durable, tracked)

Cross-surface debt surfaced during the `--api`-typecheck work — now ALL **RESOLVED**, zero open perf deferrals. The first three rows (PERF-3-offset, PERF-3-rootscope, PERF-3c) were resolved by a follow-up block that landed shared `--api` offset normalization, file-omitted whole-program diagnostics, and config-parse diagnostics as ONE architecture unit. The rows are kept as the durable record of the bug + its resolving mechanism. The implicit sequencing constraint — no new `--api` diagnostic consumer should be added before the offset normalization lands — is SATISFIED: that same block landed the offset normalization first (offset boundary → whole-program surface → consumer), in that order. The trailing rows (PERF-3-D3, PERF-3-D8, PERF-3-D9) were the latent correctness/scaling holes review surfaced on that work; a subsequent cleanup block resolved each (shared diagnostic-source cache + reusable UTF-16 line index, a unified normalized injected-path key with a fail-closed map-boundary guard, and explicit fail-closed mapping errors replacing the silent position fallbacks). The rows below record each bug and the mechanism that resolved it.

### PERF-3-offset — tsgo `--api` offset semantics are UTF-16, not bytes (owned `--api` provider) — RESOLVED

- **Bug.** `crates/verter_type_runtime/src/tsgo/owned.rs::map_api_diagnostic` copied the tsgo `--api` diagnostic's `pos`/`end` STRAIGHT into `TypeDiagnostic.start`/`end`. But the rc engine's `--api` `pos`/`end` are **UTF-16 code-unit** offsets (TypeScript position semantics), while `TypeDiagnostic.start/end` is a **byte** contract. On any non-ASCII content before the diagnostic (e.g. an em-dash `—` — 3 UTF-8 bytes / 1 UTF-16 unit — in generated-carrier comments) the offsets diverged, so the owned `--api` provider drifted LSP diagnostic positions.
- **RESOLVED.** The `--api` offset WIRE CONTRACT (`pos`/`end` are UTF-16 code units) is now owned at the shared `verter_tsgo_api` boundary (`crates/verter_tsgo_api/src/offset.rs`: `api_offset_to_byte` / `api_offset_to_line_col` / `diagnostic_byte_span`), delegating the numeric conversion to the single primitive in `verter_span` (`crates/verter_span/src/tsgo_offset.rs`: `utf16_offset_to_byte` / `utf16_offset_to_line_col`). `owned.rs::map_api_diagnostic` converts UTF-16 → byte through that helper (reading the carrier content from the `--lsp` content cache); the `// TODO(PERF-3-offset)` marker is removed. There is exactly ONE offset-normalization implementation across the workspace.

### PERF-3-rootscope — `--api` typecheck queried only `root_files` (non-root imported-file diagnostics dropped) — RESOLVED

- **Divergence.** verter-tsc's in-memory `--api` typecheck (`crates/verter_tsc/src/api_check.rs::collect_diagnostics`) enumerated ONLY `project.root_files` (the generated carriers) and called the per-file `getSemanticDiagnostics`/`getSyntacticDiagnostics` on each. A real type error in a NON-root imported file (an imported `.ts` the project pulls in) was therefore never queried and never surfaced — whereas the old whole-program `tsgo --project --noEmit` type-checked the entire program and surfaced it (as `vue-tsc` does).
- **RESOLVED.** `collect_diagnostics` now issues ONE file-omitted `get_semantic_diagnostics_for_program` + ONE file-omitted `get_syntactic_diagnostics_for_program` (the spike-proven whole-program surface — the `file` argument OMITTED returns diagnostics for every program file, INCLUDING non-root imports). `map_one` homes each diagnostic by its own `file_name`: an overlay carrier remaps through its source map, a real non-root file is a passthrough at its own disk-read position (never dropped, never re-homed onto a carrier). The Rail B parity corpus gained a non-root fixture (`NonRootImport.vue` importing an erroring `src/nonRootBad.ts`) and grows to `PARITY OK: 71`.

### PERF-3c — `--api` typecheck did not collect config/options/global diagnostics — RESOLVED

- **Gap.** The two per-file getters (and equally the file-omitted whole-program getters) do NOT cover config-file-parse / compiler-options / global diagnostics (e.g. an invalid `target` → TS6046). The old temp-file `tsgo --project --noEmit` emitted them; the `--api` path collected none, so verter-tsc under-reported config/options errors vs `tsgo --project` / vue-tsc.
- **RESOLVED.** `collect_diagnostics` also calls `get_config_file_parsing_diagnostics` (the dedicated `getConfigFileParsingDiagnostics` wire method, added to the typed method set + the pinned op inventory) and folds it into the one diagnostic stream. The config stream is filtered through `verter_workspace::tsgo_virtual_config::strip_injected_root_diagnostics` — FAIL-CLOSED: only KNOWN injected companions (the generated carriers + the synthetic tsconfig) are dropped; real user-config errors and `fileName:None` global diagnostics are retained. A config fixture (bad `target` → TS6046) guards it end-to-end.

### PERF-3-D3 — `map_one`'s `(1,1)` disk-miss fallback is silently lossy — RESOLVED

- **Debt.** `crates/verter_tsc/src/api_check.rs::map_one` homed a non-root (non-overlay) diagnostic at its own disk-read position by reading `disk.read_file(file_name)` and running the shared UTF-16 offset walk; on a `read_file` MISS it fell back to `(1, 1)` — a well-defined but WRONG position that silently mis-homed the diagnostic to the file's first character. The owned `--api` provider (`crates/verter_type_runtime/src/tsgo/owned.rs::map_api_diagnostic`) had the mirror hole: a content-unavailable carrier degraded to a forged `(0, 0)` span.
- **RESOLVED.** Both silent fallbacks are replaced by an EXPLICIT, fail-closed mapping error. `map_one` now returns `Result<Option<Diagnostic>, MappingError>`; a genuine content miss (`MappingError::SourceUnavailable { file_name, diagnostic_code, origin }`) propagates through `push_mapped`/`collect_diagnostics` to a fatal `TypecheckError` (non-zero exit + stderr note) — never a fabricated position, never a silent drop. On the LSP side, `owned.rs` extracts the decision into a pure `position_carrier_diagnostics`: diagnostics-present-with-unavailable-content surfaces a `TypeProviderError` instead of the `(0, 0)` degrade. The source-map-gap `(1, 1)` for a RESOLVED carrier whose position has no matching source-map token is preserved (that is a source-map gap, not a content miss). Rail B multiset parity is unchanged (`PARITY OK: 71`); the real on-disk non-root fixture (`nonRootBad.ts`) still homes correctly through the resolver, and the explicit-miss path is covered by discriminating hermetic tests on both surfaces.

### PERF-3-D8 — injected-path identity uses exact string equality — RESOLVED

- **Debt.** `crates/verter_workspace/src/tsgo_virtual_config.rs::strip_injected_root_diagnostics` decided whether a diagnostic's `fileName` was an injected companion (a generated carrier / the synthetic tsconfig) via exact string equality (`p == name`), while `crates/verter_tsc/src/api_check.rs::map_one` homed diagnostics through a NORMALIZED key. Two different path-identity notions drove the SAME ownership decision, so a companion echoed by the engine in a different separator / drive-letter case slipped past the strip filter on a case-insensitive host.
- **RESOLVED.** ALL FOUR path-identity decisions in the `--api` diagnostic path — the config strip, the map-boundary guard, the carrier-lookup classification in `map_one`, and the per-collection source cache — are unified behind ONE filesystem-identity key. `verter_span::path::InjectedPathKey` canonicalizes (slash-normalize, lowercase drive, strip extended/trailing) then, iff the host filesystem folds case (`fs_is_case_insensitive()`), ASCII-lowercases the whole path — matching `fs_paths_equal`'s case policy, with a case-parameterized core unit-tested on every platform. `verter_workspace` exposes an `InjectedPathSet` (keyed by that key); `strip_injected_root_diagnostics` takes the set instead of exact equality. `verter_tsc` builds ONE `InjectedPathSet` and feeds it to BOTH the upstream strip AND a fail-closed map-boundary guard, and its carrier `lookup` is keyed by `InjectedPathKey` (was a divergent slash + `cfg!(windows)`-only fold that never folded on macOS and never stripped the extended-length prefix); the project-selection equality routes through `fs_paths_equal` (the same policy). Each diagnostic is tagged with its collection `DiagOrigin` (Semantic/Syntactic/Config), and the guard — keyed by the shared identity key AND origin — suppresses only a `Config`-origin diagnostic pointing at an injected companion, before any carrier remap. A `Semantic`/`Syntactic` diagnostic on a generated carrier still remaps to `.vue` (no over-suppression / false negatives). Discriminating tests cover the case/prefix-divergent carrier-classification remap, the case-divergent config reproducer, the over-suppression invariant, and path-identity folding.

### PERF-3-D9 — per-diagnostic O(n) offset walk + disk reread — RESOLVED

- **Debt.** `crates/verter_tsc/src/api_check.rs::map_one` performed, PER diagnostic, a `disk.read_file` of the target file plus a full UTF-16 scan-from-start to convert the offset. A whole-program error set over a large program re-read and re-scanned the same files repeatedly — O(diagnostics × file length) work that did not scale.
- **RESOLVED.** Two shared pieces in `verter_span`. (1) `Utf16LineIndex` scans a file's content ONCE (recording each line's UTF-16 + byte start) and answers each offset query by walking only within the containing line; its offset semantics are byte-identical to the scalar `utf16_offset_to_*` converters (same `getLineStarts` terminator set, UTF-16 columns, past-end clamp, mid-surrogate resolution), pinned by a parity test battery. (2) `DiagnosticSourceCache<S>` — a per-collection cache generic over a `DiagnosticContentSource`, keyed by `InjectedPathKey`, memoizing both hits and misses, exposing a `SourceFile` whose `Utf16LineIndex` is built lazily once on first offset demand. `verter_tsc` wires an `OverlayThenFallback` source (overlay carriers first, then the real FS via `NativeFs`) so each file resolves ONCE and its index builds ONCE per pass; `owned.rs` (a per-file getter) builds ONE index per carrier and reuses it for every diagnostic. The scalar converters are unchanged (many downstream callers), so the index is an additive substrate, not a rewrite of them. A counting-source test asserts one resolution + one index build for N diagnostics on one file.
