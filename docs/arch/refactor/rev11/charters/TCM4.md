# TCM4 — Atomic activation and deletion

**Status:** PREPARED — charter structure implementation-ready (5-part shape, numbered exit criteria).
Digest-bound authorization record still required before dispatch; ledger status stays LOCKED until TCM0,
TCM1, TCM2, TCM3 are all ACCEPTED. TCM0's own evidence-completeness gaps are tracked in
`evidence/TCM0/OPEN-GAPS.md` and gate TCM0's acceptance, not this charter's readiness.
**Predecessors:** TCM0, TCM1, TCM2, TCM3.
**Authority:** `rulings/MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md` §"Atomic activation and deletion",
global lock §2; `evidence/TCM0/deletion-closure.md` (including its 2026-08-23 19-item cross-check);
`evidence/TCM0/feature-ownership-ledger.md`; `evidence/TCM0/distributed-lifecycle-contract.md`;
`rulings/MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md`.

## 1. Intent contract

**Actor / problem.** Every prior TCM block builds a dormant, unreachable capability. TCM4 makes it live
and removes the superseded architecture — in the SAME accepted transition, so no production state ever
runs both paths.

**Required observable outcomes.**
- Configured projects use their `tsconfig`-declared `contentMappers`; inferred projects use the resolution
  order (workspace-installed → extension-bundled).
- Every TypeScript-backed capability publishes only after the full attestation chain (engine, project/
  config, mapper load, mapper config identity, content-mapped source presence, semantic capability
  attachment, generation agreement) succeeds.
- Every item in `deletion-closure.md`'s 19-item cross-check reaches its recorded disposition: items 1-16
  each land as deleted (with its located mechanism gone) or survives (with its proven owner live); items
  17-18 (which could not be enumerated before TCM1-TCM3 existed) reach whichever disposition TCM0's own
  acceptance record ratifies for `evidence/TCM0/OPEN-GAPS.md`'s `G-DELETION-CLOSURE-ITEMS-17-18` row — an
  enumerated closure TCM4 executes exactly, mirroring items 1-16, or a per-type execution-time discovery
  method TCM0 explicitly authorises. TCM4 does not invent that resolution itself.

**Forbidden observable outcomes.**
- Any intermediate production state where both the new and old paths operate.
- `semver >= 7.1.0` alone treated as activation authority.
- Silent `tsconfig` mutation.
- A performance gate waived because the API is new.

**Authority / fallback order.** `candidate engine → certified engine → active project`, per steering §2's
exact three-tier contract: `candidate engine = stable OR recognized-preview TypeScript with base version
>= 7.1.0`; `certified engine = candidate engine AND accepted executable/package identity AND passing
mapper-conformance probe AND passing semantic-capability probe AND satisfied trust requirements`;
`active project = certified engine AND valid mapper configuration AND attested mapper activation AND
agreeing project/session identities`. There is no legacy route for an engine that fails certification —
TypeScript-backed features become explicitly unavailable, never silently answered through the old
carrier architecture.

## 2. Owned scope

1. **Configured-project activation.** Use `tsconfig`-declared `contentMappers`. Extension registration
   may assist discovery but must not pretend to inject mapper configuration into a configured project.
