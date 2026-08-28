# A5 — Surviving instrumentation owners

A5 is chartered to "reconcile open changes and **surviving instrumentation owners**". A4 landed
`verter_audit::attribution` onto a tree that already contained a second, older, unmanaged
instrumentation surface. This file decides which survives, on what boundary, and who executes
the change. It also settles the gate-coverage debt A4 explicitly deferred to A5.

Nothing here is executed as a source change by A5 — see [§4](#4-why-a5-changes-no-source).

---

## 1. The two owners, as they exist today

### 1.1 `verter_audit::attribution` (landed by A4)

`crates/verter_audit/src/attribution/` — a closed `WorkSite` enum (one macro invocation, stable
dotted ids, a `WorkDomain` and a `WorkUnit` per site), a dense atomic table, a scope guard, an
attributing allocator, and deterministic renderers. **91 call sites across seven crates**:

| crate | `attribute*!` invocations |
|---|---|
| `verter_session` | 50 |
| `verter_compiler` | 13 |
| `verter_audit` | 12 |
| `verter_scheduler` | 6 |
| `verter_workspace` | 5 |
| `verter_napi` | 3 |
| `verter_wasm` | 2 |
| **total** | **91** |

Counted as macro *invocation* lines under `crates/*/src`, excluding doc-comment mentions and the
five `macro_rules!` definitions themselves — those reference the macros, they are not sites that
attribute work. Exactly seven crates contain an invocation and no others do. Reproduce:

```sh
for c in verter_session verter_audit verter_compiler verter_scheduler \
         verter_workspace verter_napi verter_wasm; do
  printf '%s %s\n' "$c" "$(grep -rEn 'attribute[a-z_]*!\(' crates/$c/src \
    | grep -vE ':\s*(///|//!)' | grep -v macro_rules | wc -l)"
done
# completeness: grep -rEln 'attribute[a-z_]*!\(' crates/*/src | cut -d/ -f2 | sort -u
```

One qualification the raw total hides: all 12 `verter_audit` invocations are inside the substrate's
own test modules (`attribution/table_tests.rs`, `attribution/disabled_tests.rs`). The leaf does not
self-instrument in production, so the *instrumented production surface* is the other six crates'
79 sites.

Everything that can produce a number is behind the non-default `attribution` feature, so a
production build cannot branch on a counter — the path does not resolve. Proven from outside the
crate by a `compile-fail` trybuild fixture.

### 1.2 `crates/verter_session/src/loop5_instrumentation.rs` (1,121 lines, pre-existing)

Declared `pub mod loop5_instrumentation` at `crates/verter_session/src/lib.rs:311`, unconditionally
compiled, no feature gate. 46 `pub static` atomics, a `TimerGuard`, a JSON dump function, and a
separate backtrace-watchdog subsystem. 64 references across the workspace.

It is **two unrelated facilities in one module**, and that is the key to the disposition:

| part | what it is | live? |
|---|---|---|
| work counters | atomics bumped at named dispatch/materialize call sites, dumped as JSON by `dump_loop5_instrumentation_counters` | 18 of 46 statics |
| dead counters | declared, reset, loaded, and emitted into the JSON dump — but **never incremented anywhere** | 24 of 46 statics |
| watchdog | `watchdog_beat` / `watchdog_check_and_dump` / `spawn_watchdog_with_mode`, a stall/sample thread that forces a `Backtrace::force_capture()` to stderr | 4 statics, 20+ hot call sites |

Per-static census: [`loop5-counter-census.tsv`](loop5-counter-census.tsv). Reproduce the raw
reference counts with:

```sh
node -e '
const fs=require("fs"),cp=require("child_process");
const src=fs.readFileSync("crates/verter_session/src/loop5_instrumentation.rs","utf8");
for(const m of src.matchAll(/pub static ([A-Z0-9_]+)\s*:/g)){
  let h=""; try{h=cp.execSync(`grep -rn --include="*.rs" "${m[1]}" crates/ | grep -v loop5_instrumentation.rs`,{encoding:"utf8"})}catch(e){}
  console.log(m[1], h.split("\n").filter(Boolean).length);
}'
```

That command reports **26** statics with zero references outside the module. Two of the 26 —
`WATCHDOG_DUMP_SERIAL` and `WATCHDOG_ACTIVE` — are live: the module's own watchdog helpers drive
them. Subtracting those leaves **24 genuinely never-incremented statics**, the number used
throughout this file.

The subtraction is exhaustive rather than a judgement call, because the module contains only
**five** increment sites in total (`grep -n "fetch_add\|fetch_max"`): two inside `TimerGuard`
(which takes its counters as `&'static AtomicU64` parameters, so its targets are named at external
call sites), one in `watchdog_beat`, and two on `WATCHDOG_DUMP_SERIAL` in the two watchdog loops.
A static that is neither a `TimerGuard` argument nor incremented at an external call site nor one
of the four watchdog statics cannot be incremented at all.

**The 24 dead counters are not merely unused — they are actively misleading.** They are emitted
into the JSON dump with value `0`, and a `0` in a counter report reads as "this work did not
happen", not as "this counter was never wired". A4 deleted five guessed sites for exactly this
reason ("a counter wired to a near-miss is worse than an absent one, because it reads as
covered"). The same standard applied to the pre-existing module condemns 24 rows. There is also a
`#[cfg(test)] fn dump_emits_all_keys` that asserts every one of those key names appears in the
dump — a test that passes precisely *because* the dead rows are still emitted, so the module's own
test suite pins the misleading output in place.

**The live counters overlap A4's sites directly**, at the same chokepoints:

| loop5 static | site | A4 `WorkSite` | A4 site file |
|---|---|---|---|
| `EXECUTE_COOPERATIVE_CALLS` | `semantic_query_memo/mod.rs:2066` | `session.semantic_dispatch` | `project_semantic_dispatch/mod.rs:2214` |
| `EXECUTE_COOPERATIVE_COLD_BUILDS` | `semantic_query_memo/mod.rs:2753` | `session.semantic_cold_build` | `project_semantic_dispatch/mod.rs:2822` |
| `EXECUTE_COOPERATIVE_WARM_HITS`, `FAMILY_MEMO_HITS` | `semantic_query_memo/mod.rs:2239-2243` | `session.semantic_warm_hit` | `project_semantic_dispatch/mod.rs:2824` |
| `MATERIALIZE_STRUCTURE_CALLS` / `_NS` | `component_meta_materialize.rs:975` | `session.materialize_structure` | `component_meta_materialize.rs:979` |
| `SLOT_BINDING_EXPANDED_INSTANTIATE_CALLS` | `project_semantic_dispatch/build.rs:2620` | `session.instantiate` | `project_semantic_dispatch/build.rs:2587` |

`component_meta_materialize.rs` carries **both** a `loop5` `TimerGuard` and an
`attribute_scope!` **four lines apart**. That is the second-owner condition the reconciliation
contract exists to catch.

**Cost.** `loop5` is not zero-overhead the way A4's disabled arm is: `TimerGuard::new` calls
`Instant::now()` unconditionally on every entry, and `watchdog_beat()` performs a relaxed atomic
load at every one of its 20+ hot call sites. A4 measured its own OFF arm as not measurable; this
module has no OFF arm at all.

---

## 2. Decision L1 — one surviving work-attribution owner

**`verter_audit::attribution` is the single surviving work-attribution authority. `loop5`'s
counter half is a second owner and is dispositioned Converge → Delete. It is not extended, not
re-homed, and no new counter is added to it.**

Executed in three separable parts, because they have different owners and different risk:

### L1a — the 24 dead counters: **Delete**

No producer, no consumer, no test outside `dump_emits_all_keys` (which is deleted with them).
Deleting them is mechanical and removes a report surface that reads as measured-zero. This part
has **no dependency on any later block** and no semantic consequence.

### L1b — the 18 live counters: **Delete after their two test consumers are migrated**

Two in-crate tests assert on them and are the only reason this is not also mechanical:

- `crates/verter_session/src/semantic_query_memo/tests.rs:6287-6332` asserts
  `WARM_HIT_FAST_PATH_HITS` records at least one fast-path warm hit;
- `crates/verter_session/src/component_meta_no_cache_promotion_tests.rs:456,480` reads
  `EXECUTE_COOPERATIVE_CALLS` across a normalize boundary.

Both are genuine discriminating assertions and must not be dropped. Migrating them to
`verter_audit::attribution` reads **couples this part to Decision G1 below**: the attribution
reader surface is feature-gated, so a test that reads a counter only compiles under
`--features attribution`, and a `verter_session` test that is never compiled with that feature is
a test that silently stops running. Whoever executes L1b either (a) lands G1 first so the arm is
built, or (b) restates both assertions behaviourally (observable warm/cold outcome) rather than
by counter. **(b) is the architecturally preferable form** — a counter assertion is an assertion
about instrumentation, not about behaviour — but the choice belongs to the executing block, and
either way the assertions may not weaken.

### L1c — the watchdog: **Preserve, but relocate; it is not attribution**

`watchdog_beat` / `watchdog_check_and_dump` / `spawn_watchdog_with_mode` are a **debugging
facility**, documented as such by the `/debug-tooling` skill, not a work-attribution rail. They
do not count work; they detect a stalled thread and print a stack. Folding them into
`verter_audit::attribution` would be wrong on the substrate's own terms — attribution is
measurement-only leaf counting with no side effects, and a stderr-writing backtrace dumper that
spawns a thread is neither leaf-only nor side-effect-free.

So the watchdog **survives as its own owner**, and the disposition is placement, not deletion: it
must stop living in a `verter_session` module named for a retired investigation. Its 20+ call
sites are in `project_semantic_dispatch/locator_view_worklist.rs` and `lower.rs`; a relocation
that keeps the call sites and changes the module path is mechanical.

### L1d — the program-archaeology finding

`loop5_instrumentation.rs` opens with `//! Loop 5 — performance instrumentation counters for
ChatMessage cold-path investigation`, refers to "the loop-5 brief" and to "Hypothesis attribution
mapping (orchestrator memory)", and its call sites bind locals named `_loop8_timer`. This is
exactly what `CLAUDE.md` → *No phase archaeology in production code (MANDATORY)* forbids.

It survives today because the guard's trigger list
(`PHASE_ARCHAEOLOGY_LOWER_ROOTS`, `crates/verter_session/tests/cases/architecture_guards.rs:7577`)
contains no `loop` root — verified by reading the list. **A5 explicitly declines to add one.**
Adding `"loop"` would (a) grow a grandfathered name-keyed source scanner, which `CLAUDE.md`'s
forward-only rule permits only as retained legacy and never as new landed enforcement, and
(b) false-fire on every ordinary use of the word "loop" in ~600k lines. The durable fix is L1a-c:
the module ceases to exist under that name.

### L1e — owners and resolution gates

| part | durable owner block | rationale | resolution gate |
|---|---|---|---|
| L1a (dead counters) | `G4` — Cache/store convergence | `G4` classifies every current store and rewrites `SemanticGraphStore`, the surface these counters instrument; the deletion is inside its cutover closure | `G4` accepted candidate |
| L1b (live counters) | `G4` | same closure; the two test consumers both assert on `SemanticGraphStore` warm/cold behaviour | `G4` accepted candidate |
| L1c (watchdog relocation) | `K3` — Reduce/retire `VerterHost` and catch-all session ownership | a debug facility parked in the session crate is precisely the catch-all ownership `K3` exists to reduce | `K3` accepted candidate |
| backstop | — | — | `L4` — Final architecture lock. No part of L1 may survive `L4`. |

These owners are **A5's recommendation and require maintainer ratification**, because assigning
work into a later block's cutover closure changes that block's scope. They are recorded as debt
rows so the disposition is not a `TODO` (`CLAUDE.md` → Explicit finding disposition):

```text
DEBT A5-L1  Disposition: DEFER
  Finding:          two work-attribution owners; 24 never-incremented counters emitted as zeros
  Durable owner:    G4 (L1a, L1b), K3 (L1c)
  Resolution gate:  the owning block's accepted candidate; hard backstop L4
  Acceptance:       loop5_instrumentation.rs absent; the two migrated assertions still
                    discriminate (fail against the pre-change tree); the watchdog reachable
                    from its new owner with its call sites intact
  Ruling reference: PENDING — requires a maintainer ruling in the shape of R-5/R-9
```

---

## 3. Decision G1 — A4's deferred gate-coverage debt

A4 recorded (evidence/A4-summary.md, "Known gaps and deferrals"): neither the `attribution`
feature nor the `compile-fail` feature is compiled or run by `node scripts/gate.mjs`, so the
enabled arm's amount expressions and the trybuild reader-absence seal can both rot silently.
**Debt owner: A5.** Settled here.

### 3.1 The fact that decides it

**CI never runs for this program.** `.github/workflows/ci.yml` triggers on `push: branches: main`
and on `pull_request`. Maintainer ruling **R-8** states that all Revision 11 work stays local —
nothing is pushed to `origin`, no PR is opened, and landing is a local fast-forward. Therefore no
GitHub Actions job executes for any program block, and a CI job is **not** a coverage mechanism
available to this program.

This inverts the obvious answer. The `svelte-oracle` job
(`.github/workflows/ci.yml:512-547`) is the exact precedent one would reach for — it exists
because "the default `cargo nextest run --workspace` run never opts into this feature", and it
runs `cargo test -p verter_compiler --features svelte-oracle`. Structurally identical need;
structurally unavailable venue. A CI job would also need maintainer authorization, since R-7
authorized exactly one narrow `.github/` edit for a different purpose.

### 3.2 Why not wire it into `scripts/gate.mjs`

The gate's build model is variant-based: each variant is a whole-workspace
`cargo nextest archive` build, and the two existing variants already cost two full workspace
compiles (`scripts/gate.mjs:794-817`). A feature arm cannot ride the existing variants —
`--features verter_audit/attribution` on the workspace archive changes feature unification for
*every* surface, so surfaces 1-3 would no longer measure the shipped feature set. A third
variant is a third whole-workspace compile for two small crates' worth of coverage.

A package-scoped step (`cargo test -p verter_audit --features attribution`) is cheap —
`verter_audit` depends only on `verter_span` — but contradicts the gate's stated design rule that
every build it issues is a `--workspace` archive build, and a correct addition would need a
matching arm in `scripts/gate-selftest.mjs` (7,170 lines, with the `(GB9)` six-direction
discrimination pattern as the standard). That is not a bounded change for an inventory block, and
it modifies the instrument every later block's evidence depends on.

### 3.3 The decision

**The two feature arms become required per-block commands in the program's command set, locked by
A6; the durable post-program home is a CI job, proposed but not landed.**

Required commands, run per block on the exact candidate tree, with output preserved as command
proofs in the same form A1 established:

```sh
cargo check --workspace --all-targets --features verter_audit/attribution
cargo test -p verter_audit --features attribution
cargo test -p verter_audit --features compile-fail
```

The first is the arm A4 proved is load-bearing: the disabled arm does not type-check the amount
expressions, and A4 hit exactly that error. The third executes the trybuild seal that proves the
reader path is absent — the negative control for the whole no-semantic-authority claim.

Properties, stated honestly:

- This is a **standing obligation for the program's duration**, not a standing guard. It is
  weaker than a gate: it depends on the orchestrator running the command set and on the reviewer
  checking the proof. That weakness is inherent to a program in which CI cannot run at all, and
  it is the same instrument A1 already established for every other command.
- It costs approximately nothing: `verter_audit` + `verter_span` is a two-crate compile.
- It **must** be listed in the A6 Implementation Lock Record's command/capability evidence, or it
  silently lapses. That is A6's acceptance criterion, not a hope.

Proposed durable home, for the maintainer, to land **after** the program (or earlier with an
explicit ruling): a `rust-audit-features` job in `.github/workflows/ci.yml` modelled on
`svelte-oracle`, running the three commands above, with `crates/verter_audit/**` added to the
`rust` change-detection path filter. Requires a maintainer ruling extending R-7.

```text
DEBT A5-G1  Disposition: DEFER (durable CI home only; the per-block command set is ADOPT-NOW)
  Finding:          attribution / compile-fail features never compiled by any automated gate
  Durable owner:    maintainer (a ruling extending R-7), executed post-program
  Resolution gate:  A6 lock records the per-block commands; the CI job no later than L4
  Acceptance:       a deliberate type error in an enabled-arm amount expression fails the
                    command set; deleting the compile-fail fixture's `#[cfg]` gate fails it
  Ruling reference: PENDING
```

---

## 4. Why A5 changes no source

A5's in-scope clause is "evidence and source changes **strictly necessary to produce those
deliverables**". Every change contemplated above — deleting counters, relocating a watchdog,
editing `scripts/gate.mjs`, editing `.github/workflows/ci.yml` — is a production or
gate-infrastructure change that is *decided* by this block and *executed* by another. Executing
any of them here would be a Foundational-class production change landing inside an inventory
block, with no charter of its own, no scoper, and no architecture challenge. The charter's
abort/rescope clause names exactly this shape.

Consequently: **A5 modifies no file under `crates/`, `packages/`, `scripts/`, or `.github/`.**
Its deliverable is this decision record.
