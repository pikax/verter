# Architecture challenge reattestation — AMD-005 framework compiler conformance rescope

**SUPERSEDED AFTER REBASE.** This historical impact-bounded report remains bound to
`6920ddc6feed70cd4b25eb3b557ceac66c535939`, tree
`7d38eb20dd152433a469811be82a61ba200a38c3`; it does not review or approve the
rebased bytes. The current independent architecture report is
[`architecture-challenge.md`](architecture-challenge.md), bound to
`ce1d0e4688af1b5bd548b6b68286632cc0f7ede8`, tree
`1ff1f83d8e994b6f1169b0b209c9f557c23f4728`.

## Exact candidate binding

- Previous candidate commit: `8fbef4ba2ce30d93a636f769639519df7a773a92`
- Previous candidate tree: `eba511f865239ac27abf7da4fd3b4d292ed9ebec`
- Reattested candidate commit: `6920ddc6feed70cd4b25eb3b557ceac66c535939`
- Reattested candidate tree: `7d38eb20dd152433a469811be82a61ba200a38c3`
- Read-only implementation checkout spot-checked at commit:
  `e6035b433352b106957f27f3e97b71911f39f9ae`

This is an impact-bounded reattestation of the four blocking findings in
`architecture-challenge.md`, not a fresh review of the full package. The exact
previous-to-new candidate diff changes only:

- `docs/arch/refactor/rev11/charters/BV1.md`;
- `docs/arch/refactor/rev11/charters/C3.md`;
- `docs/arch/refactor/rev11/contracts/fragment-assembly.md`; and
- `docs/arch/refactor/rev11/evidence/framework-conformance/emitter-mapping-dispositions.tsv`.

## Reattestation

1. **BV1/C3 acceptance deadlock — resolved.** `charters/BV1.md:29-37` now
   defines `FC-TS-001-LOCAL` as BV1's independently closable producer/local
   criterion and expressly requires neither C3 nor a live project resolver. BV1's
   required exit at `charters/BV1.md:41-45` requires that local criterion while
   leaving jointly owned project-aware cells pending. `charters/C3.md:78-86`
   separately makes `FC-TS-001-PROJECT` a C3 exit and says that it closes
   `FC-TS-001` for the jointly owned Vue cells. This does not relocate the cycle:
   the unchanged DAG remains BV1 → B5 → C2 → C3, but no BV1 prerequisite now
   depends on the later C3 proof.

2. **BV1→C3 demand protocol — resolved.** `charters/C3.md:12-21` closes the
   common demand fields and typed subject vocabulary; `:23-31` exhaustively names
   `MacroPayload` and `PropsWithDefaults`, their roles, lanes, subjects, and subject
   ordering; and `:39-58` defines the success payloads plus closed `Success`,
   `NotFound`, `Stale`, and `Error` result vocabularies. `:60-68` binds deterministic
   identity, canonical demand/result order, one result per planned identity, and
   whole-batch rejection for missing, extra, duplicate, reordered, or mismatched
   facts. Finally, `:70-76` requires omitted or top-level degraded results to become
   typed `ProjectProjectionUnavailable` non-success with no publication, and
   prohibits empty or member-dropping silent success. BV1's local criterion also
   requires deterministic stubs to exercise all of these arms at
   `charters/BV1.md:29-35`.

3. **Fragment-assembly mapping contradiction — resolved.**
   `contracts/fragment-assembly.md:23-30` now distinguishes contract-required parts
   from optional map products. An IDE/provider companion implicitly requests its
   non-optional `SourceProjectionMap`, and the pair publishes atomically; only
   `RuntimeSourceMapData` and terminal `EncodedSourceMap` remain request-controlled.
   This agrees directly with `contracts/mapping-products.md:10-15` and `:29-36` and
   no longer allows a required IDE map to be treated as optional.

4. **Emitter/mapping disposition completeness — resolved for all three named
   missing owners.** The ledger adds real, separately owned rows at
   `emitter-mapping-dispositions.tsv:39-41`:

   - EM-038 gives `assemble_vue_main_module` a `Replace` disposition owned by
     BV1+B4+B5 and requires removal of the second session assembly path. The
     read-only main checkout confirms the symbol at
     `crates/verter_session/src/compile.rs:21-26`, with style/custom imports,
     template imports, script rewriting, render attachment, and HMR assembly at
     `:34-134`.
   - EM-039 gives the primary Svelte client-module owners a `Converge` disposition
     owned by BS1+B4. The cited files exist: `client.rs:98-115` enters the module
     emitter and `:353-469` emits imports, module/body/epilogue content, and the
     source map; `client_module_frame.rs:1-97` owns the import prelude and root
     factory.
   - EM-040 gives session `compile_entry` a `Replace` disposition owned by B4+B5+C4
     and explicitly removes host reconstruction/map reattachment as a second
     assembly owner. The cited function exists at
     `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:2751-2767`;
     `:2996-3144` assembles Main/Script/Template/Style/Custom and IDE artifacts,
     injects Template imports, and selects/attaches maps.

The bounded sanity pass found no new blocking contradiction, ownership gap, or
acceptance cycle in the four changed files. The candidate diff passes `git diff
--check`; the disposition ledger has 40 unique six-column rows, all with a valid
disposition and non-empty acceptance owner.

## Verdict

PASS — all 4 original findings resolved, no new blocking issue introduced, bound to commit `6920ddc6feed70cd4b25eb3b557ceac66c535939`.
