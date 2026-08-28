# Worker Context Packet — neutral contracts, typed identities, profile schemas, dependency firewall

**Packet digest:** `shasum -a 256` over this file's raw bytes at dispatch; recorded in the ledger's
`context_packet_digest` for the block.
**Created from program-state digest:** the ledger digest at the transition that sets this block
`READY`.
**Role:** Implementor
**Block / charter:** `B1` — [`../../charters/B1.md`](../../charters/B1.md) (BOUND; supersedes the
template)
**Stack window / snapshot / layer / acceptance block:** none. Depth 1, `stack_id = ""`,
`stack_layer = 0`. No stack window is opened for this block.
**Writable worktree / branch:** `work/b1-neutral-contracts`, in a **sibling** directory of the
program root — never nested inside it.
**Maintainer:** Carlos Rodrigues (GitHub `pikax`).
**Orchestrator:** the main session recorded in the ledger's `[orchestration]` table.

# 1. Exact identities

- **Authority package digest:** the recomputable aggregate over the landed authority tree, recorded
  in the Implementation Lock Record §1. The canonical package digest does not exist — package
  validation is waived by maintainer ruling.
- **Implementation Lock digest:** the accepted lock record's digest, from the ledger's
  `implementation_lock_digest`. It is not `PRE-A6`: the lock is accepted.
- **Entry checkout SHA/tree:** `9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0` /
  `3cf111cf5665586b7d8fdfd520f01cfee3bf8108`
- **Implementation baseline SHA/tree:** `fb863297a04c7eb114d53ff65736c00240354504` /
  `a2e01e16d705faecf259152f40d0a3b228b16dbf`
- **Block base SHA/tree:** the accepted tip of `program/architecture-lock` at dispatch. Recorded by
  the orchestrator; it is the lock-record landing, not the baseline above.
- **Current candidate SHA/tree:** `UNSET`
- **Charter digest:** `shasum -a 256 docs/arch/refactor/rev11/charters/B1.md`
- **Predecessor evidence:** the accepted lock record and the inventories it binds —
  [`../A5/owner-rows.md`](../A5/owner-rows.md),
  [`../A5/option-classification.tsv`](../A5/option-classification.tsv),
  [`../A5/compatibility-domains.md`](../A5/compatibility-domains.md),
  [`../A5/consumer-protocol-map.md`](../A5/consumer-protocol-map.md),
  [`../A5/dependency-direction.md`](../A5/dependency-direction.md).

# 2. Assigned objective

Land the distinct identity, profile, mapping and result-contract types the later blocks depend on,
plus one whole-workspace forbidden-dependency-edge build test, so that every artifact and query can
state its construction and compatibility identity without a global revision, a request identity, or
an ad hoc string — **without** creating a service owner to hold the types, without migrating semantic
behavior, and without leaving a conversion layer behind.

# 3. Current source facts

**Read the inventories directly; do not work from this summary.** These five facts are repeated only
because missing one changes the design rather than the schedule.

1. **`ProviderHub` does not exist.** `grep -rn "ProviderHub" crates/*/src` returns nothing. It is a
   target name that the seed reconciliation table listed under *current* authorities. The real owners
   are `SyncCoordinatorHandle` (`verter_lsp`), the `TypeProvider` trait (`verter_type_runtime`), and
   `EngineBackend` / `BoundProject` / `ProjectBinding` (`verter_session::external_ts`).
2. **`type_env_hash` and `lib_env_hash` are constant across every project.** `EnvHashInputs` is
   constructed at three non-test sites, all in `crates/verter_workspace/src/engine.rs`, all
   hardcoding `type_strict: false`, `type_no_implicit_any: false`, `lib_names: &[]`,
   `type_roots: &[]`. Model the dimension so it *can* carry real values. Do **not** thread real
   values in: that changes cache identity for every existing project and is a different block's
   blast radius.
3. **Two upward dependency edges exist and are ratified as an equality-pinned exception**:
   `verter_semantic -> verter_workspace` and `verter_diagnostics -> verter_workspace`. The
   consequence splits by target — `verter_workspace -> verter_scheduler` is unconditional
   (`Cargo.toml` line 37) while `verter_workspace -> verter_tsgo_api` is
   `cfg(not(target_arch = "wasm32"))` only (line 49). The exception must record the target condition
   with the edge; a `wasm32`-only resolve must not read as satisfied.
4. **`TypeExpr` reference distribution is not flat.** `verter_session` holds 65% of all references;
   the marker crates `verter_no_typeexpr` / `verter_no_storedspan` are **instruments** that prove
   absence structurally, not consumers to migrate. Extend that model rather than replacing it.
