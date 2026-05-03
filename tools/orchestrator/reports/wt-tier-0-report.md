# Tier 0 progress report

**Worker**: orchestrator (single-worker phase per plan §1; no worktree).
**Branch**: `refactor/legacy-to-graph-dispatch-migration` (off
`refactor/semantic-db-overhaul`).
**Session window**: 2026-05-03.
**Status**: PARTIAL — five Tier 0 sub-steps committed; three remain
(0.1/0.2/0.3 require offline benchmark runs); production phase-archaeology
sweep at D111 broader regex inventoried but not swept.

## Commits landed (in order)

| Commit | Subject | Step |
|---|---|---|
| `a9ac952e` | chore(orchestrator): scaffold migration orchestrator infrastructure | infrastructure |
| `a445bde9` | docs(session): add god-module-audit run command and macro-impact inventory baseline | 0.0 |
| `6d9646f8` | docs(arch): promote host-cache-rehoming doc to active Tier 1 Step 1C spec | 0.5 |
| `<sha>` | feat(protocol): add selective component-meta proto schema + LSP method binding doc | 0.7 |
| `e2215c2b` | test(session): replace expected-corpus-test-count sidecar with EXPECTED_CORPUS_MIN constant | 0.6 (partial) |

## Plan deviations (D77 blocker)

### Deviation 1 — D100 wire-format claim contradicts repo state

**Plan claim** (revision 8 §0.3 D100 + §A SHA verification): "the repo
uses Rust-derive pattern, not codegen … `crates/verter_protocol/proto/`
contains a `verter/` subdir but **no `.proto` files** at this level —
schema is Rust-side via `#[derive(prost::Message)]`, not `.proto`-codegen."

**Actual repo state at SHA `60b1295a`**:

- `crates/verter_protocol/proto/verter/v1/component_meta.proto` exists
  (872 lines, `proto3` syntax, `package verter.v1`).
- `crates/verter_protocol/build.rs` invokes `prost_build::Config::compile_protos`
  on that file, with output included via `include!(env!("OUT_DIR")/verter.v1.rs)`
  in `crates/verter_protocol/src/lib.rs:8-12`.
- The `use prost::Message;` import the plan cites at `component_meta.rs:7`
  imports the *trait* applied to **codegen-produced** types, not to
  hand-written derives.

**Resolution applied**: Step 0.7 follows the actual repo pattern — the
new `selective_component_meta.proto` file extends the existing schema
in the same package; `build.rs` is updated to compile both `.proto`
files; the codegen output is reachable via `crate::verter::v1::*` like
the existing types. No new Rust-derive file was created. This deviation
is documented in the LSP method binding doc and in the Step 0.7 commit
message.

**Impact on Tier 1 work**: Tier 1B references `selective_component_meta.rs`
(NEW) at `crates/verter_session/src/component_meta_payload.rs`. That
file IS still NEW per the plan; Step 0.7 does not change that. What's
different is that Tier 1B consumers should import the wire types from
`verter_protocol::verter::v1` (e.g.,
`verter_protocol::verter::v1::TypeHandle`), NOT from a hand-written
`crates/verter_protocol/src/selective_component_meta.rs`.

### Deviation 2 — D111 broader sweep is large

**Plan §2.1.4 + §2.1.5**: "Tier 0 close: archaeology subset zero.
Algorithm-phase hits NOT touched."

**Actual scope at SHA `60b1295a`**: the existing
`no_phase_archaeology_in_production_code` arch guard regex catches
`d-cutover`, `post-cutover`, `pre-Phase`, `phase \d+`, `phase-\d+`,
`deleted in 5[a-z]`, `retired in`. It is currently green.

The D111 broader classifier rule (committed at
`tools/god-module-audit/README.md`) adds: `Plan §`, `rev <N>`,
`revision <N>`, `Phase <N>` (with colon-prefix algorithm-phase
disambiguator), `deleted in 5g`, `phase-archaeology`, SHA-like artifact
adjacencies.

