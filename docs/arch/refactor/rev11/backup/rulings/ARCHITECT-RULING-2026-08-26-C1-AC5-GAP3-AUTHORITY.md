# C1 AC-5 / GAP-3 authority

## Dispatch record

- Input ID: `C1-AC5-AUTHORITY-2026-08-26-01`
- Model: `gpt-5.6-sol`
- Reasoning effort: `xhigh`
- Transport: Codex CLI `codex exec`, read-only sandbox
- Exit status: `0`
- Reviewed candidate: `b82ddb421480eef4718a8a0defaa254b7c946180`
- Reviewed tree: `d3c908ac58cd3bc9300d30bcebba5f0ba5d92705`
- Registered integration authority: `b9a1b5b2f5e6d689de89447ebc00cc37f9f6453b`
- Prompt SHA-256: `5d383b3388bc90eae8fd20df7cb3c066201809567f4de861e5c3042bb597ff9a`
- Raw output SHA-256: `d0b450f23f6a2c81c923d466195f27c54177733c6588b6f2139d826c94cef396`

The ruling below is the last `## Ruling` final-answer block in the raw output, reproduced exactly. Earlier prompt, template, and trace receipts in the raw output are not part of this ruling.

## Ruling

PASS. The correct disposition is option 3: ratify an explicit C1→C2 boundary amendment whose operational result is option 2.

C1 must not fabricate the absent gateway, and the present evidence must not call GAP-3 complete. The completed `ModuleResolverCore` cutover is accepted separately; the closed C2-facing `TypeInfoCore::attempt(NonFlowOperation)` contract is deferred to C2 under a new, distinct obligation:

`C2-AC-C1-GAP3-TYPEINFO-GATEWAY-001`

This ruling is not operative until the repository acts below are registered. Until then, C1 remains blocked.

### Binding invariants

1. Revision 11 charters and rulings override local architectural descriptions, and an impossible plan requires a recorded deviation—not a local substitute. `CLAUDE.md:3-11`.
2. There is exactly one type-resolution engine: `SemanticQueryKey → ProjectSemanticDispatch::execute → SemanticGraphStore`. `CLAUDE.md:25-40`.
3. The current addendum preserves full non-flow extraction and behavioral `AttemptOutcome` coverage; it was expressly forbidden from silently shrinking C1. `ARCH-ADDENDUM-C1-THREE-GAPS.md:17-26`.
4. GAP-3’s current binding mechanism is one inherent `TypeInfoCore::attempt`, a closed `NonFlowOperation`, private internals, wildcard-free dispatch, compile-fail alternate-entry rails, and all-missing-observation tests. `ARCH-ADDENDUM-C1-THREE-GAPS.md:110-147`.
5. C1 currently promises the kernel; C2 owns `CompileTypeInfo`, projection tokens, and the outer retry transaction. `charters/C1.md:379-389`.
6. C1’s present Required Exit still names both `TypeInfoCore` and full C2-reachable coverage. `charters/C1.md:391-404`. Therefore a mere evidence-note exclusion would be unlawful; a registered boundary amendment is required.
7. Abstractions must solve a current demonstrated problem, and a zero-consumer artifact must be identified honestly with its first consumer. `orchestration/delivery.md:7-17`; `orchestration/review.md:45-48`.
8. A lawful DEFER requires a recorded ruling, durable owner, acceptance ID/test, deadline, and ruling reference. `CLAUDE.md:584-589`; `orchestration/review.md:219-221`.
9. C2 directly succeeds C1, and C3 cannot consume project-aware projection until C2 is complete. `program-dag.toml:171-187`; `program.md:229-243`.
10. Registry and ledger acts are trunk-owned; the candidate cannot authorize itself. `orchestration/roles.md:28-54`.

## Current-tree reachability

`TypeInfoCore::attempt(NonFlowOperation)` is not required by any operation actually reachable from C2 on this candidate. The complete current C2-reachable operation universe is empty:

- C2 is `LOCKED`, has no charter digest, candidate, or implementation identity. `program-state.toml:942-965`.
- The future C2 carrier types are described only in C1’s boundary prose. `charters/C1.md:379-383`.
- A complete production search finds `TypeInfoCore` only in two comments: `crates/verter_semantic/src/resolver_core/mod.rs:1` and `observation.rs:53-56`. There is no type definition.
- `NonFlowOperation` has no production definition; the candidate acceptance map records that exact absence. `evidence/C1/ac-map.md:19`.
- The current TypeInfo surfaces are host/session operations—named-symbol resolution, expression evaluation, shallow surfaces, and related `VerterHost` APIs—not a C2 facade. `crates/verter_session/src/typeinfo/mod.rs:9-30`; `resolve_named_symbol.rs:86-132`; `shallow_surface.rs:51-96`.
- The live dispatcher still contains `&dyn ResolverContext`. `project_semantic_dispatch/mod.rs:309-311`. It is therefore not the future immutable C2 attempt gateway.

The current module-resolver path is real and independently complete:

- `AttemptOutcome` is the closed three-arm result and `KernelAttempt<T>` specializes it for kernel output. `attempt_outcome.rs:369-379,416-447`.
- `ModuleResolverCore::resolve_attempt` and `resolve_for_project_attempt` return `KernelAttempt`. `module_resolver_core.rs:117-150`.
- The workspace production driver exhaustively handles `Complete`, `NeedInputs`, and `Terminal`, loads only the requested snapshot inputs, and retries against a fresh immutable attempt view. `crates/verter_workspace/src/resolver.rs:282-385`.
- Both production resolution entries use that driver. `crates/verter_workspace/src/resolver.rs:393-430`.
- `ResolverObservation`’s methods all return `AttemptOutcome`, with `ResolverAttemptView` as the sole production implementation. `observation.rs:65-214`; `resolver_attempt_view.rs:93-113,341-525`.

Thus GAP-3 is a future contract with no current C2 consumer. That does not make it vacuously complete.

## Acceptance split and deferral

Register these exact dispositions:

- `C1-AC-5A-MODULE-RESOLVER` — accepted now. It covers the two production module-resolution attempt entries, their immutable observation snapshots, the workspace retry driver, and C1-AC-9’s path-probe/realpath/package-manifest I/O conversion. The existing observation trait is supporting evidence, not a substitute proof for TypeInfo GAP-3.
- `C2-AC-C1-GAP3-TYPEINFO-GATEWAY-001` — CODEX-DEFER to C2.

The C2 obligation includes:

- The semantic-owned `TypeInfoCore`, closed `NonFlowOperation` and heterogeneous result vocabulary.
- One inherent `TypeInfoCore::attempt(...) -> AttemptOutcome<...>`.
- The minimum non-flow TypeInfo extraction necessary to make that gateway real rather than a session-facing forwarding facade.
- Every external operation exposed by C2’s concrete `CompileTypeInfo`, including any C2 use of `ModuleResolverCore`, routed through that gateway.
- Private implementation fields/helpers and no alternate C2-accessible kernel entry.
- A wildcard-free exhaustive operation match.
- Compile-fail tests for foreign implementation, private internals, and alternate entry access.
- One all-missing-observation test per operation variant, requiring `NeedInputs` or `Terminal` without host/scheduler/blocking access.
- Complete/preloaded equivalence against the canonical engine and mutation tests that remove each route/privacy rail.

No exact variants may be named now. There is no C2 facade or charter from which to derive them, and copying today’s `VerterHost` method inventory would invent the future C2 contract. C2’s charter must enumerate the operation table one-for-one from its closed `CompileTypeInfo` projection surface.

The gate is:

1. Bind this obligation into C2’s eventual charter before that charter is ratified or C2 is dispatched.
2. Implement it as C2’s first semantic slice, before any `CompileTypeInfo` projection path may land or enter review.
3. Close it before C2 acceptance and no later than Revision 11 plan close.

Once the amendment and debt row are registered trunk-side and the candidate inherits them by clean rebase, C1 may resume freeze and review. It is not automatically ready; every other landing-path and performance gate remains binding. `ARCHITECT-RULING-2026-08-26-C1-LANDING-PATH.md:81-87`.

## Performance authority

No performance-authority or continuation bytes may be reused as GAP-3 authority or evidence.

`C1-A6-WALL-REL-001` covers only one relative wall-time conjunct. `ARCHITECT-RULING-2026-08-26-C1-PERFORMANCE-AUTHORITY.md:111-139`. `C2-AC-C1-A6-CONTINUATION-001` concerns cross-snapshot request-local continuation, revalidation, and replay. `ibid.:150-164`.

They have different owners’ tests, semantics, failure modes, and acceptance evidence. Preserve those registered bytes and IDs exactly; append the new obligation separately. The rebase proof already rejects conflation. `evidence/C1/rebase-proof.md:43-47`.

## Operative repository acts

No files were modified. The trunk authority/program owner must:

1. Record this ruling at `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-26-C1-AC5-GAP3-AUTHORITY.md`, including the dispatch metadata, candidate SHA/tree, reachability proof, both acceptance IDs, gates, tests, and receipt.

2. Amend `charters/C1.md` at its ruling list, C1-AC-5 row, C2 boundary, and Required Exit to record the exact split above. Do not edit or weaken the historical three-gaps addendum; state that the new ruling supersedes only GAP-3’s owner/timing for the absent C2-facing gateway.

3. Update `program.md:221-235` and the C1/C2 names in `program-dag.toml:171-181` to reflect the moved gateway responsibility. Preserve all predecessor edges—especially `C2 → C1` and `C3 → C2`.

4. Register a digest-bound document row:

   - ID: `RULING-2026-08-26-C1-AC5-GAP3-AUTHORITY`
   - Kind: `RULING`
   - Path: the ruling path above
   - SHA-256: computed from the exact registered bytes.

   Append that ID to the single existing C1 authorization, preserve all existing document/scope bytes, update the re-ratified C1 charter digest, and add a distinct successor field for `C2-AC-C1-GAP3-TYPEINFO-GATEWAY-001`. Do not overwrite the existing A6 successor obligation or create a second C1 authorization.

5. Update `program-state.toml` trunk-side:

   - Append C1’s `C1-AC-5A-MODULE-RESOLVER` acceptance and registered exclusion reference.
   - Append `CODEX-DEFER C2-AC-C1-GAP3-TYPEINFO-GATEWAY-001` to C2’s notes, preserving the existing A6 continuation text byte-for-byte.
   - Keep C2 `LOCKED` and its charter digest empty until its real charter is authored.
   - Keep C1’s three review fields pending.

6. Update `evidence/C1/ac-map.md` from `PARTIAL — OPEN` to the registered split, and create a fresh authority/rebase proof showing all production, harness, and performance-configuration blobs are unchanged. Update final freeze evidence rather than rewriting historical pre-authority results.

7. Rebase the candidate onto the trunk registration commit, re-stamp the final candidate identity, then run:

```text
node scripts/validate-program-state.mjs \
  --dag docs/arch/refactor/rev11/program-dag.toml \
  --state docs/arch/architecture-lock/ledger/program-state.toml \
  --authority docs/arch/architecture-lock/ledger/authority-registry.toml \
  --mode live
```

8. Resume landing-path Step 6 only after that validator passes and the new authority is inherited exactly.

===VERTER-RECEIPT-BEGIN===
LANE: c1-ac5-architecture-authority
RESULT: PASS
REVIEWED: b82ddb421480eef4718a8a0defaa254b7c946180
FINDINGS: none
===VERTER-RECEIPT-END===
