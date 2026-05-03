# God-module audit tool

Per migration plan §2.1.0, Tier 0 Step 0.0.

## Run command

```bash
cargo test -p verter_session --test architecture_guards \
    god_module_audit -- --nocapture --ignored
```

## What it does

Extends the existing `syn`-AST scanner used by `architecture_guards.rs` (`render_type`,
`no_off_store_host_caches_inner`, `no_phase_archaeology_in_production_code`,
`god_module_size_budget`) to also dump per-file:

- **SCC table** — strongly-connected components of the intra-file function call graph.
  One row per SCC: `{ file, scc_id, members: Vec<fn_name>, size, recursive: bool }`.
- **Budget-edge table** — every call where the callee's pinned recursion budget is
  read or decremented. One row per edge: `{ file, caller, callee, budget_kind, line }`.
- **Cache-identity edge table** — every read/write to an `_*Db` cache. One row per:
  `{ file, fn, db_name, op: "read" | "write", line }`.
- **Public-surface edges** — `pub fn` exits visible outside the crate.
- **Cross-file shared-cache edges** — calls that publish into `ProjectTypeStore`,
  `SemanticGraphStore`, or any `*Db` accessor.

## Output location

`docs/arch/debt-closure/13-god-module-split-audit/<module>.md` per Step 0.3.

## Smoke gate

The tool runs against `crates/verter_session/src/host_resolve.rs` and produces a
non-empty SCC table before any audit document is committed. Empty SCC tables
indicate a parse/visit failure rather than a clean module.

## Phase classifier (D111)

A grep hit is "archaeology" (sweep target) if its line OR surrounding 3 lines
contains ANY of:

- `Plan §` or `plan §`
- `rev <N>` or `revision <N>` (decimal N)
- `Phase <N>` where N is decimal — disambiguator: if followed by colon-prefixed
  verb (`Phase 1: collect…`), it's algorithm-phase (preserve).
- `post-cutover` or `d-cutover`
- `deleted in 5g` or `phase-archaeology`
- Git artifact: SHA-like 7-40 hex chars adjacent to `commit`/`branch`/`merge`

Otherwise: algorithm-phase (preserve).

## Targets covered

| File | LOC (validation SHA `60b1295a`) | Owner |
|---|---|---|
| `crates/verter_session/src/semantic_query_memo.rs` | 5765 | Tier 2 W5a |
| `crates/verter_parser/src/utils/oxc/vue/script/resolve_type.rs` | 5597 | Tier 2 W5b |
| `crates/verter_session/src/host_resolve.rs` | 4186 | Tier 2 W5c |
| `crates/verter_session/src/resolver_core/component_meta.rs` | 3948 | Tier 2 W5d |
| `crates/verter_ffi/src/convert.rs` | 3783 | Tier 2 W5e |
