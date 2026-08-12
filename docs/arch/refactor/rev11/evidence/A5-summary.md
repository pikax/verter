# A5 — Owner, identity, profile, compatibility, protocol, and consumer inventories

Resolves current authorities and every affected direct consumer against the post-A4 tree, so that
later blocks cannot create a second owner or discover a hidden public/wire consumer mid-cutover by
omission.

This is a **decision-record block. It changes no file under `crates/`, `packages/`, `scripts/`, or
`.github/`.** Every change it contemplates is decided here and executed by a named later block —
the reasoning is in [`A5/instrumentation-reconciliation.md`](A5/instrumentation-reconciliation.md) §4.

Identity-free by construction: this file records no candidate SHA and no review verdict, per the
convention A0 established.

## The evidence

| artifact | what it resolves |
|---|---|
| [`A5/owner-rows.md`](A5/owner-rows.md) | all 16 `VERIFY` seed rows of `contracts/current-tree-reconciliation.md`, source-verified, plus 2 rows the seed table omits |
| [`A5/option-classification.tsv`](A5/option-classification.tsv) + [`.md`](A5/option-classification.md) | 84 configuration fields across 5 owner structs, one class each per `contracts/semantic-profile.md` §1 |
| [`A5/compatibility-domains.md`](A5/compatibility-domains.md) | every version-like value and the 5 cache-identity dimensions, against ADR-002 |
| [`A5/consumer-protocol-map.md`](A5/consumer-protocol-map.md) | `TypeExpr` / component-meta / graph / wire consumers — the seed `E1` turns into the exact map |
| [`A5/dependency-direction.md`](A5/dependency-direction.md) | the locked dependency-direction test strategy for `B1`, and the 2 upward edges in the current tree |
| [`A5/instrumentation-reconciliation.md`](A5/instrumentation-reconciliation.md) | the two instrumentation owners; A4's deferred gate-coverage debt |
| [`A5/loop5-counter-census.tsv`](A5/loop5-counter-census.tsv) | per-static reference census backing that decision |
| [`A5/open-changes.md`](A5/open-changes.md) | the 471 unlanded local branches (469 third-party candidates) and the 10 live worktrees |
| [`A5/program-operations-policy.md`](A5/program-operations-policy.md) | evidence custody, program-state workflow, worktree/branch/CI/merge rules, stack window, review contexts |

## What the inventory found

### Two seed rows were wrong about the current tree

The point of running the reconciliation against source rather than against the plan's own prose.

- **`flow_slice_content.rs` is not "a second flow/control semantics path".** It is the content half
  of the one flow substrate — `FunctionProgramIndex` is the structural inventory, the flow-slice
  planner produces the content-free `FlowSliceIR`, and this module lowers only slice-selected
  content, routing expression lowering through the one shared shallow-pass lowering. A `D2` charter
  ratified against "delete the second flow engine" would have sent an implementor to delete
  something that is not there.
- **`ProviderHub` does not exist.** `grep -rn "ProviderHub" crates/*/src` returns nothing; it is a
  Revision 11 *target* name (block `H2`), listed in the seed table under *current* authorities. The
  real owners are `SyncCoordinatorHandle` (`verter_lsp`), the `TypeProvider` trait
  (`verter_type_runtime`), and the `EngineBackend` / `BoundProject` / `ProjectBinding` triple
  (`verter_session::external_ts`).

### The semantic kernel is not lifecycle-independent

`verter_semantic` depends on `verter_workspace`, which depends on `verter_scheduler`
unconditionally and on `verter_tsgo_api` under
`[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`. So the kernel's production closure, on
the native `--all-features` resolve, is:

```text
verter_audit  verter_css_syntax  verter_ecma  verter_language  verter_no_storedspan(+_derive)
verter_no_typeexpr(+_derive)  verter_parser  verter_scheduler  verter_span  verter_tsgo_api
verter_type_expr  verter_type_expr_oxc  verter_workspace
```

ADR-015's stated consequence — "semantic kernel remains reusable across lifecycles" — does not hold:
linking the kernel links a task scheduler on every target, and an out-of-process tool client on
everything but `wasm32`. It is not repairable by re-layering (`verter_workspace` cannot sit below
`verter_semantic` because of its own upward deps), and the platform split narrows only the second
half — a `wasm32`-only firewall is not a firewall. Owner `C1`, with `B1` owning the test that makes
it fail rather than be reviewed for; `B1`'s equality-pinned exception must record the target
condition with the edge.

For contrast, the two crates that *do* have a real firewall: `verter_audit`'s closure is
`{verter_audit, verter_span}`, and `verter_macro_dto`'s is itself plus the four `No*` marker crates
and `verter_span`.

### Two of the five cache dimensions have no production input

`EnvHashInputs` is constructed at exactly three non-test sites, all in
`crates/verter_workspace/src/engine.rs`, and all three hardcode `type_strict: false`,
`type_no_implicit_any: false`, `lib_names: &[]`, `type_roots: &[]`. `type_env_hash` and
`lib_env_hash` are therefore **constant across every project** today.

