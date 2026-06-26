# Phase 8 Worker Report — Cache rehoming verification + `no_off_store_host_caches` guard

**Status:** SUCCESS (atomic gate)
**Branch:** `wt/phase-08-cache-rehoming`
**Base commit at spawn:** `e2f41a0066c60b0e2015df004d8e65b9fdcc1105` (phase-04b-complete; the topologically-final marker on `refactor/semantic-db-overhaul`)
**Work head before marker:** `0eef4772`
**Marker:** to be written as `chore(orchestrator): mark phase 08 complete`

---

## §1 Executive summary

Phase 8 verified Phase 6b's binding classification of every cache-shape
field on `VerterHost`, found the classifications sound post-Phase-5l and
post-Phase-4b, and shipped the `no_off_store_host_caches` static guard
that mechanically enforces the "no caches outside `ProjectTypeStore`"
rule for all future commits.

Per the brief's "expected path": **zero rehoming commits required.**
Phase 6b had already deleted the three `mirror`-classified fields
(F3 `routes`/`imported_roots`, F6 `external_type_analysis_cache`,
F7 `route_owned_shallow_cache`); Phase 8's audit confirmed each is absent
from the integration tip. Every `legitimate-authority` field's 6b
rationale survived the post-5l/4b verification.

The guard ships **un-ignored** at land time (different convention from
prior architecture guards because Phase 8 has full visibility per §8.1
of the cutover plan). It includes a discriminator self-test that
exercises the algorithm against synthetic structs to prove the
empty-violations result against the production tree is signal, not a
broken detector.

`deferred[]` is empty. No STOPs encountered.

---

## §2 Per-commit summary

| SHA        | Title                                                                              |
| ---------- | ---------------------------------------------------------------------------------- |
| `2ecf89cc` | docs(orchestrator): phase 08 cache-shape audit (post-5l + post-4b)                 |
| `0eef4772` | test(arch): add no_off_store_host_caches static guard with phase-06b allow-list    |
| (next)     | chore(orchestrator): mark phase 08 complete                                        |

---

## §3 Audit findings (full enumeration)

### §3.1 Per-field audit of `VerterHost`

The `VerterHost` struct at `crates/verter_session/src/lib.rs:261–398`
contains 20 fields. The full table is in `phase-08-audit.md` §2.1; the
classification summary is:

| Bucket                                      | Count | Fields (by 6b row)                                                                       |
| ------------------------------------------- | ----- | ---------------------------------------------------------------------------------------- |
| Cache shape, allow-listed (legitimate-authority) | 8     | F1 compile_cache, F2 resolved_type_cache, F4 eval_env_cache, F5 semantic_db, F10 query_profile, F12 alias_to_canonical, F13 last_const_prop_overrides + workspace (single-cell handle) |
| Cache shape, ProjectTypeStore destination   | 1     | `project_type_store: Arc<ProjectTypeStore>` (the destination)                            |
| Mirror — DELETED by Phase 6b                | 3     | F3 `runtime.routes`/`imported_roots` (Arc-shared in 6b.B2), F6 `external_type_analysis_cache` (deleted in 6b.D2a), F7 `route_owned_shallow_cache` (deleted in 6b.D2a) |
| Non-cache shapes                            | 9     | instance_id, config, tick, store_view_epoch, scheduler, provenance, resolver (now Arc-share wrapper), request_id_counter, audit_records, plus cfg-gated metrics + test_audit |