5. **`provider_protocol_version = 12` is NOT PROVEN** to be free of duplicate-domain overlap
   (`crates/verter_protocol/src/consumer_compatibility_manifest.rs:75`, consumed at `:109`). Do not
   resolve it by assumption in either direction; it is another block's question.

**Known open branch/PR conflicts:** none live. The unlanded local branch population is abandoned as a
class and no branch is deleted; nothing is pushed to `origin`.

# 4. Allowed write set

- Rust source across the workspace, as required to introduce the typed contracts and migrate the
  bounded consumer closure.
- The forbidden-edge test and the deletion of the guards it supersedes.
- `Cargo.toml` manifests only where a dependency genuinely changes; lockfile updates that follow.
- `docs/arch/refactor/rev11/evidence/B1/` and the block's identity-free summary.
- The block's own `work/` branch history.

Everything else is read-only unless the orchestrator accepts a rescope. In particular: the ledger
(`docs/arch/architecture-lock/ledger/program-state.toml`) is **orchestrator-only** — an implementer
that records its own row is self-accepting. `performance-gates.toml` is **immutable to this block**.

# 5. Forbidden changes

- **No gate weakening.** Thresholds may not be relaxed, reweighted, subsetted or reinterpreted, and a
  pass criterion may not be chosen after measurement. A cell may be *added* only through a new lock
  record digest and the same independent review class.
- **No scope widening or unrelated cleanup.** A defect found outside the closure is reported with its
  evidence, not fixed here.
- **No compatibility shim, shadow path, runtime switch or alternate authority.** Displaced aliases,
  wrappers and counters are deleted in the same candidate. A conversion layer intended to survive is
  a charter violation.
- **No new name-keyed source-text scanner as landed enforcement.** The dependency test is a
  resolve-graph walk. A grep over the source tree is not an acceptable substitute and is forbidden by
  the repository's forward-only rule.
- **No service owner created merely to hold types.**
- **No `.github/` change.** The one narrow CI-wiring authorization was granted for a different named
  purpose; extending it needs its own maintainer ruling. No CI job runs for this program anyway.
- **No self-approval or review-result fabrication.** This context fills no review mandate.

# 6. Required end state and deletions

**Surviving owners.** The new identity/profile/mapping/result-contract types, in the lowest
dependency-neutral crate that can correctly serve every consumer. `StableEntityId` and
`SessionHandle` non-interchangeable; `QueryIdentity` distinct from `SemanticFlightKey` and
`InputBasisId`; no global revision, request, deadline or budget inside reusable identity.

**Deletions, in the same accepted candidate.**

- `verter_audit_no_upward_deps` (`crates/verter_session/tests/cases/architecture_guards.rs`) — its
  invariant is strictly implied by the closure walk.
- Both tests in `crates/verter_scheduler/tests/cases/no_session_dep.rs` — same reason.
- Every alias, wrapper or counter displaced by the new types.

**One decision that must be made explicitly, not assumed.** `audit_substrate_isolation` is *not*
fully implied by the closure walk. Its dependency half is; its **naming** half is not — it rejects any
`verter_*` token on a non-comment line under `crates/verter_audit/src`, including tokens that are not
dependencies at all, which is exactly what caught a local binding name during the instrumentation
block. Decide: keep it as a separate, named, grandfathered guard, or drop it as coverage the program
does not need. Deleting it silently while calling the closure test a superset loses real coverage and
is not an available option.

**Exact one-path invariant.** After this block there is exactly one dependency-direction authority —
the resolve-graph closure test — and exactly one home for each new identity type. Two authorities for
one rule is the failure this block exists to prevent.

# 7. Required commands and proof

| Command/evidence | Expected non-vacuous work | Required result | Raw output path |
|---|---|---|---|
| `node scripts/gate.mjs` | the full three-surface Rust universe; non-zero executed tests on every surface | PASS **except** the inherited `tracked_paths_no_machine_roots` baseline debt — see the precondition below; the byte-pin freshness pair must run genuinely (run with `node_modules` present) | `B1/command-proofs/` |
| `cargo clippy --workspace --all-targets -- -D warnings` | every target, host | clean | same |
| `cargo check --workspace --release` | real release profile; catches the `debug_assert!` name-resolution class the gate cannot | clean | same |
| `cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings` | the wasm32 artifact host clippy cannot see | clean | same |
| `cargo fmt --all --check` | whole workspace | clean | same |
| `pnpm install --frozen-lockfile` | lockfile in sync | clean | same |
| `pnpm test` | the JS/TS suites | pass | same |
| `cargo check --workspace --all-targets --features verter_audit/attribution` | type-checks the enabled arm's amount expressions, which the default arm does not | clean | same |
| `cargo test -p verter_audit --features attribution` | the enabled-arm tests | pass | same |
| `cargo test -p verter_audit --features compile-fail` | the trybuild seal proving the counter-reader path is absent | pass | same |
| **dependency-test discrimination** | plant one forbidden edge in a scratch manifest; the test must FAIL | FAIL when planted, PASS when reverted, **plus proof the plant was present, unique and new in the source before the run** | same |
| **performance cell `A6_META_COMPILE_40_COLD_RUST`** | ≥30 measured samples per arm, alternating invocations, idle machine, control benchmark at session start and end | every metric passes conjunctively; the component-meta digest oracle must match exactly | same |
| `node scripts/validate-performance-gates.mjs --gates performance-gates.toml` | proves the gate file is unmodified and placeholder-free | PASS | same |