2. **The full attestation chain** before publishing any TypeScript-backed capability: certified engine
   active; expected project/config active; Verter mapper loaded; mapper configuration identity matches;
   content-mapped source present in the current TypeScript project; required semantic-API capabilities
   attached where needed; project/session generations agree (assembled from the four
   `distributed-lifecycle-contract.md` local owners' identities into the immutable capability descriptor).
   Identity or generation disagreement fails closed.
3. **`NeedsMapperConfiguration` state** when the mapper is absent: do not run the old carrier path; do not
   publish Verter features requiring unattested TypeScript data; retain only independently sound
   framework-native features; issue one actionable, project-scoped recommendation, deduplicated by
   `project configuration identity + certified engine identity + mapper configuration identity`.
4. **Inferred-project resolution order**: (1) trusted compatible workspace-installed mapper, (2) exact
   extension-bundled mapper. Record the selected package, path, manifest, and contract identity.
5. **Trust and external-code compliance**: workspace trust, TypeScript's `--runExternalCode` requirements,
   package execution boundaries, local-pipe permissions, mapper executable resolution, project
   configuration ownership. Never bypass a TypeScript trust refusal through a private Verter channel;
   never automatically enable arbitrary third-party external code.
6. **Documentation** at `https://verterjs.dev/typescript/content-mappers`, covering: certified TypeScript
   versions/builds; mapper installation; configured vs. inferred projects; trusted workspaces; external-
   code requirements; Vue/Svelte/mixed projects; monorepos and project references; mapper options;
   external-source limitations; semantic feature ownership; build/watch/declaration behavior; conflicting
   mappers; diagnostics; troubleshooting; migration from older Verter releases.
7. **The canonical configuration shape** (`{"contentMappers": [{"package":
   "@verter/typescript-content-mapper", "extensions": [...]}]}`), listing only extensions actually owned
   by the project.
8. **A reviewed, safe JSONC edit** for the "add the mapper" action: preserves comments, formatting,
   `extends`, and existing mapper entries; avoids duplicates; refuses overlapping `.vue`/`.svelte`
   ownership by another mapper; shows the exact edit before applying; never mutates `tsconfig` silently.
9. **Deletion, exactly per `deletion-closure.md`'s 19-item cross-check** (2026-08-23 correction) — TCM4
   executes that manifest for the 17 items it names, it does not re-derive the deletion list at execution
   time. Items 17-18 (old DTOs whose only owner was the removed route; historical content-mapper codecs)
   are an OPEN TCM0 gap, not a settled part of this manifest — see `evidence/TCM0/OPEN-GAPS.md`'s
   `G-DELETION-CLOSURE-ITEMS-17-18` row. TCM4 executes whichever resolution TCM0's own acceptance record
   ratifies for those two items (either an enumerated closure it can execute exactly, mirroring items
   1-16, or a per-type execution-time discovery method TCM0 explicitly authorises); TCM4 may not invent
   that resolution itself.
10. **TCM3-EC-G1 gate respected**: TCM4 may delete `feature-ownership-ledger.md` rows #25-26's code only
    after the maintainer ruling TCM3's exit criterion 5 requires is recorded.

## 2a. Timing taxonomy

Every TCM4 timing-sensitive mechanism is classified using `architecture.md` §1.6.

- Activation and deletion in one accepted transition is **owned causal progress** of the cutover. There
  is no dual-path intermediate whose completion is inferred from time.
- The Project-Bound External-TS CRITICAL rule remains in force: production external-TypeScript results
  require a resolved `ProjectBinding` and `BoundProject` witness. TCM4 does not reintroduce an inferred
  backend, a path-only project, or a coalescer that joins work across unbound projects.
- Performance obligations in §6 are **performance measurement**. They are not waived because the API is
  new.
- Creating an unnamed same-key coalescer on the activated path remains FORBIDDEN as a design rule (see
  §5). Proving its ABSENCE by search is not required and is not a close condition: the maintainer ruled
  that closure is disposition of the named inventory and that we do not keep testing for unnamed cells
  (`rulings/MAINTAINER-RULING-COALESCER-CLOSURE-IS-NAMED-DISPOSITION.md`),
  and `charters/K3.md`'s recorded search is evidence of how that inventory was built, not a gate. If an
  independent adversarial search does surface a new cell, classify it in `charters/K3.md` as usual.

## 3. Owned-scope boundary (what TCM4 does NOT own)

- No new feature-ownership decisions — TCM4 activates and deletes per TCM0's ratified ledger; it does not
  reassign a row.
- No new mapping-product design — TCM1's typed `SourceProjectionMap` and TCM2's terminal-view adapter are
  consumed as-is.
- No new oracle-client design — TCM3's `TypeSemanticOracle` is consumed as-is.
- Editor-side `typescriptPluginRefreshScheduler` and Native Preview `transition` promise are TCM4 deletion/activation surfaces (`ProviderHub` is LSP-only and does not own them). `activationGate` is TCM4 extension-activation join; `tsPluginPromise` dies when TCM4 deletes `@verter/typescript-plugin`, and so does that package's own in-plugin sibling of the scheduler — the per-`projectKey` `refreshScheduled` + `pendingScriptInfoReloads` + `pendingResolutionCacheClear` fold into one pending `setImmediate` (`packages/typescript-plugin/src/index.ts:544-546`, folding at `:636-639`), which exit criterion 8's absence check must cover and not only the editor-side scheduler.
- Deletion of a neutral compiler/query facility with a demonstrated surviving owner is explicitly OUT of
  scope (`deletion-closure.md`'s "Survives" table is authoritative; TCM4 does not second-guess it without
  a new finding routed through the program orchestrator).

## 4. Numbered exit criteria

1. **Activation and deletion land in one accepted transition.** Evidence: the landing record shows no
   intermediate commit in the accepted history where both the mapper-backed path and the old relay/
   carrier/plugin path are simultaneously reachable from production routing.
2. **Attestation-chain test suite** (owned-scope item 2): a fixture per missing-attestation-element
   (wrong engine, wrong project, mapper not loaded, config-identity mismatch, source absent from project,
   missing capability, generation disagreement) each proving a fail-closed, non-publishing result.
3. **`NeedsMapperConfiguration` fixture set**: a project with a certified engine but no declared mapper
   produces exactly one deduplicated recommendation, framework-native features keep working, and no
   TypeScript-backed feature silently answers through the old carrier path.
4. **JSONC-edit safety tests**: round-trip fixtures proving comments/formatting/`extends`/existing entries
   survive; a duplicate-mapper fixture is refused; an overlapping-extension-ownership fixture is refused;
   the edit is shown (not silently applied) in a dry-run mode test.
5. **Deletion-closure completion.** Evidence: for each of items 1-16 and 19 in `deletion-closure.md`'s
   cross-check, a citation (file/commit, or an explicit "survives, owner X" citation) proving its recorded
   disposition was executed — a checklist-completion artifact, not a claim. For items 17-18, evidence is
   whichever form TCM0's own acceptance record ratifies for `evidence/TCM0/OPEN-GAPS.md`'s
   `G-DELETION-CLOSURE-ITEMS-17-18` row: if TCM0 enumerates them, the same citation form as items 1-16;
   if TCM0 authorises a per-type execution-time discovery method, a citation proving that authorised
   method ran and recorded its result. TCM4 does not invent the evidence form.
6. **Capability-ledger green-before-delete proof**: for every row TCM4 deletes, the corresponding
   `feature-ownership-ledger.md` row/sub-row's conformance test (named in that ledger) is passing BEFORE
   the deletion commit, proving the new owner already serves the capability.
7. **TCM3-EC-G1 citation.** Evidence: the authority-registry entry recording the rows #25-26 maintainer
   ruling, cited by ID, before any commit deleting `register_carrier_member`/`activate_carrier_member`
   code lands.
8. **No production trace of forbidden names** (§5 below) — a structural/grep-based negative check (this
   program's stated preference for a structural guard, acceptable here as a landing-time check since it
   is verifying ABSENCE of a whole deleted subsystem, not gating an ongoing invariant) confirms zero
   references to the deleted mechanisms' entry points from `crates/`/`packages/` production source.
9. **Performance acceptance**: every HARD REQUIREMENT and reference-point bound
   `performance-baselines.md` currently locks is met under this exact activated configuration (§6 below)
   — measured, not asserted. The full comparative numeric table `performance-baselines.md` does not yet
   populate (tracked as `evidence/TCM0/OPEN-GAPS.md` item G-PERF-NUMBERS) is TCM0's own acceptance gate,
   not silently treated as already-locked here; once populated, every additional locked metric applies to
   this criterion too, without a charter amendment.
10. **Activation/deletion conformance fixtures pass**, per steering's "Required conformance coverage"
    list, scoped to TCM4's activation/deletion responsibilities: missing/malformed/duplicate mapper
    detection, multi-installation monorepos, project references, and mapper/API crash and
    shutdown-cleanup fixtures.
11. **Same-key coalescer inventory holds on the activated tree.** Evidence: every row in
    `charters/K3.md` that names a cell on the activated path is dispositioned — absent, or converged with
    its recorded owner — and no second generic `FlightCell` was introduced. This is NOT a re-run of that
    charter's search: absence of unnamed cells is neither claimed nor required
    (`rulings/MAINTAINER-RULING-COALESCER-CLOSURE-IS-NAMED-DISPOSITION.md`). Mapper
    JSON-RPC admission remains TCM2; oracle snapshot flights remain G2 consumed by TCM3.

## 5. Forbidden

- Any intermediate production state running both the new and old paths.
- `semver >= 7.1.0` as sole activation authority.
- Silent `tsconfig` mutation, or an edit applied without being shown first.
- Deleting the old query plane before its capability ledger is green.
- Retaining the old query plane after its capability ledger is green.
- Deleting `feature-ownership-ledger.md` rows #25-26's code without the TCM3-EC-G1 ruling on file.
- Deleting a neutral facility with a demonstrated surviving owner (`deletion-closure.md`'s "Survives"
  table).
- Waiving a performance gate because the API is new.
- An unnamed same-key coalescer on the activated path; a second generic `FlightCell`; inferred-backend
  fallback that the Project-Bound External-TS rule forbids.
- Bypassing a TypeScript trust refusal through a private Verter channel; automatically enabling arbitrary
  third-party external code.

## 6. Material bounds

Per `performance-baselines.md`, all thresholds locked before TCM4's own implementation results exist:

1. **No measurable regression in direct no-projection compilation** (inherited from TCM1's own bound,
   re-verified under the fully activated configuration).
2. **Zero projection-product allocations in the no-projection path** — re-verified end-to-end, not only at
   TCM1's unit-test boundary.
3. **No compiler invocation caused only by terminal feature policy or position encoding.**
4. **No duplicate TypeScript compilation** — TypeScript's own project graph and Verter's compiler each run
   exactly once per source change, never a redundant second pass.
5. **No unbounded state** — project-handle, snapshot, singleflight, and derived-cache state are bounded
   and released, re-verified under sustained editor-session fixtures (the "100 open/close cycles" bound
   TCM2 already proves, re-run at TCM4's activation boundary).
6. **No hidden second project graph in editor-attached operation** unless TCM0 explicitly proved it
   necessary and superior (it did not).
7. **Removed relay/plugin/carrier work is ABSENT, not merely bypassed after initialization** — a
   still-constructed-but-unused relay object would fail this bound even if never invoked; exit criterion 8
   is the structural proof.
8. **No weakening of existing performance or correctness gates** — the debounced background-diagnostics
   300ms window and every other pre-existing threshold this program did not explicitly widen stay
   unchanged.
9. A performance miss BLOCKS acceptance — it is never waived because the upstream API is new (steering's
   own explicit statement, restated here as a binding bound, not a suggestion).
10. Package certification is settled (`rulings/MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md`);
    TCM4 activates against `typescript@7.1.0-dev.20260822.1` or a later package certified against the
    same locked contract (steering global lock §1's "one current codec... multiple TypeScript builds may
    be certified only when they pass the exact same locked contract and conformance suite").

## Abort / rescope

Per steering global abort conditions, applied to TCM4: old and new routes coexist; deletion is deferred
to another release; a required correctness/memory/performance gate would need weakening; a required
feature still has no legal owner at activation time (including rows #25-26 if TCM3-EC-G1 has not
resolved); the broad `TypeProvider` abstraction is deleted while a surviving caller still needs it.