(F11 `audit_records` is a `legitimate-authority` 6b classification but its
host-level field shape is `Arc<AuditRecordsStore>` — Arc holding a struct
— so the structural cache-shape detector does not flag it. The inner
state is bounded `Mutex<IndexMap<u64, RustAuditRecord>>` capacity 256
FIFO. Documented in the audit for review; no allow-list entry needed
because the structural shape doesn't match the detector pattern.)

### §3.2 Mirror-deletion verification

Phase 6b's three `mirror` classifications were each verified absent
from the integration tip:

| 6b row | Field name                         | Verification at HEAD `e2f41a00`                                                |
| ------ | ---------------------------------- | ------------------------------------------------------------------------------ |
| F3     | `runtime.routes` / `.imported_roots` | Arc-shared with `ProjectTypeStore` per 6b.B2 commit `cb6f5bf1`. Verified at `lib.rs:373` (`project_type_store: Arc<ProjectTypeStore>`); `routes_handle()`/`imported_roots_handle()` accessors return shared `Arc`s. |
| F6     | `external_type_analysis_cache`     | Field absent (`grep` zero hits). Comment at `lib.rs:363–369` documents F6/F7 atomic deletion in 6b.D2a commit `c6e7fbeb`. |
| F7     | `route_owned_shallow_cache`        | Field absent. Same comment block verifies. |

The new guard re-verifies F6 and F7 absence by name as a belt-and-
suspenders check independent of the syn walk
(`crates/verter_session/tests/architecture_guards.rs:1023–1034`).

### §3.3 Re-litigation check (post-5l/4b)

For each `legitimate-authority` allow-list entry, the 6b rationale was
re-checked against the post-5l + post-4b tree. Phase 5l deletes
deprecated `ComponentMetaQueryEngine` resolver methods; Phase 4b deletes
the `read_source` text-projection helpers in `declaration_metadata.rs`.
Neither phase introduces new cache-shape fields on `VerterHost`. Every
6b rationale stands at the integration tip. Full table in
`phase-08-audit.md` §8.

Conclusion: zero `legitimate-authority` fields needed to be re-litigated.
The expected path of the brief held.

### §3.4 New-cache search

A targeted `git log -p` between Phase 6b's foundation HEAD `3147c02f`
and the integration tip `e2f41a00` for `+\s*pub(crate)\s+\w+:\s*(DashMap|RwLock|Mutex|FxHashMap|HashMap)`
on `crates/verter_session/src/lib.rs` returned zero net additions. The
intervening commits across 5h–5l, 6, 6b, 6c, 7, and 4b touched
`VerterHost` only for:

- 6b.A annotation pass (commit `79fbad38`) — pure doc-only.
- 6b.D2a F6/F7 atomic deletion (commit `c6e7fbeb`) — net field removal.
- 6b.D2b host wrappers in commit `5ced1e8f` — added methods, no fields.

No cache-shape field was added that would require Phase 8 rehoming.

---

## §4 Static guard

### §4.1 Body summary

`no_off_store_host_caches` (in
`crates/verter_session/tests/architecture_guards.rs`) does the following:

1. Reads `crates/verter_session/src/lib.rs`.
2. Verifies F6/F7 deleted-field names do not reappear by simple
   substring match against the declaration pattern.
3. Parses the file via `syn::parse_file` (added as a dev-dep alongside
   `quote` for `Type::to_token_stream()`).
4. Walks `pub struct VerterHost`'s named fields. For each, renders the
   type signature via `quote::ToTokens` to a canonical token-string.
5. Classifies by structural shape:
   - `DashMap<...>`, `Shared<FxHashMap...>`, `Shared<HashMap...>`,
     `Mutex<...>`, `RwLock<...>` → cache-shape candidate.
   - Plain `Arc<X>`, `Box<X>`, `Atomic*`, owned scalars → non-cache.
6. For each cache-shape candidate, asserts it is either:
   - on the documented allow-list (with phase-report citation), OR
   - the `project_type_store` field, OR
   - a future field whose type points at `ProjectTypeStore`.
7. Asserts the syn walk surfaced ≥ 1 cache-shape field (otherwise the
   detector is broken).

### §4.2 Allow-list

The 8-entry allow-list:

```text
"alias_to_canonical"          → §F12: caller-supplied virtual-alias map at upsert time
"last_const_prop_overrides"   → §F13: Phase-7 invalidation state-diff record
"compile_cache"               → §F1: per-profile compile state, sub-mirror lifecycle
"resolved_type_cache"         → §F2: shared external-type cache, profile-gated writes
"eval_env_cache"              → §F4: owned-data EvalEnv snapshots, host-local consumers
"semantic_db"                 → §F5: different crate, different artifact
"query_profile"               → §F10: execution-policy state, not a cache
"workspace"                   → §6b.2.F6.bypass: single-cell handle (Arc<RwLock<Arc<dyn>>>)
```

Each entry maps to `phase-06b-report.md` plus a one-line architectural
rationale. Future allow-list additions must follow the same convention.

### §4.3 Discriminator self-test

`no_off_store_host_caches_discriminator_self_test` exercises the inner
algorithm against three synthetic structs to prove the algorithm
discriminates. CLAUDE.md "Stub Prevention" requires that the
empty-violations result on the integration tip be discriminating, not
trivially passing.

| Synthetic case        | Cache-shape fields                                                                | Expected behaviour                                       |
| --------------------- | --------------------------------------------------------------------------------- | -------------------------------------------------------- |
| (a) `synthetic_pass`  | `compile_cache: dashmap::DashMap<...>`, `workspace: Arc<RwLock<Arc<dyn ...>>>`    | 0 violations (both allow-listed); both surveyed.         |
| (b) `synthetic_fail`  | `p8_probe_cache: parking_lot::Mutex<rustc_hash::FxHashMap<String, u64>>`          | exactly 1 violation, naming `p8_probe_cache`.            |
| (c) `synthetic_destination` | `future_db: Arc<...::ProjectTypeStore>`, `future_cache: parking_lot::Mutex<...::ProjectTypeStore>` | 0 violations (destination allowance).                   |

The self-test would FAIL against any future commit that:
- Removes the cache-shape detector substrings (case (b) would no longer detect).
- Removes the allow-list (case (a) would falsely flag legitimate fields).
- Removes the destination allowance (case (c) would falsely flag the future hook).

This satisfies the CLAUDE.md "characterization tests must discriminate"
rule.

### §4.4 Insertion point

Per the brief, the guard is appended after the existing
`no_scheduler_backed_workspace_shim_in_session_src` guard (the most
recent guard pre-Phase-8). It ships un-ignored at land time per the
brief's instruction:

> The guard SHIPS un-ignored. It MUST PASS at land time. (Phase 8 has
> full visibility per §8.1; no `#[ignore]` lifecycle here.)

---

## §5 Verification

### §5.1 Workspace tests

```text
cargo test --workspace --tests --verbose 2>&1 | tee /tmp/p08-workspace.txt:
  blocks:  45
  passed: 10283
  failed:  0
  ignored: 4
```

Up from baseline (pre-Phase-8) of `passed: 10281` by **+2** — the new
`no_off_store_host_caches` plus its discriminator self-test.

### §5.2 Correctness gate

```text
cargo test -p verter_session --test correctness 2>&1 | tee /tmp/p08-correctness.txt:
  passed: 18
  failed: 0
  ignored: 1
```

Identical to baseline (no snapshot drift; Phase 8 introduces no Class A
fixtures).

### §5.3 cargo fmt --all --check

Clean. (One re-flow applied to the new guard's `parse_file` call to
satisfy 100-col line wrap; committed in the same commit.)

### §5.4 cargo clippy --workspace --tests -- -D warnings

The integration tip carries 13 pre-existing clippy errors (verified via
baseline run on `e2f41a00` before any Phase-8 commits — see
`/tmp/p08-baseline-clippy.txt`). Phase 7's report at the integration tip
called out 2 pre-existing errors in `verter_session`; the count grew
between Phase 7 and Phase 4b's tip. Phase 8 introduces NO new clippy
warnings — the new guard, its helpers, and the dev-dep additions
compile clean. Per CLAUDE.md "Fix Quality" / Phase 7's precedent, these
pre-existing warnings are owned by Phase 11a (the meta_resolve.rs
god-module split).

### §5.5 pnpm install --frozen-lockfile

Clean. Lockfile in sync.

---

## §6 Cargo.toml / Cargo.lock changes

Added two dev-dependencies to `crates/verter_session/Cargo.toml` for the
guard's syn-based field walk:

```toml
syn = { version = "2", features = ["full", "extra-traits"] }
quote = "1"
```

`Cargo.lock` gains two lines under the `verter_session` dependency
entry: `quote` and `syn 2.0.117`. Both crates are already transitively
present in the lock file from other workspace dependencies — no version
bumps elsewhere.

---

## §7 Anchor drift log

No anchor drift. Phase 8 reads anchors only via the guard at runtime
(no compile-time anchor citations); the audit cites Phase 6b's
classification §6b.2 entries (F1–F13) which are stable section ids in
the sub-plan document, not line numbers.

---

## §8 Hard-stop constraints (§8.2 of the cutover plan) compliance

| #   | Constraint                                                                                                   | Status                                                                                                          |
| --- | ------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------- |
| 1   | Phase 5l report present at integration tip                                                                   | Verified — `phase-05l-report.md` present and reviewed.                                                          |
| 2   | Phase 6 report present at integration tip                                                                    | Verified — `phase-06-report.md` present.                                                                        |
| 3   | Phase 6b inventory present at integration tip                                                                | Verified — `phase-06b-report.md` + sub-plan at `<scratch>/verter-architecture-cutover-phase-06b.md` (1092 lines) reviewed end-to-end. |
| 4   | No cache requires a new abstraction beyond Template-A / Template-B                                           | Verified — zero rehoming commits required; allow-list captures every legitimate-authority case via 6b citation. |
| 5   | Every seed-inventory entry (§8.1 table) located OR shown deleted by prior phase                              | Verified — F1/F2/F4/F12/F13 are kept (legitimate-authority); F6/F7 deleted in 6b.D2a; F3 Arc-shared in 6b.B2.    |
| 6   | No `legitimate-authority` field's 6b rationale is unsound post-5l/4b                                         | Verified — full re-litigation table in `phase-08-audit.md` §8.                                                  |
| 7   | New `no_off_store_host_caches` guard PASSES at land time (un-ignored)                                        | Verified — workspace test 10283/0 with guard active.                                                            |
| 8   | Marker `phase-08-complete` is `status: "success"` AND `deferred[]` is empty                                  | This phase ends with the marker so labeled.                                                                     |

---

## §9 Deferred

**EMPTY.** Per the atomic-gate constraint (§8.2 of the plan and r17/Codex-P1#1),
phase-08 is in the `ATOMIC_GATE_PHASES` allowlist; `deferred[]` MUST be
empty. The expected path of the brief held: zero rehoming commits
required, zero deferrals.

---

## §10 Files touched

### `crates/verter_session/Cargo.toml`
- Lines 80-84 — added `syn = { version = "2", features = ["full", "extra-traits"] }` and `quote = "1"` as dev-dependencies, with a Phase-8 explanatory comment.

### `crates/verter_session/tests/architecture_guards.rs`
- Lines 822–942 — Phase-8 prologue + `phase_8_allow_list()` helper.
- Lines 944–957 — `is_cache_shape()` helper.
- Lines 959–966 — `render_type()` helper.
- Lines 968–1041 — `no_off_store_host_caches_inner()` core algorithm (pure function).
- Lines 1043–1090 — `no_off_store_host_caches` test (production-tip walk).
- Lines 1092–1177 — `no_off_store_host_caches_discriminator_self_test` (discriminator).

### `Cargo.lock`
- 2-line addition under the `verter_session` dependency block (`quote`, `syn 2.0.117`).

### `phase-08-audit.md`
- New file (268 lines) documenting the per-field audit, mirror-deletion verification, re-litigation check, new-cache search, and audit conclusion.

### `phase-08-report.md`
- This file.

---

End of report.