The strict-family semantics do exist (`StrictFamilyConfig`, covering `strictNullChecks`,
`strictFunctionTypes`, `exactOptionalPropertyTypes`) but are driven by a `pub(crate)`
test-injection `AtomicU8` with no production writer. This is not a live cache-correctness defect —
nothing varies, so nothing collides — but it is a missing ingress, and whichever block first threads
real tsconfig values in **changes cache identity for every existing project at that moment**. `B1`
obligation, `G4` blast radius.

### Two instrumentation owners, one of them reporting 24 permanent zeros

A4 landed `verter_audit::attribution` onto a tree that already contained
`crates/verter_session/src/loop5_instrumentation.rs` — 1,121 lines, unconditionally compiled, no
feature gate, 46 atomics. The census
([`A5/loop5-counter-census.tsv`](A5/loop5-counter-census.tsv)) splits them:

- **24 statics are never incremented anywhere** — yet they are reset, loaded, and emitted into the
  JSON dump as `0`, which reads as "this work did not happen" rather than "this counter was never
  wired". A `#[cfg(test)] fn dump_emits_all_keys` asserts those key names appear in the dump, so the
  module's own tests pin the misleading output in place. A4 deleted five of its own guessed sites
  for exactly this reason; the same standard condemns these 24.
- **18 are live and overlap A4's sites at the same chokepoints.**
  `component_meta_materialize.rs` carries a `loop5` `TimerGuard` at line 975 and
  `attribute_scope!(MaterializeStructure)` at line 979 — four lines apart.
- **4 belong to a backtrace watchdog**, which is a debugging facility, not work attribution, and is
  correctly *not* folded into the attribution substrate.

`loop5` also costs what A4's disabled arm does not: `TimerGuard::new` calls `Instant::now()`
unconditionally, and `watchdog_beat()` does a relaxed atomic load at 20+ hot call sites.

**Decision:** `verter_audit::attribution` is the single surviving work-attribution authority; the
`loop5` counter half is Converge → Delete (owner `G4`), the watchdog is Preserve-and-relocate
(owner `K3`), backstop `L4`. Two in-crate tests assert on live counters and must be migrated, not
dropped.

The module is also program archaeology in production source (`//! Loop 5 …`, "the loop-5 brief",
"orchestrator memory", `_loop8_timer` locals). It escapes `no_phase_archaeology_in_production_code`
because that guard's trigger list has no `loop` root. **A5 declines to add one** — it would grow a
grandfathered name-keyed scanner and false-fire on every ordinary use of the word. The durable fix
is the deletion above.

### CI cannot run for this program, which decides A4's deferred debt

`.github/workflows/ci.yml` triggers on `push: branches: main` and `pull_request`. Ruling R-8 keeps
all program work local — nothing pushed, no PR, landing by local fast-forward. **No GitHub Actions
job executes for any program block.**

That inverts the obvious answer to A4's debt (the `attribution` and `compile-fail` features are
compiled by no automated gate). The natural precedent — the `svelte-oracle` CI job, which exists
because "the default `cargo nextest run --workspace` run never opts into this feature" — is
structurally unavailable. Wiring the gate instead is not a bounded change: a feature arm cannot ride
the existing archive variants without changing feature unification for all three surfaces, a third
variant is a third whole-workspace compile, and a correct addition needs a matching arm in the
7,170-line `scripts/gate-selftest.mjs`.

**Decision:** the three commands below become **required per-block commands locked by A6**, with
their output preserved as command proofs in the A1 form; the CI job is proposed for after the
program, requiring a ruling extending R-7.

```sh
cargo check --workspace --all-targets --features verter_audit/attribution
cargo test -p verter_audit --features attribution
cargo test -p verter_audit --features compile-fail
```

Stated honestly: this is weaker than a gate. It depends on the orchestrator running the set and the
reviewer checking the proof. That weakness is inherent to a program in which CI cannot run at all,
and it must be recorded in the A6 lock or it silently lapses.

### The ledger does not record where accepted blocks land

`program-state.toml`'s `[repository]` table records `branch = "main"`, `head_sha = 9af553dd…` — the
A0 entry checkout. A1–A4 landed on `program/architecture-lock`, now 15 commits ahead of `main`, and
no field distinguishes the two. A resuming agent reading `[repository]` alone would land onto `main`
and silently drop four accepted blocks. A6 adds an explicit integration-lineage field.

### 471 unlanded local branches, none of them a competing forward line