A grep for the broader patterns over `crates/*/src/**` (excluding
`tests/` and `**/*_tests.rs`) returns **618 hits** across multiple
files. The hits are concentrated in:

- `crates/verter_lsp/src/features/hover_provenance.rs` (~13 hits)
- `crates/verter_lsp/src/server/{lifecycle,mod,nav_features}.rs` (~12 hits)
- `crates/verter_lsp/src/config.rs` (3 hits)
- `crates/verter_napi/src/{lib,meta}.rs` (~5 hits)
- `crates/verter_parser/src/utils/oxc/vue/script/resolve_type.rs` (~5 hits)
- `crates/verter_protocol/src/types.rs` (1 hit)
- `crates/verter_scheduler/src/{pool,queue,scheduler}.rs` (~6 hits)

The remainder (~570 hits) are spread across `verter_session/src/**`
(the largest crate — many sub-modules contain old plan references).

**Resolution required**: a focused sweep worker with TDD per CLAUDE.md
A5 for each rephrasing. Each hit needs the project-management vocabulary
removed while preserving the durable architecture insight (e.g.,
`// Plan §3 Commit 9 — hover.provenance opt-in.` → `// hover.provenance
is opt-in (default false).`). The sweep is scoped to a dedicated
session because it's mechanical-but-careful work across ~30 files.