**Precondition on the gate row — discovery `D-1`, inherited baseline debt, NOT B1's to fix.** The
canonical gate does **not** return PASS on the tree this block inherits, and did not return PASS on
the implementation baseline either. `tracked_paths_no_machine_roots` fails on **two** tracked files —
`docs/arch/refactor/rev11/evidence/A4/context-packet.md` and
`.../A5/context-packet.md` — each of which embeds an absolute machine path. Both were landed by
evidence-only blocks that skipped the canonical gate on the reasoning that they changed no production
source; the guard scans tracked *bytes*, not production source, so that reasoning had a hole in it.
Repairing those two files changes the `context_packet_digest` values the ledger already records for
two accepted blocks, which is an orchestrator/maintainer action, **outside this block's write set**.

So B1's gate result is evaluated against the baseline rather than against a bare PASS:

- **Required:** the same or fewer `tracked_paths_no_machine_roots` violations than the recorded
  baseline of **2**. Introducing a third — for example by committing this block's own dispatch packet
  or a raw log with an absolute path in it — is a **FAIL**, not inherited debt.
- **Required:** every other test in the gate passes. Any non-`tracked_paths_no_machine_roots` failure
  is B1's, and "the gate was already red" is not an available explanation for it.
- Run the guard in isolation to establish the count discriminatingly rather than reading it off the
  full-gate summary:
  `cargo test -p verter_session --test main cases::tracked_paths_no_machine_roots -- --nocapture`.

Record the isolated-guard output alongside the full gate log. Two load-sensitive tests (a
real-tsserver respawn test and a trybuild smoke test at the 360 s cap) also failed at the baseline
under heavy load and passed in isolation on an idle machine; if either recurs, prove it the same way
rather than classifying it by assertion.

A green command that executed zero intended work is a failure. Record exact command, working
directory, environment and features, exit code, executed and skipped counts, exact
binaries/packages/fixtures, and the raw output digest.

**The discrimination proof is the one most likely to be faked by accident.** A plant that fails to
apply reports a pass: `perl`/`sed`/`grep` exit 0 on a non-match, and a verification search that hits a
pre-existing occurrence of the planted string is a false positive. Prove the mutation is present,
unique and new before trusting the run. A green planted run means the plant failed until proven
otherwise.

# 8. Review scope and output

- **Mandatory changed surface:** every new type and its call sites; the dependency test and its
  discrimination proof; the deletion set; the `audit_substrate_isolation` decision.
- **Required dependency/owner closure:** the full resolve graph under
  `cargo metadata --all-features`, plus the enumerated public and wire consumers.
- **Causal blocker rule:** a finding blocks only if it is causally connected to this block's changed
  surface or to its exit criterion. Everything else is a non-blocking discovery, recorded with
  evidence.
- **Output format:** the block record required by the orchestration contract, with raw evidence paths
  and digests and no unsupported success claim. Return one of `PASS` / `BLOCKING FINDINGS` /
  `NOT PROVEN` / `NON-BLOCKING DISCOVERIES`, bound to one exact candidate SHA **and** tree.

# 9. Stop/rescope conditions

Stop and report rather than improvise when any of these is true:

- an undiscovered public or wire consumer appears that the consumer map does not enumerate;
- an incompatible persisted domain would have to change;
- a dependency cycle appears;
- canonical equality material is missing for a type that must be an identity;
- a configuration field's profile or identity class is genuinely ambiguous;
- the closure walk finds an upward edge that the ratified exception does not already record —
  **record it, do not widen the exception and do not weaken the test**;
- the correct fix requires a primitive that does not exist yet;
- a locked gate metric fails and the only way to green it is to change the gate.

Breadth, breakage, effort or migration size are never grounds to stop or to narrow scope. An
architectural deviation that appears necessary is recorded for ratification, never silently shipped.

# 10. Handoff result

Return the block record required by `contracts/agent-orchestration.md`, with raw evidence paths and
digests. Write no ledger row. Make no acceptance claim: acceptance is the maintainer's, on three
independent mandates against one unchanged candidate SHA and tree.