Of 520 local branches, 471 are not ancestors of `main`; two are program-own, leaving 469
candidates. **Every one of the 469 was cut from a merge-base at or before `2de3b2d07`**, i.e.
before the squashes that superseded them — that lineage bound is the test the disposition rests on,
and it holds without exception. The blunter net-deletion reading agrees for 468 of 469
(`agent/rc-integration` is +43,506 / −196,814, stale pre-squash WIP whose content already landed as
`main`'s squashed commits); the single exception, `port/rust`, is dispositioned individually — its
+370,822 is one 2,991,892-line generated artifact absent from `main`, and excluding that file it is
the population's largest net deletion (`open-changes.md` §2.1). None is a competing forward
authority. Recommended disposition: abandon as a class (recording the program's relationship only;
no branch deleted, no GitHub action), preserving `program/architecture-lock`, the active block
branch, and the two `origin/preserved/a2c-*` refs that R-10 requires be kept as failed historical
evidence.

## Decisions requiring maintainer ratification

Each assigns work into a later block's cutover closure or takes a program-relationship position, so
none is A5's to make alone. Full text in the linked artifacts.

| id | decision | artifact |
|---|---|---|
| A5-L1 | `loop5_instrumentation` Converge → Delete; owners `G4` (counters) / `K3` (watchdog); backstop `L4` | instrumentation-reconciliation §2 |
| A5-G1 | attribution/compile-fail arms become A6-locked per-block commands; CI job deferred post-program | instrumentation-reconciliation §3 |
| A5-DD1 | `verter_semantic → verter_workspace` recorded as an equality-pinned exception with `C1` as removal gate | dependency-direction §4 |
| R-12 (proposed) | the unlanded local branch population is abandoned as a class | open-changes §6 |
| S-1 | `max_open_stack_layers = 2`, `stack_mode_policy = ATOMIC_REVIEW`, `stack_tool = LOCAL_BRANCH_CHAIN` | program-operations-policy §5 |
| P-3 | the ledger gains an explicit integration-lineage field | program-operations-policy §2 |

## Claims this block does not make

Recorded so a reviewer does not have to infer the boundary:

- **`provider_protocol_version = 12`**'s producer is located and named
  (`crates/verter_protocol/src/consumer_compatibility_manifest.rs:75`, consumed at `:109`; the
  committed JSON is its test-pinned generated mirror, not an independent source). What is **NOT
  PROVEN** is whether that hand-pinned literal duplicates a compatibility domain owned elsewhere —
  ADR-002's forbidden "duplicate counter that must stay equal" — and why it is hand-maintained
  when `component_meta_schema_version`, three lines away in the same function, is sourced from its
  owner. Owner `H2`.
- **The ~455 older abandoned branches were not individually inspected.** A5 claims the class
  property (merge-base at or before `2de3b2d07`) and states the test that re-derives it. The
  corroborating net-deletion reading is recorded with its one exception (`port/rust`) named, not as
  a universal.
- **No claim that `flow_slice_content.rs` is architecturally final** — only that "second flow
  semantics path" is not a source-verified description of it. That judgment is `D2`'s.
- **Lifetime modelling of the consumer map is `E1`'s**, not A5's. A5 stops at enumeration plus
  compatibility posture, because extending further would pre-empt `E1` with decisions made without
  `C1`'s and `D2`'s results.

## Verification

No production source changed, so no test suite was run for this block; running one would prove
nothing about a documentation-only tree. Every factual claim is instead re-derivable from the
commands recorded beside it. The load-bearing ones:

| claim | command |
|---|---|
| loop5 counter census | the `node -e` census script in instrumentation-reconciliation §1.2 |
| crate closures and upward edges | `cargo metadata --format-version 1 --all-features`, walking `resolve.nodes[].deps`, skipping all-`dev` `dep_kinds` |
| `ProviderHub` absent | `grep -rn "ProviderHub" crates/*/src` |
| `EnvHashInputs` production sites | `grep -rn "EnvHashInputs {" crates/*/src \| grep -v _tests` |
| branch dispositions (load-bearing test) | per branch: `git merge-base main <b>`, then `git merge-base --is-ancestor <that> 2de3b2d07` |
| branch dispositions (corroborating, 468/469) | `git diff main <branch> --shortstat`; the `port/rust` exception is dispositioned in open-changes §2.1 |
| `provider_protocol_version` producer | `grep -rn PROVIDER_PROTOCOL_VERSION crates/` |
| attribution call sites | the per-crate `grep` loop in instrumentation-reconciliation §1.1 |
| proto message/enum counts | the `grep -cE '^[[:space:]]*(message\|enum) '` loop in consumer-protocol-map §2 |
| CI triggers | `.github/workflows/ci.yml:3-6` |

Guards whose scan surface includes this content, and which therefore constitute its real coverage —
the same honest accounting A0 gave for its documentation landing:

- `tracked_paths_are_portable` — enumerates `git ls-files`, enforces portable path shapes;
- `tracked_paths_no_machine_roots` — fixed-marker scan of tracked bytes for machine/user path roots;
- `every_critical_rule_in_docs_has_registered_guard` — reads `CLAUDE.md` and `.claude/skills/*/SKILL.md`
  only, so it does **not** see this directory; named because it is the guard a reader would assume covers it.

`no_phase_archaeology_in_production_code` scans `crates/*/src/**` and does not see this directory
either; the program-vocabulary prohibition on source is honoured here by A5 touching no source at all.