**Recommendation**: dispatch a Step 0.4-bis worker (`Agent` tool,
`isolation: "worktree"`, brief: "sweep production phase-archaeology
hits per D111 classifier; preserve durable insight, remove project
vocabulary; final state must pass the existing arch guard with the
broader regex applied"). Out of scope for this orchestrator session.

## Steps remaining for Tier 0

| Step | Status | Reason |
|---|---|---|
| 0.0 | DONE | audit tool README + macro-impact inventory committed |
| 0.1 | NOT STARTED | requires extending `audit_real_component_meta` benchmark example with `bridge_max_depth_observed` instrumentation AND running it for ~hours against `.integration-tests/repos/nuxt-ui-codex-bench` to produce the 179-row CSV. Out of scope for this session. |
| 0.2 | NOT STARTED | requires per-fixture instrumentation to dump interned `SemanticQueryKey` set + `dep_signature` for 32 fixtures. Out of scope for this session. |
| 0.3 | NOT STARTED | requires substantive analysis of 23K LOC across 5 god modules (`semantic_query_memo.rs`, `resolve_type.rs`, `host_resolve.rs`, `resolver_core/component_meta.rs`, `convert.rs`). Out of scope for this session. |
| 0.4 | INVENTORIED | 618 production hits enumerated above. Inventory file commit pending. |
| 0.4-bis | NOT STARTED | sweep scope characterized; dispatch as dedicated worker. |
| 0.5 | DONE | rehoming doc rewritten to active Tier 1 Step 1C spec |
| 0.6 | PARTIAL | EXPECTED_CORPUS_MIN constant landed; clippy --workspace pinning to `tmp/tier-0-sweep-verification.txt` remains (long-running command). |
| 0.7 | DONE | selective component-meta `.proto` schema + LSP method binding doc committed. Plan-deviation note above. |
| 0.8 | DONE | D49 entry conditions verified (output captured in this report's "Tier 1 entry verification" section below). |

## Tier 1 entry verification (D49 — captured here for the next worker)

All entry conditions hold at SHA `e2215c2b` on
`refactor/legacy-to-graph-dispatch-migration`:

1. ✓ `crates/verter_session/src/lib.rs` four off-store fields present:
   `compile_cache:375`, `resolved_type_cache:391`, `eval_env_cache:404`,
   `semantic_db:416`.
2. ✓ `crates/verter_session/src/host_manage.rs:418..422` thread-locals
   present (`HOST_PARSED_EVAL_PROGRAM_CACHE:419`,
   `HOST_PARSED_TYPE_CONTEXT_CACHE:422`). NOTE: line 425 has a
   pre-existing `TODO(follow-up)` to move them — Tier 1A acts on it.
3. ✓ `crates/verter_napi/src/meta.rs:222` `impl NapiMetaSession`
   present; `:284` `get_component_meta` present.
4. ✓ `packages/component-meta/src/runtime/project-session.ts:172`
   `ProjectSession.getComponentMeta` present.
5. ✓ `packages/component-meta/src/project.ts:196`
   `ComponentMetaSession.getComponentMeta` present.
6. ✓ `packages/native/index.ts:503` `getComponentMeta` typing on
   `ComponentMetaSession` present.
7. ✓ `packages/component-meta/src/runtime/project-engine.ts:35`
   `NativeMetaSession` interface present; `:46` `getComponentMeta`.
8. ✓ `crates/verter_lsp/src/main.rs:118..160` `.custom_method(...)`
   chain present; verified absent: `getComponentMeta`,
   `getComponentMetaSurface`, `getComponentMetaTypeExpansion`. All
   three are NEW additions for Tier 5b per D113.
9. ✓ `crates/verter_lsp/src/server/custom_methods.rs` exists as a file
   (27754 bytes); D96 conversion target — Tier 5b converts it to
   `custom_methods/{mod.rs, component_meta.rs}`.
10. ✓ `crates/verter_protocol/src/component_meta.rs:7`
    `use prost::Message;` present (verifies the `Message` trait is
    consumed). Plan deviation: this trait is applied to *codegen-
    produced* types, not hand-written derives — see Deviation 1 above.
11. ✓ `crates/verter_session/src/meta.rs:491`
    `MetaSession::resolve_alias_or_canonical` present (NOT
    `resolve_canonical_or_alias` as r7 wrongly claimed; D122 corrected).

## Discriminating tests landed in this session

1. `expected_corpus_test_count_constant_replaces_sidecar` (Step 0.6,
   §2.2 of the plan). FAILS pre-change (sidecar exists), PASSES
   post-change (sidecar deleted).

The other 14 Tier 0 discriminating tests (per plan §2.2) require:

- Test fixtures for the corpus-snapshot tests (gated by Step 0.1
  completing).
- Test fixtures for the eager-key-set test (gated by Step 0.2
  completing).
- Test fixtures for the audit-document tests (gated by Step 0.3
  completing).
- Author the proto-audit test for D100 schema validation.
- Author the LSP method binding doc presence test.
- Author the macro-impact-inventory baseline test.

Dispatch as a Step 0.X follow-up worker.

## Tooling versions verified (D82)

| Tool | Required | Actual |
|---|---|---|
| git | ≥ 2.38 | 2.52.0.windows.1 |
| cargo | ≥ 1.75 | 1.92.0 |
| node | ≥ 20.10 | v22.20.0 |
| pnpm | ≥ 9.0 | 10.22.0 |

All clear.

## Next step recommendation

The fastest path forward to a clean `phase-tier-0-complete` marker:

1. **Dispatch Step 0.4-bis worker** — sweep the 618 production
   archaeology hits per D111 classifier. Concrete brief, mechanical
   work, ~1 session.
2. **Dispatch Step 0.1 + 0.2 + 0.3 worker** — extend
   `audit_real_component_meta` example, run benchmarks, produce the
   golden corpus snapshot, semantic-graph eager snapshot, and 5 audit
   documents. Substantial work; ~2-3 sessions.
3. **Author remaining 14 discriminating tests** — partially blocked
   on (1) and (2). Some can land independently (proto-audit, LSP
   binding doc presence test).
4. **Clippy pinning + final acceptance gate** — quick, can run inside
   the same session that closes Tier 0.

After `phase-tier-0-complete`: dispatch W1 (Tier 1) + W2 (Tier 3) +
W3 (Tier 5a) + W4 (Tier 6) in parallel via `Agent` tool with
`isolation: "worktree"`. Width 4 (within max 6 per D76).
