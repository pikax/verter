# A5 — Dependency-direction test strategy (locked)

A5 locks the strategy; `B1` implements it ("Land the distinct identity/profile/mapping/
result-contract types and **forbidden dependency-edge build tests**"). This file fixes *what kind
of test*, *what it must reject*, and *what it replaces* — so `B1` cannot satisfy the requirement
with a source scanner and cannot discover the real current violations mid-implementation.

Binding authority: [`ADR-015 — Core Dependency Direction Is Inward and Cycle-Free`](../../decisions/ADR-015-binding-dependency-direction.md).

---

## 1. What enforces direction today

| mechanism | location | kind |
|---|---|---|
| `dependency_closure_guard` | `crates/verter_macro_dto/tests/cases/dependency_closure_guard.rs` | **structural** — walks the real resolved graph from `cargo metadata --format-version 1 --all-features`, follows normal+build edges transitively, equality-pins the sanctioned `oxc_span` subtree |
| `verter_audit_no_upward_deps` | `crates/verter_session/tests/cases/architecture_guards.rs:11846` | TOML text scan of one manifest |
| `audit_substrate_isolation` | `…architecture_guards.rs:11915` | source-text scan of `crates/verter_audit/src` for `verter_*` tokens |
| `scheduler_does_not_depend_on_verter_session` / `scheduler_src_has_no_verter_session_path` | `crates/verter_scheduler/tests/cases/no_session_dep.rs` | manifest substring + source-text scan |
| cargo itself | — | rejects dependency **cycles**; nothing else |

Coverage today is **four crates out of thirty-nine** (`verter_macro_dto`, `verter_audit`,
`verter_scheduler`, plus cargo's global cycle rule). Direction between the other thirty-five is
enforced by review only.

## 2. The locked strategy

**One `cargo metadata`-driven forbidden-edge test over the whole workspace graph, modelled
exactly on `verter_macro_dto`'s `dependency_closure_guard`. No new source-text scanner.**

This is not a stylistic preference; it follows from `CLAUDE.md`'s forward-only rule: *landed*
enforcement is compiler/type-system/tool-based and never a name/text/grep scanner over the source
tree. Three of the four mechanisms above are exactly the scanner form that rule grandfathers but
forbids extending. The fourth is the model to generalise, and it is already the better test on the
merits:

- it reads what cargo actually links, so a forbidden edge added **behind any feature**, or
  arriving **transitively**, fails — a manifest scan sees none of those;
- it distinguishes production edges (normal + build) from dev-dependencies, so a guard's own
  tooling dev-deps cannot self-trip it;
- it pins its sanctioned exception by **equality** against a structurally computed set, so an
  upstream release that grows the exception is a re-audit gate rather than a silent grant.

Naming crates in a layer matrix is not the scanner failure mode the rule forbids. The rule's
target is text matching over source; a resolve-graph walk over cargo's own output is the
"real used tool" form it prescribes. The matrix is data for a graph query, not a grep pattern.

### 2.1 Required properties of the `B1` test

1. **Input** — `cargo metadata --format-version 1 --all-features`, the resolve cargo builds from.
2. **Edges** — normal + build, transitive. Dev edges excluded and that exclusion stated.
3. **Assertion form** — for each crate, its production closure must not reach any crate in a
   strictly higher layer. Closure-based, not direct-edge-based: a direct-edge test passes while a
   two-hop violation walks straight through it (§4 shows this is not hypothetical).
4. **Exceptions** — equality-pinned, never subset-checked, each with a recorded rationale.
5. **Discrimination** — the test must FAIL when a forbidden edge is added. `B1` proves it by
   adding one to a scratch manifest and observing the failure, and the proof must show the
   mutation was actually applied (`CLAUDE.md` → Verification Must Prove Execution: a plant that
   fails to apply reports a pass).
6. **Supersession, with one residue that must be decided rather than assumed.** Landing the
   closure test deletes `verter_audit_no_upward_deps` and both tests in `no_session_dep.rs`:
   their invariants are strictly implied by the walk (`verter_audit` closure ⊆
   {`verter_audit`, `verter_span`}; `verter_scheduler` closure excludes `verter_session`), and
   a source path reference to a non-dependency crate does not compile, so the source-scan half
   adds nothing a closure walk misses. Keeping them would be a second authority for one rule,
   and deleting them in the same accepted candidate is `CLAUDE.md`'s clean-cutover requirement.

   `audit_substrate_isolation` is **not** fully implied, and `B1` must not delete it on the
   assumption that it is. Its *dependency* half is implied. Its residue is a **naming** rule: it
   rejects any `verter_*` token on a non-comment line in `crates/verter_audit/src`, including
   ones that are not dependencies at all — that is exactly what it caught during A4, when the
   scope macro's local binding `_verter_attribution_scope` tripped it. A closure walk cannot see
   a local variable name. So `B1` decides explicitly: either the naming rule is worth keeping as
   a separate, named, grandfathered guard, or it is dropped as coverage the program does not
   need. Silently deleting it and calling the closure test a superset would lose real coverage.

## 3. A5's proposed layer assignment

ADR-015's chain, resolved to this workspace's crates. **`B1` ratifies this; A5 proposes it and
supplies the evidence.**

| layer | ADR-015 name | crates |
|---|---|---|
| 1 | identity / span / language / contracts | `verter_span`, `verter_language`, `verter_ecma`, `verter_analysis_inputs`, `verter_audit`, `verter_no_typeexpr`(+`_derive`), `verter_no_storedspan`(+`_derive`) |
| 2 | shared syntax frontends + dependency-neutral DTOs | `verter_type_expr`, `verter_type_expr_oxc`, `verter_parser`, `verter_css_syntax`, `verter_macro_dto`, `verter_session_query` |
| 3 | semantic kernel / module resolver / relation / flow | `verter_semantic`, `verter_diagnostics`, `verter_actions` |
| 4 | compiler | `verter_compiler` |
| 5 | managed engine / session | `verter_session`, `verter_workspace`, `verter_scheduler`, `verter_tsgo_api`, `verter_type_runtime`, `verter_protocol` |
| 6 | adapters | `verter_lsp`, `verter_napi`, `verter_wasm`, `verter_ffi`, `verter_mcp`, `verter_mcp_server`, `verter_tsc`, `verter_relay_shim`, `verter-editor-client` |
| 7 | harnesses (outside ADR-015; no crate may depend on these) | `verter_bench`, `verter_dx_baseline`, `verter_vue_conformance`, `verter_svelte_conformance`, `verter_session_oracle_macro` |

Two placements need justification because they are the ones a reviewer will challenge:

- **`verter_protocol` at layer 5, not layer 2.** ADR-015's layer 2 is *dependency-neutral* DTOs.
  `verter_protocol` depends on `verter_semantic` (7 references, all
  `analysis::component_meta::OrderedSfcStructureAnalysis` plus `refs`), so it is not neutral. It
  is a transport projection above the kernel. `verter_macro_dto` is the crate that actually
  satisfies layer 2's neutrality, and its closure guard proves it.
- **`verter_audit` at layer 1.** Its production closure is exactly
  `{verter_audit, verter_span}` — verified by closure walk. It is a leaf by construction.

## 4. The two upward edges in the current tree

Running the proposed matrix over the resolved graph yields **exactly two** production edges that
point upward:

```text
L3 verter_semantic     ->  L5 verter_workspace   (normal)
L3 verter_diagnostics  ->  L5 verter_workspace   (normal)
```

`verter_semantic` uses `verter_workspace::WorkspaceRead` (6 sites), `resolver`, `types`,
`fact_registry`, and `FilesystemWorkspace`.

**This is not repairable by re-layering.** The tempting fix — move `verter_workspace` down to
layer 2 — fails, because `verter_workspace` itself depends on `verter_scheduler` and
`verter_tsgo_api`. The two are declared differently, and `B1` must not conflate them
(`crates/verter_workspace/Cargo.toml`):

| dep | declaration | reach |
|---|---|---|
| `verter_scheduler` | `[dependencies]`, line 37 | unconditional — every target |
| `verter_tsgo_api` | `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`, line 49 | native only; absent from a `wasm32` build |

So the edge is real on every target, and its consequence is measurable — but the `verter_tsgo_api`
half of that consequence is **native-only**:

```text
verter_semantic's production closure (--all-features, normal+build, transitive):
  verter_audit  verter_css_syntax  verter_ecma  verter_language
  verter_no_storedspan(+_derive)  verter_no_typeexpr(+_derive)  verter_parser
  verter_scheduler  verter_span  verter_tsgo_api  verter_type_expr
  verter_type_expr_oxc  verter_workspace
```

That closure is the `--all-features` **native** resolve, which is what `cargo metadata` reports by
default and what the closure test will read.

**The semantic kernel's production closure reaches the task scheduler unconditionally, and the
external tsgo API on native targets.** ADR-015's stated consequence — "semantic kernel remains
reusable across lifecycles" — does not hold today: linking the kernel links a task scheduler on
every target, and additionally an out-of-process tool client on everything but `wasm32`. The
platform split narrows the second half of the violation; it does not repair either half, because a
`wasm32`-only firewall is not a firewall.

Compare the crates that *do* satisfy their firewall:

```text
verter_audit      -> verter_audit  verter_span
verter_macro_dto  -> verter_macro_dto  verter_no_storedspan(+_derive)  verter_no_typeexpr(+_derive)  verter_span
```

Reproduce all three with the closure walk over `cargo metadata --format-version 1 --all-features`,
following `resolve.nodes[].deps` and skipping deps whose `dep_kinds` are all `dev`.

### Disposition

```text
DEBT A5-DD1  Disposition: DEFER
  Finding:          verter_semantic (and verter_diagnostics) depend on verter_workspace, so the
                    semantic kernel's production closure reaches verter_scheduler (unconditional)
                    and verter_tsgo_api (native only — declared under
                    cfg(not(target_arch = "wasm32")) in verter_workspace/Cargo.toml),
                    contradicting ADR-015's reusability consequence
  Durable owner:    C1 (converge ModuleResolverCore / non-flow TypeInfoCore — the block that
                    decides what the kernel's input surface is), with B1 owning the test that
                    makes the violation fail rather than be reviewed for
  Resolution gate:  C1 accepted candidate. B1 MAY land its test with this pair as a recorded,
                    equality-pinned exception; it MUST NOT land it as a subset-checked allowance,
                    and the exception must name C1 as its removal gate.
  Acceptance:       verter_semantic's closure contains neither verter_scheduler nor
                    verter_tsgo_api on ANY target; the closure test fails if either returns.
                    B1's equality-pinned exception must therefore record the target condition
                    alongside the edge, so a wasm32-only resolve cannot read as satisfied.
  Ruling reference: PENDING
```

The `B1`-may-land-with-exception clause matters: without it, `B1` either cannot land its own exit
criterion or is tempted to weaken the test to green. The equality pin plus a named removal gate is
what keeps a temporary exception from becoming permanent.

## 5. What the strategy deliberately does not cover

- **Intra-crate direction.** The closure walk sees crates. `verter_session` is 5 of the 6 layers
  by volume, and the module-level direction inside it is not testable this way. ADR-015 is
  explicit that "logical owners do not automatically require crates; use modules/functions until
  a real dependency firewall … exists" — so intra-crate direction is held by the type system
  (privacy, sealed traits, the `NoTypeExpr` marker) and by review, not by this test. `K3` is the
  block that turns the module boundaries that matter into crate boundaries.
- **Trait-object back-edges.** A closure walk cannot see a callback that inverts control without
  inverting the dependency. ADR-015 rejects "mutual compiler/semantic callbacks" for this reason;
  detecting them is a review mandate, not a graph query.
