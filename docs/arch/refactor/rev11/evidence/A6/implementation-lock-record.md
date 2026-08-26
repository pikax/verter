# Verter Revision 11 Implementation Lock Record

**Status:** Accepted
**Record ID:** `VERTER-REV11-LOCK-001`
**Entry checkout SHA:** `9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0`
**Implementation baseline SHA:** `fb863297a04c7eb114d53ff65736c00240354504`
**Implementation baseline tree OID:** `a2e01e16d705faecf259152f40d0a3b228b16dbf`
**Record digest:** computed over this file's raw bytes at acceptance and recorded in the ledger's
`implementation_lock_digest` — `shasum -a 256 implementation-lock-record.md`. Not embedded here: a
file containing its own digest is unrecomputable.
**Authority package digest:** see §1.
**Program DAG digest:** `efe8d299f5ee96c6faf864161b387f6f39bf111fbf358d269e20d0138b21c77c`
(`shasum -a 256 docs/arch/refactor/rev11/program-dag.toml`; matches the ledger's
`program_dag_digest`, so the DAG this lock binds is the DAG the validator recomputes).
**Accepted program-state digest:** the ledger's own digest at the transition that accepts this
record. The orchestrator is the ledger's sole writer, so it is recorded there, not here.
**Accepted by/date:** maintainer, 2026-08-12 — after two Foundational review rounds
(conformance/architecture/adversarial, all `BLOCKING_FINDINGS` on the first candidate;
findings fixed and impact-bounded-reattested rather than re-run as a full fourth review
round, per the fix-cycle convention this program has used since A4/A5) plus the AMD-001
timing rescope (ruling R-12) and the D-1 machine-path repair, both applied and
re-verified before this acceptance.

**Immutability.** This record is an immutable evidence artifact addressed by its own digest. It may
be stored outside the implementation commit to avoid self-reference; if it is later committed, that
documentation commit does not replace the recorded implementation baseline unless a new lock is
accepted (`contracts/baseline-lock.md` §5). Any implementation-baseline SHA change requires a new
record, an architecture-affecting diff, refreshed reconciliation rows, rerun command proof, and
review of affected measurements, gates and charters.

**How to read the deferrals.** Several template rows below are recorded thin, with an explicit
resolution point, because nothing before the first unlocked block needs them and inventing detail
for a block far down the graph would freeze guesses as authority. Every such row is in §11 with a
named owner and a named gate. The rows that are **not** deferrable — the baseline identity, the
command evidence, the gate file, and the unlock — are complete.

---

# 1. Repository and authority

**Entry checkout SHA / clean state.** `9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0`, tree
`3cf111cf5665586b7d8fdfd520f01cfee3bf8108`. Captured clean at entry (untracked 0, submodules none,
branch level with `origin/main`). Recorded in the ledger's `entry_checkout_sha` /
`entry_checkout_tree` and in the entry-lock record addressed by `block.A0.entry_lock_digest`.

**Ordered Gate 0 commits since entry checkout.** Twenty-three commits,
`9af553dd..fb863297a`, all on the integration lineage. The ones that carry the ordered Gate 0
lineage the baseline depends on:

| accepted at | what it is |
|---|---|
| `8a11cecf4f141f6a0254787e2fb51bd91b1d926b` | adoption landing: program documents, state validator, gate wiring |
| `fdad3da1375473ccf1375b48b2e13fffaba62d79` | command-truth evidence referenced from the capability matrix |
| `d6eefef76c515949a7b7f760bbdf4596a5eef77c` | flow-return corpus strengthening |
| `c1aef669d9c1505e69faf0e327a9c1a5069c5798` | the fail-closed safety retraction (unsupported flow-return results retracted from warm admission) |
| `1ab403c0107801b080438fab30b887c0c8164ecb` | measurement-only work attribution and the captured baseline |
| `9e053d014ca4f98124f431a8d39e1a688087bf49` | the verbatim dispatch prompts retained as the preceding block's context packet |
| `518dc45f0dc35b716b353f3f79fa30cc929c5d5f` | the ledger row for the preceding acceptance |
| `2ec141798f9a9e265ab81e30152c74b4af451188` | ledger identities corrected after the commit-message rewrite described below |
| `6cf19c54dc01fa95b66fd4a1d762d89e497a6640` | machine-specific paths normalised in the retained dispatch records |
| `fb863297a04c7eb114d53ff65736c00240354504` | the amendment's §1 timing rescope — **this baseline** |

The remaining thirteen are ledger, amendment, and evidence commits interleaved between them; the
full ordered list is `git log --oneline 9af553dd262f82ac2f66e4ebf0a0faa70bc7aec0..fb863297a`.

**The integration lineage was rewritten below this baseline, and the rewrite is disclosed rather
than absorbed silently.** An earlier draft of this record bound the baseline
`6af543c8a65b495aad2d6231e5e90878c3bf1769` / tree `e8a6dcd1b6ce2dddf5e965f14138d2ad51f82943`. That
commit is **not an ancestor of the current baseline**: the six commits above the shared ancestor
`839645e3ea7d2b7588e0c9461435b73364ea9b1f` were rewritten to strip program vocabulary from their
messages, per this repository's commit convention, and three further commits then landed on the
rewritten lineage. The rewrite is **message-only, and that is verified rather than asserted** —
each superseded commit and its replacement have an identical tree OID **and** an identical
`git patch-id --stable`:

| superseded | replacement | tree OID (identical) |
|---|---|---|
| `147258e0be47b65fb872236599655d06bf4621f5` | `1ab403c0107801b080438fab30b887c0c8164ecb` | `4ccdd9e72e21aa3bbf16615ee59d23339768ef1b` |
| `3debc2f47` | `bbf953640acb13a372373b0cf8090f9befe5d96b` | `a8769b5c97aa19bce6568fc43e8b2307868bf583` |
| `3982daedf` | `10c635239b2de2dc28b45446bb7a0bf6b250c607` | `def5c212db3a2da02e1f7116536b0147cc437cba` |
| `049d9c976` | `deac7e9a689a6a14994b0d6a7358fe7399f98a28` | `221313c386eea44989fc4dd26e5d8e12aa38c7a3` |
| `b6b347f9287b86e9b90cb416aa357d7a81dfffaf` | `9e053d014ca4f98124f431a8d39e1a688087bf49` | `d06647dc7aa0991bdff07b4216d81520e27e2dd3` |
| `6af543c8a65b495aad2d6231e5e90878c3bf1769` | `518dc45f0dc35b716b353f3f79fa30cc929c5d5f` | `e8a6dcd1b6ce2dddf5e965f14138d2ad51f82943` |

So the superseded baseline's tree and the tree of its live replacement are the same object, and
**no measured number, counter row, or command proof in this record depends on which of the two
names it**. The three commits that follow the rewrite carry documentation bytes only: the ledger
identity correction, the dispatch-record path normalisation, and the amendment rescope (§9). The
superseded SHAs remain resolvable in this worktree's reflog but are reachable from no ref; every
citation of them in this record has been repointed to its live replacement, and the four Gate 0
SHAs above the shared ancestor were never rewritten.

**Exact implementation baseline SHA and tree / clean state.**
`fb863297a04c7eb114d53ff65736c00240354504`, tree `a2e01e16d705faecf259152f40d0a3b228b16dbf`. The
checkout was clean at this commit: `git status --porcelain` empty, `git submodule status` empty.

One property of this baseline is load-bearing for every gate in §7 and is verified rather than
assumed: **it is source-identical to the tree the baseline measurement was captured on.**

```sh
git diff --stat 1ab403c0107801b080438fab30b887c0c8164ecb fb863297a04c7eb114d53ff65736c00240354504 \
  -- crates packages scripts Cargo.lock Cargo.toml pnpm-lock.yaml .github
```

is empty; the only differences between the two commits are the ledger file and files under
`docs/arch/refactor/rev11/` documentation. So the retained counter dataset describes this exact
source. This was **re-run against the rewritten lineage**, not carried over from the superseded
pair, and is empty there too.

**Evidence refreshed after each affected Gate 0 SHA change.** No Gate 0 source change landed after
the instrumentation commit — the two subsequent accepted blocks are documentation-only, tree-hash
verified. The measurement was nevertheless re-run at this baseline rather than carried over, because
the sampling policy this lock freezes (≥30 samples) is stricter than the one the retained timings
used (7). See [`baseline-measurement.md`](baseline-measurement.md) §2.

**Open PR/branch disposition.**

| population | disposition | authority |
|---|---|---|
| PR #98 (`agent/rsvelte-runtime-engine`, draft) | **abandon** — records the program's relationship only; no GitHub action taken or to be taken | ruling R-5 |
| the 469 unlanded local candidate branches | **abandoned as a class.** Every one was cut from a merge-base at or before `2de3b2d07`, i.e. before the squashes that superseded them; that lineage bound is the test the disposition rests on and it holds without exception. No branch is deleted and no GitHub action is taken. | ruling R-13 (ratified) |
| `port/rust` | dispositioned individually within the class: its `+370,822` is one 2,991,892-line generated artifact absent from `main`; excluding that file it is the population's largest net deletion, and its merge-base already satisfies the lineage bound | ruling R-13 |
| `program/architecture-lock`, the active block branch, and the two `origin/preserved/a2c-*` refs | **preserved.** The preserved pair is failed historical evidence that ruling R-10 requires be kept. | R-10, R-13 |

**Toolchains.**

| | version | source |
|---|---|---|
| rustc | `1.97.1 (8bab26f4f 2026-07-14)` — exact-pinned by `rust-toolchain.toml`, not floating `stable` | `rustc --version` |
| cargo | `1.97.1 (c980f4866 2026-06-30)` | `cargo --version` |
| cargo-nextest | `0.9.130 (f0feb11a1 2026-03-09)` | `cargo nextest --version` |
| node | `v20.20.2` | `node --version` |
| pnpm | `10.22.0` | `pnpm --version` |
| platform / architecture | `Darwin 25.6.0` / `arm64` (Apple M3, 8 logical CPUs, 24 GiB) | `uname -srm`, `sysctl` |
| `Cargo.lock` | sha256 `ee9e936f7a95baab80998aea6bb3dee6e53105b9e989bf7e745baae12c050354` | `shasum -a 256` |
| `pnpm-lock.yaml` | sha256 `3f789a2ade9617b68dc75b2734b36ab331c5aa0518f44e0d04a33dec7cda1cfb` | `shasum -a 256` |

External TypeScript/provider/framework/compiler versions used by affected tests are pinned in the
tree and unchanged from the measured baseline (`BUNDLED_TSGO_VERSION = 7.0.2`, the pinned Svelte
compiler, the pinned official-Vue golden corpus).

**Revision 11 document digests.** The canonical 85-file package digest **does not exist**: package
validation is WAIVED by ruling R-2 (the ZIP was never available), and the tree here is a verbatim
reconstruction from the digest-verified consolidated master, not the canonical package. The binding
artifact is therefore the recomputable aggregate over the landed split tree, computed by the command
`PROVENANCE.md` states:

- aggregate at the A6 candidate tree, 75 files:
  `b4e59668fd3d8bd87068d9c56b293be50fc595987f452d604c09cb6f347ffcfc`
- the same aggregate at the implementation baseline tree `fb863297a…`, 74 files:
  `a061d97534f2b96f96a92eae24569f8500c696a4b82d827efbd1a52deb78a173`

Both were recomputed from the git object store rather than from a working tree, so a dirty checkout
cannot contribute bytes.

**The method check, and why it is now anchored one commit lower.** The published aggregate in
`PROVENANCE.md` is `ff49cddeb8f6577144dcf85cb9d026cba0d14e7164e7b3825c4a59b215c8148b`. That value
**still reproduces exactly** — but at the *pre-rescope* baseline tree
`6af543c8a…`/`518dc45f0…`, not at the current one, and reproducing it there is the control that
proves the command above is `PROVENANCE.md`'s command rather than a plausible-looking variant of it.
It no longer reproduces at the current baseline because the rescope commit `fb863297a…` edited
`amendments/AMD-001-stack-window-validator-prerequisite.md`, which is **inside** the aggregate's
input set. `PROVENANCE.md`'s published value is therefore stale with respect to the tree it
describes, by exactly that one amendment. That is a property of the integration lineage, not of this
candidate, and this record does not edit `PROVENANCE.md` to hide it — see §11 note U-12.

The candidate and baseline aggregates differ by exactly one file: this block adds `charters/B1.md`,
which is inside the input set. Every other artifact this block produces lives under `evidence/`,
which the input set excludes, so the candidate digest does not move as evidence is finalised.

Both values have moved twice, and each move was verified by reproducing the value it superseded
before the replacement was trusted:

| | candidate aggregate (75 files) | baseline aggregate (74 files) |
|---|---|---|
| first recorded | `13a09655231e13a7b16e9cb16ab0d8e53ab332f335332af6f33a3fb87cf2d178` | `ff49cddeb8f6577144dcf85cb9d026cba0d14e7164e7b3825c4a59b215c8148b` |
| after the `B1.md` correction | `703c913a8db7983e4546e55c86caf02a17ae3740fd4626b1a414999bad51140f` | unchanged |
| after the amendment rescope | `b4e59668fd3d8bd87068d9c56b293be50fc595987f452d604c09cb6f347ffcfc` | `a061d97534f2b96f96a92eae24569f8500c696a4b82d827efbd1a52deb78a173` |

The first move was caused by a review round correcting `B1.md` — its `performance-gates.toml` path
pointed inside this tree, where the file deliberately does not live. The second was caused by the
rescope of the amendment (§9), which is the only in-input-set file the new baseline changes; it moves
**both** columns, because the amendment is in the input set at the baseline tree as well as at the
candidate tree. All three superseded values reproduce exactly when the same command is run against
the corresponding superseded tree extracted from the git object store, which is what proves each
replacement is a recomputation rather than a differently-computed number.

`B1.md`'s own digest moved with the first correction, from
`7f9c892dc243e538b48c9ddc723e87b2b2185ad2a7f9faecbb42556f4426d1ef` to
`ac60d191221fc5e5938e0343091c6809648a482960ca7c1a49596e547d3e28e1`, and is unaffected by the
rescope; the ledger records the charter digest at acceptance, so it takes the accepted candidate's
value, not a superseded one. `performance-gates.toml` is deliberately placed at the **repository
root**, outside that input set, so the digest it records stays recomputable instead of
self-referential — the rescope changes the value that file records without changing the file's
membership in the set it measures.

**Designated maintainer and orchestrator.** Maintainer: Carlos Rodrigues (GitHub `pikax`) — ruling
R-1, holding package adoption/supersession, this acceptance, ADR amendments, formal rescopes, gate
recalibrations, irreversible compatibility decisions, and final block acceptance. Orchestrator:
the Claude Opus 5 main session recorded in the ledger's `[orchestration]` table. The orchestrator is
the ledger's sole writer and may not self-accept.

**Evidence root and custody policy.** Three tiers, ratified unchanged:

1. **In-tree, identity-free summary** — `docs/arch/refactor/rev11/evidence/<BLOCK>-summary.md` plus
   a `<BLOCK>/` directory of data artifacts. It records no candidate identity and no review outcome,
   which is what lets it be committed on the candidate branch without invalidating itself when a fix
   round produces a new candidate.
2. **Exact-candidate record** — SHA/tree, the three mandate verdicts, evidence digests and raw
   command proofs, in the ledger tree, addressed by `block.<ID>.evidence_digest`.
3. **The live ledger** — external to any checkout (ruling R-6). The in-tree copy at
   `docs/arch/architecture-lock/ledger/` is a **transport copy**; it has no merge story, one machine
   writes at a time, and it is removed from the repository and from git history at plan close.

**Integration lineage (new field, ratified as P-3).** Accepted blocks land on
**`program/architecture-lock`**, currently at `fb863297a…`, not on `main`. The ledger's
`[repository]` table records `branch = "main"`, `head_sha = 9af553dd…` — the *entry checkout* — and
no existing field distinguishes the two. A resuming agent reading `[repository]` alone would land
onto `main` and silently drop every accepted block. The ledger gains an explicit integration-lineage
field (or `[integration]` table) naming this branch; until the orchestrator adds it, the lineage is
recorded here and in each block's exact-candidate record.

One consequence, recorded before the first landing to `main` rather than discovered at it: the
lineage must **not** be fast-forwarded into `main` while the ledger-import commit `49850029c` is in
its history, because that commit carries the transport copy whose removal obligation includes git
history. Landing the program is therefore a history-rewriting operation or a squash that excludes
the directory.

# 2. Non-vacuous command manifest

Recorded per `contracts/baseline-lock.md` §4: a green command that executed zero intended work is a
failure. Raw logs are preserved at the paths named in the last column and digested in
[`command-proofs.md`](command-proofs.md), which also carries the per-command exit codes, executed and
skipped counts, and the sentinel that discriminates each selector.

**The governing constraint.** `.github/workflows/ci.yml` triggers on `push: branches: main` and on
`pull_request`; ruling R-8 keeps all program work local — nothing pushed, no PR, landing by local
fast-forward. **No GitHub Actions job executes for any block of this program.** The canonical local
gate plus the recorded command proofs are the whole automated surface, and `CLAUDE.md`'s
*Verification Must Prove Execution* rule applies with full force precisely because no independent
runner exists.

| # | Exact command/features/target | Executed | Skipped | Exit | Raw log digest |
|---|---|---:|---:|---:|---|
| 01 | `cargo fmt --all --check` | whole workspace | — | **0** | `c45b11f437393f29` |
| 02 | `pnpm build:ts` | all TS packages | — | **0** | — |
| 03 | `node scripts/gate.mjs` | 24,156 + 3 suites + 8,533 across three surfaces | 581 / 563 | **1** | `05c9ffc890377940` |
| 04 | `cargo clippy --workspace --all-targets -- -D warnings` | all targets, host | — | **0** | `142532f24744724e` |
| 05 | `cargo check --workspace --release` | real release profile | — | **0** | `10e70690aaad8991` |
| 06 | `cargo clippy --target wasm32-unknown-unknown -p verter_wasm -- -D warnings` | the wasm32 artifact | — | **0** | `9efe1a76f2a4067d` |
| 07 | `cargo check --workspace --all-targets --features verter_audit/attribution` | all targets, enabled arm | — | **0** | `8295e3a2625b6eba` |
| 08 | `cargo test -p verter_audit --features attribution` | 86 | 3 ignored | **0** | `4ee9010dae74a641` |
| 09 | `cargo test -p verter_audit --features compile-fail` | 83, incl. 37 trybuild | 1 ignored | **0** | `5879a24ddbae7556` |
| 11 | `pnpm run test:scripts` | 78 across five suites | 0 | **0** | `196208a39f1ffa92` |
| 12 | `pnpm install --frozen-lockfile` | lockfile in sync | — | **0** | `426d05bd6d78a919` |
| 13 | `pnpm run build:native` | the release `.node` binding | — | **0** | `ee5beed305d6117a` |
| 14 | `pnpm test` | 555 (552 passed, 3 failed) | — | **1** | `2ef4921ad8b53220` |

Skip/ignore policy: no command filters its own universe. The gate's skips are nextest's own
platform/feature skips reported by the runner; rows 08 and 09 report their `#[ignore]` counts. The
sentinel that discriminates each selector, plus the per-row non-vacuity evidence, is in
[`command-proofs.md`](command-proofs.md) §2.

**Two rows are non-zero and are classified rather than rounded off**, in
[`command-proofs.md`](command-proofs.md) §3 and
[`command-proofs-native.md`](command-proofs-native.md):

- **Row 03 returned FAIL.** Five reported failures, three distinct tests. One —
  `tracked_paths_no_machine_roots` — is a **genuine tracked-tree defect that pre-exists this
  baseline**: the two preceding blocks' context packets embed an absolute machine path, proven by
  `git grep` against the baseline commit, and both blocks skipped the canonical gate on the reasoning
  that they changed no production source. The guard scans tracked bytes, not production source. This
  block's own packet was the third instance and is fixed; re-running the guard alone reports 2
  violations instead of 3. Repairing the other two invalidates two `context_packet_digest` values the
  ledger already records, which is outside an implementer's write set. The other two failures
  (a real-tsserver respawn test, and a trybuild smoke test killed at the 360 s cap) both **pass in
  isolation on an idle machine** — 2/2 and 1/1 — and neither can be caused by a candidate whose diff
  under `crates/` and `packages/` is empty.
- **Row 14 returned 1**, with 552 tests passing and three `@verter/typeinfo` resolution tests
  failing. Same empty-diff argument: the candidate changes zero bytes of either input those tests
  read.

**The canonical gate does not return PASS on this tree, and did not return PASS on the baseline
either.** This record does not claim a green gate. Confirming rows 03 and 14 against a baseline
checkout is an orchestrator action at landing.

**Locked per-block commands (decision A5-G1, ratified).** The instrumentation feature arms are
compiled and run by no automated gate: the disabled arm does not type-check the enabled arm's amount
expressions, and the trybuild seal that proves the counter-reader path is absent — the negative
control for the no-semantic-authority claim — is never executed by the canonical gate. The obvious
remedy is structurally unavailable: a feature arm cannot ride the existing archive variants without
changing feature unification for all three surfaces, a third variant is a third whole-workspace
compile, and a correct addition needs a matching arm in the 7,170-line gate self-test. So these three
become **required per-block commands**, whose captured output is preserved as command proofs:

```sh
cargo check --workspace --all-targets --features verter_audit/attribution
cargo test -p verter_audit --features attribution
cargo test -p verter_audit --features compile-fail
```

Stated honestly, and locked here so it does not silently lapse: **this is weaker than a gate.** It
depends on the orchestrator running the set and the reviewer checking the proof. That weakness is
inherent to a program in which CI cannot run at all. The CI job is proposed for **after** the
program and requires a ruling extending R-7; no `.github/` change is made now.

# 3. Capability and maturity matrix

`contracts/capability-matrix.md` carries eight seed rows, and **every `Status` cell is `VERIFY`**.
This lock **ratifies no maturity, default, or compatibility promise**, and says so rather than
manufacturing one: ratifying a Supported/Stable cell requires product/conformance review and
oracle evidence that no block through this one produced.

| Framework/surface | Operation/product | Complete/Partial/Unsupported | Profile | Oracle/evidence | Public contract |
|---|---|---|---|---|---|
| Vue | runtime compile | `VERIFY` | `VERIFY` | official Vue fixtures + Verter corpus; execution evidence via the canonical gate and the committed golden corpus | `VERIFY` |
| Vue | IDE companion | `VERIFY` | provider-specific | provider + mapping corpus; the editor-neutral provider-matrix lane | `VERIFY` |
| Vue | imported macro runtime projection | `VERIFY` | supported normalized profiles | official/compiler-sfc differential oracle | `VERIFY` |
| Svelte | native runtime compile | Experimental (pin verified as a tree fact) | syntax/toolchain profile | pinned Svelte compiler corpus | experimental |
| TypeInfo | `TypeAtPosition` | `VERIFY` | normalized TS profiles | selected TS oracle; native suites in the canonical gate | `VERIFY` |
| TypeInfo | graph export | advanced explicit; off unless requested | profile stamped | protocol/round-trip corpus; the byte-pin freshness receipt | named compatibility domain |
| LSP | external TypeScript provider | `VERIFY` | provider profile | capability matrix; the provider-matrix lane | provider epoch/profile stamped |
| CSS | parse/format/index/transform | `VERIFY` | dialect profile | dialect/framework corpus; the CSS-syntax package receipt | `VERIFY` |

The non-`VERIFY` cells above are the plan's own seed values, carried unaltered from the accepted
base; they are not ratifications made here.

**Consequence, and it is deliberately fail-closed.** `contracts/capability-matrix.md` §3: "A
missing/`VERIFY` row means the capability is not approved for architecture claims or default
changes." `program.md` requires the atomic flow-cutover block to satisfy every effective-flow
capability row declared Supported/Stable **in the A6 matrix**; with no row so declared, that
obligation is currently satisfied vacuously. That is a real gap, not a pass: **the matrix must be
ratified before that block begins.** Recorded as §11 row U-1 with its owner and gate. Nothing before
the first unlocked block depends on it, which is why it is deferred rather than guessed.

# 4. Identity and profile lock

**Canonical digest schema / domain epoch.** The tree carries no single canonical digest schema; it
carries seven real compatibility domains and a separate family of disposable-cache invalidation
namespaces, enumerated in §5. The Revision 11 canonical-digest schema is created by the first
unlocked block, not lifted from an existing owner.

**Source/unit/syntax/parse/placement identities.** Current owners are enumerated per row in
[`../A5/owner-rows.md`](../A5/owner-rows.md) — all sixteen seed `VERIFY` rows source-verified, plus
two the seed table omitted. Two corrections in that inventory bind design and are restated because
missing either changes the work rather than the schedule:

- **`ProviderHub` does not exist.** It is a Revision 11 *target* name that the seed reconciliation
  table listed under *current* authorities; `grep -rn "ProviderHub" crates/*/src` returns nothing.
  The real current owners are `SyncCoordinatorHandle` (`verter_lsp`), the `TypeProvider` trait
  (`verter_type_runtime`), and the `EngineBackend` / `BoundProject` / `ProjectBinding` triple
  (`verter_session::external_ts`).
- **`flow_slice_content.rs` is not a second flow/control semantics path.** It is the content half of
  the one flow substrate. A charter ratified against "delete the second flow engine" would have sent
  an implementor to delete something that is not there.

**Stable ID collision/equality policy.** Not yet lockable as a policy — the distinct identity types
it would govern (`StableEntityId`, `SessionHandle`, `QueryIdentity`, `SemanticFlightKey`,
`InputBasisId`) are created by the first unlocked block. What **is** locked now is the constraint
that block must satisfy, from its charter: `StableEntityId` and `SessionHandle` non-interchangeable;
`QueryIdentity` distinct from `SemanticFlightKey` and `InputBasisId`; no global revision, request,
deadline or budget smuggled into reusable identity. §11 row U-2.

**TypeScript option-classification table locations.**
[`../A5/option-classification.tsv`](../A5/option-classification.tsv) and its
[`.md`](../A5/option-classification.md) — 84 configuration fields across 5 owner structs, one class
each per `contracts/semantic-profile.md` §1.

**Output/presentation/serialization/execution profile schemas.** Created by the first unlocked block;
the classification input is the 84-field table above. §11 row U-2.

**Unknown option behavior.** Locked: an unrecognised configuration field is a typed rejection at the
owner boundary, never a silent default. This is the one identity-lock row stated as a rule rather
than deferred, because a block that lands profile schemas with permissive unknown-field handling
cannot be corrected later without a compatibility break.

**One live finding that constrains identity work.** Two of the five cache-identity dimensions have
**no production input**: `EnvHashInputs` is constructed at exactly three non-test sites, all in
`crates/verter_workspace/src/engine.rs`, and all three hardcode `type_strict: false`,
`type_no_implicit_any: false`, `lib_names: &[]`, `type_roots: &[]`, so `type_env_hash` and
`lib_env_hash` are constant across every project today. The strict-family semantics exist
(`StrictFamilyConfig`) but are driven by a `pub(crate)` test-injection atomic with no production
writer. This is a missing ingress, not a live collision — nothing varies, so nothing collides — but
whichever block first threads real tsconfig values in **changes cache identity for every existing
project at that moment**. The profile schema must be able to carry real values; threading them in is
not the first block's work, and the blast radius is owned separately. §11 row U-3.

# 5. Compatibility and protocol lock

| Domain | Owner | Current namespace/epoch | Consumers | Persistence/public | Evolve or replace | Migration/rejection |
|---|---|---|---|---|---|---|
| TypeInfo graph wire | `verter_protocol/src/typeinfo/graph.rs` — `TYPEINFO_GRAPH_SCHEMA_VERSION` | 7 | off-tree protobuf clients; TS bindings | public wire | evolve (monotonic epoch) | closed-set schema-version gate; typed request error |
| component-meta payload | `verter_protocol/src/component_meta.rs` — `COMPONENT_META_SCHEMA_VERSION` | 10 | `@verter/component-meta` and its compat layer | published payload | evolve | typed rejection on unknown epoch |
| tsgo control protocol | `verter_tsgo_api/src/control/messages.rs` — `PROTOCOL_VERSION` | 2 | the out-of-process tsgo channel | process boundary | evolve | handshake rejection |
| tsgo advertisement | `verter_tsgo_api/src/control/advertisement.rs` — `ADVERTISEMENT_VERSION` | 1 | capability handshake | process boundary | evolve | handshake rejection |
| editor/tsserver attestation | `verter_lsp/src/editor_tsserver.rs` — `EDITOR_TSSERVER_ATTESTATION_VERSION` | 1 | the editor-neutral attestation record | recorded artifact | evolve | rejected attestation |
| Svelte conformance manifest | `verter_svelte_conformance/src/manifest.rs` — `SCHEMA_VERSION` | 4 | the committed on-disk manifest | persisted | evolve | manifest rejection |
| analysis-projects config | `verter_analysis_inputs/src/config.rs` — `ANALYSIS_PROJECTS_SCHEMA` | `"verter.analysis-projects.v1"` | user-authored config | user-facing file | **replace** (namespace form) | a new namespace is a new domain |

The last row is the only value in the tree following the *namespace* form rather than the epoch
form. It is consistent with the compatibility-domain ADR; it is recorded explicitly because a later
block converging "all versions to integers" would regress it.

**Not compatibility domains.** The disposable-cache invalidation namespaces
(`CACHE_CLUSTER_SCHEMA_VERSION`, `CURRENT_PARSER_VERSION`, `LEGACY_PARSER_VERSION`,
`ROUTE_DB_RESOLVER_VERSION`, `RESOLVED_IMPORT_FACTS_RESOLVER_VERSION`, the carrier parser/schema
versions, `RUNE_AMBIENT_PRELUDE_VERSION`, the Svelte script-fact capture version), package semver,
and external tool versions are correctly *not* epochs. Full table in
[`../A5/compatibility-domains.md`](../A5/compatibility-domains.md) §2–§3.

One legibility defect is recorded rather than fixed here: `CURRENT_PARSER_VERSION = 5` and
`LEGACY_PARSER_VERSION = 6` are genuinely independent sequences whose names imply one ordering and
whose values invert it. No behavioural change; rename when a block next touches the artifact key.

**Semantic graph protocol disposition.** The typeinfo wire surface stays a closed contract under its
four invariants (closed-enum discipline, field numbers never reused, purely additive audit-envelope
additions, validation before semantic execution). The provisional framework-surface payload retag
and its schema-version bump remain owed by the block that owns them; this lock does not retag it.

**`TypeExpr` consumer inventory disposition.** Enumerated in
[`../A5/consumer-protocol-map.md`](../A5/consumer-protocol-map.md) §1: sixteen workspace crates
declare the dependency, and the distribution is not flat — `verter_session` holds 65% of all
references, so the eventual elimination's blast radius is one crate, not sixteen. The FFI/binding
tail (`ffi` + `napi` + `wasm` + `lsp` = 148 refs) is the public half. `verter_no_typeexpr` and
`verter_no_storedspan` are **not** consumers to migrate: they are marker crates whose purpose is to
prove absence structurally, and they are the instrument the program should extend rather than
replace. Lifetime modelling and migration order belong to the consumer-map block, not to this lock.

**Parser/artifact/cache/schema duplicate authority disposition.** No duplicate authority is
ratified. One candidate duplicate is recorded as **NOT PROVEN** rather than resolved:
`provider_protocol_version = 12` (`verter_protocol/src/consumer_compatibility_manifest.rs:75`,
consumed at `:109`) has a located producer, but whether that hand-pinned literal duplicates a
compatibility domain owned elsewhere — the forbidden "duplicate counter that must stay equal" — is
unproven, as is why it is hand-maintained when `component_meta_schema_version`, three lines away in
the same function, is sourced from its owner. §11 row U-4.

# 6. Dependency and owner baseline

**Crate/module dependency graph artifact.** Derived from
`cargo metadata --format-version 1 --all-features`, walking `resolve.nodes[].deps` and skipping deps
whose `dep_kinds` are all `dev`. The seven-layer assignment and the closure results are in
[`../A5/dependency-direction.md`](../A5/dependency-direction.md) §3.

**Forbidden edges / tests.** The strategy is **locked**: one `cargo metadata`-driven forbidden-edge
test over the whole workspace graph, modelled on the existing closure guard. Closure-based, not
direct-edge-based (a direct-edge test passes while a two-hop violation walks straight through it).
Exceptions equality-pinned, never subset-checked, each with a recorded rationale and a named removal
gate. Discrimination proven by planting a forbidden edge and proving the plant applied. **No new
source-text scanner** — landed enforcement is compiler/type-system/tool-based, and three of the four
current mechanisms are exactly the grandfathered scanner form the rule forbids extending.

Superseded in the same accepted candidate: `verter_audit_no_upward_deps` and both tests in
`crates/verter_scheduler/tests/cases/no_session_dep.rs`. `audit_substrate_isolation` is **not** fully
implied — its dependency half is, its *naming* half is not — and must be decided explicitly rather
than deleted on the assumption that a closure walk is a superset of it.

**The one recorded exception (A5-DD1, ratified).**

```text
DEBT A5-DD1  Disposition: DEFER
  Finding:          verter_semantic (and verter_diagnostics) depend on verter_workspace, so the
                    semantic kernel's production closure reaches verter_scheduler (unconditional,
                    every target) and verter_tsgo_api (native only — declared under
                    cfg(not(target_arch = "wasm32")) in verter_workspace/Cargo.toml), contradicting
                    the binding-dependency-direction ADR's reusability consequence.
  Durable owner:    the module-resolver/type-info convergence block, with the first unlocked block
                    owning the test that makes the violation FAIL rather than be reviewed for.
  Resolution gate:  that block's accepted candidate. The first unlocked block MAY land its test with
                    this pair as a recorded, equality-pinned exception; it MUST NOT land it as a
                    subset-checked allowance, and the exception must name the removal gate.
  Acceptance:       verter_semantic's closure contains neither verter_scheduler nor verter_tsgo_api
                    on ANY target; the closure test fails if either returns. The exception must
                    record the target condition alongside the edge, so a wasm32-only resolve cannot
                    read as satisfied.
  Ruling reference: ratified with this lock.
```

Not repairable by re-layering: `verter_workspace` cannot sit below `verter_semantic` because of its
own upward dependencies, and a wasm32-only firewall is not a firewall.

For contrast, the two crates that do satisfy a real firewall: `verter_audit`'s production closure is
exactly `{verter_audit, verter_span}`, and `verter_macro_dto`'s is itself plus the four marker crates
and `verter_span`.

**What the strategy deliberately does not cover**, recorded so a reviewer does not read it as
coverage: intra-crate direction (the walk sees crates; `verter_session` spans five of the six layers
by volume, and its internal direction is held by the type system and by review), and trait-object
back-edges (a callback that inverts control without inverting the dependency is a review mandate,
not a graph query).

**Owner/service/cache/lock/queue/path concept inventory.** [`../A5/owner-rows.md`](../A5/owner-rows.md).

**Current IDE/build parser front-ends, and current direct/managed/FFI routes.** Recorded thin: the
enumerations exist in the owner-rows inventory, and the blocks that consume them as a *closure*
(shared syntax front-ends, the direct compiler, the managed runtime) are not unlocked by this lock.
§11 row U-5.

# 7. Work, performance, and memory lock

**Counter schema/version.** The closed `WorkSite` enum declared in one macro invocation in
`crates/verter_audit/src/attribution/schema.rs`: 71 site ids of the form `<owner>.<chokepoint>`, each
carrying a work domain and a unit. Sites cannot be minted ad hoc at a call site the way a
string-keyed counter can, and the inventory is enumerable at compile time. Everything that can
produce a number is behind the non-default `attribution` feature, so a production build cannot branch
on a counter: the path does not resolve. That is proven from outside the crate by a trybuild fixture
that names the whole reader surface and must fail to compile.

**Surviving instrumentation ownership (decision A5-L1, ratified).** That schema is the single
surviving work-attribution authority, which is a statement about **two** owners, not one. The tree
also carries `crates/verter_session/src/loop5_instrumentation.rs` — 1,121 lines, unconditionally
compiled, no feature gate, 46 atomics, of which the predecessor's census found 24 never incremented
anywhere yet still reset, loaded and emitted into the JSON dump as `0`, which reads as "this work did
not happen" rather than "this counter was never wired". 18 more are live and overlap the attribution
sites at the same chokepoints (`component_meta_materialize.rs` carries a `loop5` `TimerGuard` four
lines from `attribute_scope!(MaterializeStructure)`), and 4 belong to a backtrace watchdog, which is
a debugging facility rather than work attribution and is correctly not folded into the attribution
substrate. It also costs what the disabled arm does not: `TimerGuard::new` calls `Instant::now()`
unconditionally and `watchdog_beat()` does a relaxed atomic load at 20+ hot call sites.

The predecessor recorded the disposition with its ruling reference `PENDING`. It is ratified here,
and the owners are named rather than left to the block that trips over the duplication:

```text
DEBT A5-L1  Disposition: DEFER — Converge then Delete
  Finding:          two work-attribution owners; 24 never-incremented loop5 counters emitted as
                    zeros, 18 live counters overlapping the attribution sites at the same
                    chokepoints, and a backtrace watchdog parked in the same module.
  Durable owner:    the counter half (dead and live) migrates under block G4 — the cache/store
                    convergence that rewrites SemanticGraphStore, the surface these counters
                    instrument, so the deletion sits inside its cutover closure. The watchdog
                    relocates under block K3, which exists to reduce exactly the catch-all session
                    ownership that parks a debug facility in that crate.
  Resolution gate:  each owning block's accepted candidate; hard backstop block L4 — the final
                    architecture lock. NO part of this debt may survive L4.
  Acceptance:       loop5_instrumentation.rs absent; the two in-crate tests that assert on live
                    counters MIGRATED rather than dropped, and still discriminating (they must fail
                    against the pre-change tree); the watchdog reachable from its new owner with its
                    call sites intact.
  Ruling reference: ratified with this lock.
```

Two consequences this lock states rather than leaves implicit. **This block performs none of that
work** — no counter is migrated and `loop5_instrumentation.rs` is not touched here; assigning the
work is the ratification, doing it is G4's and K3's. And the module's program archaeology in
production source (`//! Loop 5 …`, "the loop-5 brief", "orchestrator memory", `_loop8_timer` locals)
escapes `no_phase_archaeology_in_production_code` only because that guard's trigger list has no
`loop` root, and **no trigger is added**: doing so would grow a grandfathered name-keyed scanner the
landed-guard rule forbids extending, and would false-fire on every ordinary use of the word. The
durable fix is the deletion above, not a wider scanner.

**Baseline raw result location.**

- [`baseline-counters.tsv`](baseline-counters.tsv) — the counter dataset at this baseline, 44 data
  rows (45 lines including the header).
- [`../A4/baseline-40-components.tsv`](../A4/baseline-40-components.tsv) — the retained dataset from
  the source-identical measured tree, used as an independent reproduction check.
- [`baseline-measurement.md`](baseline-measurement.md) — timings, peak RSS, the measured noise floor,
  and the derivation of every locked limit.
- [`counter-reproduction.md`](counter-reproduction.md) — the row-by-row reproduction, including the
  two counters that moved and are therefore **not** gated.

**Accepted `performance-gates.toml` digest.** File at the repository root; digest recorded in the
ledger's `performance_gates_digest` at acceptance (`shasum -a 256 performance-gates.toml`). It
contains no placeholder or `REQUIRED_*` value, and that is machine-checked rather than asserted:

```sh
node scripts/validate-performance-gates.mjs --gates performance-gates.toml
node --test scripts/validate-performance-gates.test.mjs
```

The validator is a new ratified Node implementation, not a port — the plan's Python original was
never available and the maintainer ruled the validators are reimplemented in Node. Its suite carries
twenty negative controls, each a single attributable mutation of a complete locked shape, each
asserted to be unique in the source before it is applied so that a mutation which fails to apply
cannot report a pass.

**Benchmark runner/machine class.** `apple-silicon-laptop-8core-24gib` — Apple M3, 8 logical CPUs,
24 GiB, Darwin 25.6.0 arm64, AC power with low-power mode off. Absolute wall and RSS limits are bound
to this class; a different class is a recalibration, not a local adjustment.

**Competitor versions/builds/corpus equivalence.** **None locked.** No competitor cell exists in this
gate file. The competitor comparison belongs to the direct-compiler benchmark families, whose owning
blocks are not unlocked here; locking a competitor ratio now would freeze a threshold for work whose
shape is unknown. Adding those cells is an extension requiring a new lock record digest and the same
independent review class — never a relaxation of what is locked now.

**Conformance-oracle corpus pins (added by forward amendment).** FORWARD-STATED: this records what is
pinned as of this amendment, and asserts nothing about what was locked before it. These pins were not
previously in this record; that absence was real and is not being rewritten.

| oracle | pinned version | authority in the tree | what compares against it |
|---|---|---|---|
| Svelte conformance corpus | `5.56.10` | root `package.json`, `pnpm-lock.yaml`, `scripts/svelte-golden-lib.mjs` (`SVELTE_ORACLE_VERSION`), and each golden's own `oracleVersion` field | the `verter_svelte_conformance` golden corpus suite and the `verter_css_syntax` Svelte compatibility profile |
| Vue CSS-transform oracle | `@vue/compiler-sfc@3.6.0-rc.5` | root `package.json` | the Vue-owned CSS transform acceptance evidence |

Changing either version is a §4.1 recalibration — written cause, retained old/new calibration data, an
independent reviewer, amendment of this record, and rerun of affected evidence — never a local edit.

**Explicitly NOT locked by this amendment:** `SVELTE_CASE_ID_SALT` (`svelte-5.56.8`). It is a frozen
identity-namespace constant, not a version tracker; its own record requires that it never be updated to
follow the live oracle, and moving it would change existing conformance case identifiers.

**Owner memory budgets and allocator slack.** Not applicable at this lock: the Revision 11 owner
memory model does not exist, so there is no per-owner budget to state. Whole-process memory **is**
gated, by the peak-RSS metrics in the locked cell. §11 row U-6.

**Soak workloads/durations/seeds.** None locked. The single locked cell is a ~70 ms batch; soak,
quiescence and slope gates belong to the managed-runtime and lifecycle blocks. §11 row U-6.

**What the locked cell actually gates, and why it is the right cell for the work now unlocked.** One
cold project batch — 41 files, fresh host, upsert, load, component metadata per component, host-backed
batch compile — with fifteen conjunctive metrics: four timing and memory gates (an absolute and a
no-regression bound each on wall clock and peak RSS) measured on the disabled instrumentation arm,
eleven deterministic work counters measured on the enabled arm, three zero-work
CSS assertions, and an exact output oracle on the component-meta digest. The block now unlocked lands
typed identities and canonical encodings; this cell measures canonicalization/hash/key-construction
overhead *in situ*, because path normalisation runs 11,313 times in one pass and the cold-build
counter moves first when a new key type is not equal where the old one was.

Two disciplines are recorded because they are what makes the numbers trustworthy rather than
convenient:

- **The relative bounds are derived from measured noise, not chosen.** `max(3%, 2 × noise)` gives
  3.000% for wall clock (measured noise 1.4757%) and 4.952% for peak RSS (measured noise 2.4760%).
  The RSS bound is not rounded up to 5.0, because the rule is an upper bound.
- **The absolute limits are product budgets, not fits.** 100 ms for the batch comes from a 2.5 ms
  per-component cold budget; 256 MiB comes from a whole-project RSS ceiling. Neither is derived from
  the 70.5 ms and 74.9 MB actually measured, and the RSS absolute is explicitly recorded as a
  catastrophe stop whose tight fence is the relative gate.

**Gate immutability.** Thresholds cannot be relaxed after candidate direction is observed. A
recalibration requires a materially changed machine/toolchain/corpus/competitor, retained raw
before/after calibration, no candidate result inspected first, an independent performance reviewer's
acceptance, and a re-baselined block — through a new lock record digest and the same independent
review class. A benchmark defect requires baseline **and** candidate reruns. A candidate cannot choose
its pass criterion after measurement.

# 8. Semantic safety state

**Harness location/result.** The flow-return corpus and its comparator controls live in
`verter_session`'s flow-shape corpus suites, executed by the canonical gate. The strengthening
landing and the retraction landing are the two Gate 0 commits named in §1.

**Wrong-complete rows retracted.** The safety block retracted unsupported flow-return results from
warm admission through the existing typed degradation and non-admission rails. It changed behavior
only to retract a wrong-complete result; it had no structural-completion obligation.

**Typed gap/admission behavior.** A retracted result surfaces as a typed gap and is refused warm
admission rather than published. No syntax-only completion detector, second graph, second
classifier, or false refusal of a checker-correct clean/warm result was created, and none may be.

**Remaining unsupported rows.** Exact structural completion and the associated discrimination remain
open debt, owned by the completion-graph block, with the sole demanded function flow graph as the
completion authority and no syntax-only fallback. This lock does not close that debt and does not
license a second classifier to work around it. §11 row U-7.

# 9. Orchestration, worktree, CI, review, and stack lock

**Designated maintainer and acceptance channel.** Carlos Rodrigues (`pikax`); acceptance is recorded
as a maintainer decision in the ledger by the orchestrator. No agent may self-accept.

**Orchestrator identity/harness and permissions.** The main session recorded in the ledger's
`[orchestration]` table, running locally. Permissions are local-only by construction: nothing is
pushed to `origin`, no pull request is opened, no GitHub action of any kind is taken.

**Maximum active worker contexts.** Three, as the ledger records. Note the recorded tension: the
program-state validator enforces a strict single-`IN_PROGRESS` reading, which already conflicts with
three active workers. Fail-closed by choice; a parallel regime must relax the check under review
alongside the stack model, not ad hoc.

**Context-packet storage/digest policy.** Each block's packet is committed under
`docs/arch/refactor/rev11/evidence/<BLOCK>/context-packet.md` and digested into the ledger's
`context_packet_digest`. Where a packet was originally an ephemeral dispatch prompt, the verbatim
prompt is the artifact of record; a reconstruction is labelled as one.

**Program-state path, writer, validation command, and transition policy.** External ledger is
authoritative; the in-tree transport copy is at
`docs/arch/architecture-lock/ledger/program-state.toml`. **The orchestrator is the sole writer — a
block implementer never writes a ledger row**, because an implementer that records its own accepted
row is self-accepting. Validated after every transition, both copies, with:

```sh
node scripts/validate-program-state.mjs \
  --dag docs/arch/refactor/rev11/program-dag.toml \
  --state docs/arch/architecture-lock/ledger/program-state.toml \
  --mode live
```

A divergence between the two copies is a synchronisation failure to reconcile by hand, not a
validator finding.

**Worktree/branch naming and one-writer lease policy.**

| purpose | convention |
|---|---|
| integration lineage | `program/architecture-lock` (single, fixed) |
| block implementation | `work/<block-id-lowercase>-<slug>` |
| block review checkout | `review/<block-id-lowercase>-<mandate>`, mandate ∈ `conformance` \| `architecture` \| `adversarial` |
| fix round | the same `work/…` branch — a fix produces a new candidate, not a new branch |
| worktree path | a **sibling** directory of the program root, never nested inside it |

One writable worktree per worker, by assignment. A fresh worktree runs `pnpm install
--frozen-lockfile` before any JS/TS test or workspace-importing Node script: `node_modules/` is
gitignored and its absence makes JS/TS tests fail in a way that reads as a regression. The
"sibling, never nested" rule is not cosmetic — nested worktrees under the program root's own ignored
tree are invisible to a `git status` in the parent and easy to leave behind.

**Generated-file/lockfile/protocol central-writer policy.** Generated bindings, lockfiles and
protocol schemas have one writer: the block that owns the producer. A block that regenerates a
binding it does not own is out of scope. Lockfile changes are landed by the block that changes the
dependency, never as drive-by churn.

**GitHub branch protection, required checks, merge queue, signed-commit/rebase facts.** `main` has
**no** branch protection, **no** merge queue and **no** required status checks; one ruleset applies
to the default branch (deletion, non-fast-forward, Copilot review). Commit signing is **not** in use.
Stack tooling (`gt`, `git-town`, `spr`, `ghstack`, `jj`) is **absent**. None of it is reachable
anyway: nothing is pushed.

**Merge constraint.** One block per landing delta, fast-forward, no co-batching with unrelated
changes. Landing is a local fast-forward of the integration lineage onto the accepted candidate.
Because a reviewed candidate SHA and an accepted landing SHA may differ when the base advances, a
diverged accepted identity requires the landing-equivalence artifact, which the program-state
validator already gates.

**Stack implementation.** `LOCAL_BRANCH_CHAIN` — GitHub-native stacks, merge queues and dependent PRs
are all unavailable under the no-push ruling. A "stack" here is a chain of local branches with an
explicit stack-window record.

**Default/max stack window and larger-window approval rule (decision S-1, ratified; this is the
standing policy from this lock onward, including this block's own landing).**

- `max_open_stack_layers = 2` — the minimum of the permitted two-through-six range.
- `stack_mode_policy = "ATOMIC_REVIEW"`.
- `stack_tool = "LOCAL_BRANCH_CHAIN"`.
- **Default operating depth is 1.** A window of 2 is a ceiling, not a target: blocks land
  sequentially on the accepted tip, and a window is opened only for the one mandated
  private-checkpoint → acceptance pair or for a case the maintainer explicitly ratifies. Sequential
  operation needs no stack-window record at all, which is why every block so far has correctly
  carried an empty `stack_id`.
- A wider window requires an explicit maintainer ruling **and** a validator that models it.

Rationale, recorded because a reviewer should be able to attack it: the program requires exactly one
stack and it is depth 2; the stack-window validator prerequisite makes window width a real cost, and
modelling a 2-layer window is bounded where modelling a 4-to-6-layer window is not, for capability
the program has no use for; and narrowing the window shrinks the existing single-`IN_PROGRESS`
validator conflict to the one case that must be modelled anyway. The `LANDABLE` mode stays permitted
but unused.

Full policy, including restack and invalidation rules, in
[`stack-window-policy.toml`](stack-window-policy.toml). It is deliberately a **policy**, not an
instance of the snapshot template: no window is open, and minting a one-layer snapshot would record a
stack that does not exist.

**`AMD-001` — the registered prerequisite this lock is named by and bound by digest, RESCOPED (R-12)
before this candidate's acceptance so it no longer names this block as deliverer.** The amendment
[`AMD-001 — Stack-Window Validator Is a Prerequisite for the D1/D2 Path`](../../amendments/AMD-001-stack-window-validator-prerequisite.md)
originally named **this** block as its §1 deliverer. Before this candidate's acceptance, the maintainer
ruled **AMEND-AMD-001-TIMING** (registered as [`maintainer-rulings.md` R-12](../maintainer-rulings.md)):
§1's four artifacts (Node stack-window validator, composite program-state cross-validation, CI wiring,
D1/D2 transition test) remain mandatory before the first post-lock stack window opens, and
unconditionally before `D1` enters `PRIVATE_CHECKPOINT` — but the delivery duty binds to whichever
accepted candidate immediately precedes that event, not to this block by name. `§§2-4` of the amendment
stand unchanged, including `§4`'s traceability duty, which this block still discharges (against the
POST-amendment text, since the amendment was rescoped before this candidate's base was finalised):

| | |
|---|---|
| identifier | `AMD-001` (as amended by R-12 — see the amendment's own "Amendment to §1's timing" section) |
| path at the base tree | `docs/arch/refactor/rev11/amendments/AMD-001-stack-window-validator-prerequisite.md` |
| base tree | `fb863297a04c7eb114d53ff65736c00240354504` (post-amendment; supersedes the pre-amendment `6af543c8a…` binding this section originally recorded) |
| **SHA-256 (lowercase hex, raw bytes), post-amendment** | `01661d01445e76f8861995061fd61511415550633a05b6ad351ec562b0ad5fd4` |
| recomputed by | `git show fb863297a…:docs/arch/refactor/rev11/amendments/AMD-001-stack-window-validator-prerequisite.md \| shasum -a 256` |

The amendment spells that command `sha256sum`; that binary is absent on the locked runner class and
`shasum -a 256` is the same algorithm over the same bytes. The digest is quoted here, never inlined
into the amendment — a self-digest is a fixpoint. **This block delivers none of §1's four artifacts** —
that is now correct per the rescoped timing, not a gap. The matching binding on the packet side is the
addendum at the end of [`context-packet.md`](context-packet.md); a second addendum there records the
rescope and the digest change.

**None of `AMD-001` §1's four deliverables is delivered by this candidate**, and the amendment's own
terms make that a required input rather than prose to rediscover, so it is stated plainly here rather
than left to be inferred from an unresolved-items row. Undelivered:

1. the Node stack-window validator (the `validate_stack_window.py` reimplementation under ruling R-4);
2. composite program-state cross-validation — the stack-window validator and
   `scripts/validate-program-state.mjs` run against each other's records;
3. CI wiring for the new validator's suite, in the `test:scripts`/path-filter pattern;
4. the discriminating `D1`/`D2` transition test.

What **is** satisfied: §4's mechanical traceability, above; and §3, the rule that matters most —
the program-state validator's fail-closed refusal of a begun successor of a `PRIVATE_CHECKPOINT`
predecessor is **untouched by this candidate**, not deleted, not weakened, not bypassed. That refusal
is what keeps the unmodelled path closed rather than open in the interim, and it may be removed only
by being superseded by the delivered composite validation.

The deferral was **not a decision this lock was entitled to make on its own**: `governance.md` §10
assigns it to the maintainer, and AMD-001 §3 forbids the disposition being taken unilaterally. It was
recorded as a deviation memo, in §10's required form, at
[`AMD-001-deviation-memo.md`](AMD-001-deviation-memo.md), recommending `DEFER`. The maintainer RULED
**AMEND-AMD-001-TIMING** instead (registered as [`maintainer-rulings.md` R-12](../maintainer-rulings.md)
and inside the amendment itself, "Amendment to §1's timing") — neither the memo's own `DEFER`
recommendation nor the alternative `DELIVER-NOW` instruction this block received first; the ruling
rescopes §1's delivery duty off this block by name rather than leaving it as an open debt against this
block. The memo's status is updated to record that ruling rather than left `PENDING`. §11 row U-9 is
updated to match: it no longer reads as an open deviation against this block, because the amendment
text itself no longer names this block as deliverer.

**Restack/range-diff/CI/review invalidation rule.** A base advance under an open window requires a
restack; restacked layers are new candidates; a restack invalidates every review verdict bound to the
superseded candidate identity, because a verdict binds one exact SHA **and** tree. CI invalidation is
not applicable — no CI runs — so the canonical local gate is re-run on the restacked candidate
instead.

**Atomic private-layer and checkpoint/acceptance landing rule.** A private checkpoint layer never
lands independently and never receives maintainer acceptance on its own; the acceptance-and-landing
layer is the sole mergeable unit of that pair.

**Post-merge candidate-delta/generated-output equivalence verification.** When the accepted identity
equals the reviewed candidate identity — the normal case under linear fast-forward landing — no
landing-equivalence artifact is required. When it diverges, the artifact binds base, candidate and
accepted identities plus the exact candidate-delta and post-landing proof, and its digest is recorded
in the ledger.

**Review mandates.** Foundational work takes three distinct mandates — conformance, architecture,
adversarial performance/memory — in three independent contexts, each returning exactly one of
`PASS` / `BLOCKING FINDINGS` / `NOT PROVEN` / `NON-BLOCKING DISCOVERIES`, bound to one exact
candidate SHA **and** tree. Four hard exclusions: the implementer context fills no mandate on its own
block in any round; the orchestrator's synthesis is not a mandate; a reviewer that applies a fix does
not re-approve its own patch; and three instances of the same prompt are one context, not three.

For an evidence-only block the adversarial mandate is **re-pointed, not waived** — at whether the
claims are falsifiable and false-negative-resistant, by re-running the load-bearing derivations.

| Initial block | Context packet digest | Worktree/branch | Stack/layer | Worker | Review mandates |
|---|---|---|---|---|---|
| `B1` | minted by the orchestrator at dispatch from [`B1-context-packet.md`](B1-context-packet.md); digest recorded in the ledger at that transition | `work/b1-neutral-contracts`, sibling worktree | none — depth 1, `stack_id = ""`, `stack_layer = 0` | one implementer, one writable worktree | conformance, architecture, adversarial performance/memory — three independent contexts |

# 10. First unlocked charters

| Block | Charter digest | Implementation baseline SHA | Review class | Scoper/challenger | Predecessors verified |
|---|---|---|---|---|---|
| `B1` | `shasum -a 256 docs/arch/refactor/rev11/charters/B1.md` at acceptance; recorded in the ledger's `block.B1.charter_digest` | `fb863297a04c7eb114d53ff65736c00240354504` | Foundational — all three mandates required | orchestrator scopes; the architecture mandate is the challenger and may challenge the charter itself | **yes** — sole predecessor is this block; accepted at the transition that accepts this record |

`B1`'s bound charter is [`../../charters/B1.md`](../../charters/B1.md), which supersedes the unbound
template and fills the source-specific closure the template left to this lock: current owners, the
locked dependency-direction strategy, the ratified equality-pinned exception, the enumerated public
and wire consumers, the required commands, and the performance cell.

**`J1` is not unlocked.** The template's required set is `B1`, "and optionally `J1` when CSS work is
selected". CSS work is not selected: no CSS cell is locked in the gate file, the baseline workload
records zero CSS work, and nothing before or within `B1` depends on the CSS inventory. Unlocking it
now would put a block in flight whose evidence nothing consumes. §11 row U-8 records the resolution
point — the maintainer selecting CSS work, at which point `J1`'s charter is bound and a CSS cell is
added by extending this lock.

No later block is unlocked merely by being listed here. Contingent stacked draft or review work is
legal only under the validated stack contract; no successor may become acceptance-recommended until
every predecessor is formally satisfied and the candidate is restacked and revalidated.

# 11. Unresolved items

The template requires every item here to be a private implementation choice that cannot change
semantics, identity, lifetime, cache validity, mapping interpretation, compatibility, dependency
direction, or pass/fail gates. **Two of the rows below do not meet that bar, and are recorded as
exceeding it rather than quietly filed under it** (U-1, U-4). Each is a deferral of *program
scope*, not a private choice, and each names the gate that must resolve it. They are stated here
because the alternative — inventing the content — would freeze a guess as authority, and because the
gate for each falls after the work this lock unlocks.

U-9 was a third such row while it stood as a recorded deviation against an amendment that named this
block as deliverer. Ruling R-12 rescoped that duty off this block before acceptance, so U-9 is now
**informational**: it records that the rescope happened and where the obligation went, and it is no
longer a deviation this lock carries. U-12 is likewise informational — it discloses a staleness in
an authority file that this record deliberately does not edit.

| Item | Why non-blocking | Owner | Resolution point |
|---|---|---|---|
| **U-1** Capability matrix is entirely `VERIFY`; no maturity, default or compatibility promise is ratified | Fail-closed: an unratified row is not approved for architecture claims or default changes, so nothing can rely on it by accident. **Exceeds the §11 bar** — it is a compatibility decision, deferred, not a private choice | product/conformance review with the maintainer | **before the atomic flow-cutover block begins.** That block's charter requires it to satisfy every Supported/Stable row in this matrix; with none declared, the obligation is vacuous until the matrix is ratified |
| **U-2** Stable-ID collision/equality policy and the output/presentation/serialization/execution profile schemas are stated as constraints, not as schemas | The types they govern do not exist yet; the constraints they must satisfy are locked in §4 and in the bound charter | `B1` | `B1`'s accepted candidate |
| **U-3** `type_env_hash` and `lib_env_hash` have no production input | Nothing varies, so nothing collides; this is a missing ingress, not a live cache defect | `B1` owns modelling the dimension honestly; the block that threads real values owns the blast radius | when real configuration values are first threaded through — that change alters cache identity for every existing project at that moment and must be landed as such |
| **U-4** `provider_protocol_version = 12` may duplicate a compatibility domain owned elsewhere | Recorded **NOT PROVEN**, not assumed either way; no block through this one reads or writes it. **Exceeds the §11 bar** — it is a compatibility question | the protocol/provider convergence block | that block's accepted candidate |
| **U-5** Current IDE/build parser front-ends and direct/managed/FFI routes are recorded thin | The enumerations exist in the owner-rows inventory; only the blocks that consume them as a closure need them assembled, and none is unlocked | the shared-front-end, direct-compiler and managed-runtime blocks | each block's own scoping, from the existing inventory |
| **U-6** No owner memory budget, allocator slack, quiescence protocol or soak cell is locked | Whole-process memory is gated by the locked cell's peak-RSS metrics; the owner memory model does not exist yet, and a ~70 ms batch cell cannot carry a soak gate | the managed-runtime and lifecycle blocks | extending this lock with those cells, before the first block whose acceptance depends on them |
| **U-7** Exact structural completion and its discrimination remain open debt | The safety retraction already refuses the wrong-complete results; the remaining gap is unsupported behavior, not incorrect published behavior | the completion-graph block | that block's accepted candidate; no second classifier may be created to work around it |
| **U-8** `J1` not unlocked; no CSS benchmark cell locked | CSS work is not selected and nothing unlocked depends on the CSS inventory | maintainer | maintainer selects CSS work → `J1`'s charter is bound and a CSS cell added by a new lock digest |
| **U-9** *(informational — no longer a deviation against this lock)* All four of **`AMD-001`** §1's deliverables — the Node stack-window validator, composite program-state cross-validation, that validator's CI wiring, and the discriminating checkpoint/acceptance transition test — are **not delivered by this lock** (§9 enumerates them and binds the amendment's post-rescope digest) | The maintainer ruled **AMEND-AMD-001-TIMING** ([`maintainer-rulings.md` R-12](../maintainer-rulings.md)) **before this candidate's acceptance**: §1 is amended in place so the four artifacts bind to whichever accepted candidate immediately precedes the first opened stack window, and unconditionally to the one before `D1` enters `PRIVATE_CHECKPOINT` — **not to this block by name**. So this row records no open deviation and no unratified choice; the amendment text and the delivery reality now agree. The [`AMD-001-deviation-memo.md`](AMD-001-deviation-memo.md) is retained as the historical record that a `DEFER` recommendation was made and was superseded by a *different* ruling. Substantively unchanged: no window is open, the unlocked block is sequential and single-layer, and the program-state validator's fail-closed refusal is untouched here, so the unmodelled path stays closed | a later block, under the amended §1 — **not this one**; the orchestrator carries the duty forward to the candidate the amended timing names | **before the first snapshot with more than one open layer is minted, and unconditionally before the private-checkpoint block begins.** The amendment's refusal is superseded by delivering the validator, never by deleting it |
| **U-10** CI wiring for the instrumentation feature arms | No GitHub Actions job runs for any block of this program, so a CI job added now would not execute; the arms are locked as required per-block commands instead | post-program | after the program lands on `main`; requires a ruling extending the one narrow CI-wiring authorization |
| **U-11** The new gate validator is not added to the CI change-detection path filter | Same reason as U-10, plus: the existing authorization for a `.github/` edit was granted for one named purpose only, and extending it needs its own ruling | post-program | with U-10 |
| **U-12** `PROVENANCE.md`'s published aggregate digest is stale with respect to the tree it describes | The rescope commit `fb863297a…` edited `amendments/AMD-001-…md`, which is inside the aggregate's input set, without republishing the aggregate; the published value `ff49cdd…` now reproduces only at the pre-rescope tree. Non-blocking here because this record does not consume the published value as authority — §1 recomputes both aggregates from the git object store and records the recomputed pair, and it reproduces `ff49cdd…` at the pre-rescope tree as the method control. **Not corrected by this candidate:** `PROVENANCE.md` is an authority file of the integration lineage, not a block artifact, and a lock candidate silently rewriting an authority digest is exactly the move this program's evidence discipline forbids | orchestrator, on the integration lineage | at, or before, the transition that accepts this record — republish the aggregate as `a061d97534f2b96f96a92eae24569f8500c696a4b82d827efbd1a52deb78a173` (74 files) at the accepted tip, or record why the published value is pinned to a superseded tree |

# 12. Acceptance checklist

- [x] exact entry checkout, exact implementation baseline, tree OID, record digest rule, and
      authority digest recorded — §1
- [~] all canonical commands non-vacuous — §2: every command executed non-zero intended work, and
      the selectors are proven non-vacuous. But **the canonical gate returns FAIL**, on a defect that
      pre-exists this baseline plus two load-sensitive tests that pass in isolation. Recorded as a
      classified failure, not as a pass
- [~] capability/protocol/consumer inventory complete — protocol and consumer inventories are
      complete (§5); the **capability matrix is unratified** and recorded as U-1, fail-closed
- [~] identity/profile/compatibility decisions accepted — compatibility decisions accepted (§5);
      identity/profile **schemas** are the unlocked block's deliverable, with their constraints
      locked here (§4, U-2)
- [x] performance gate file contains no placeholders/zero-required fields — machine-checked by
      `scripts/validate-performance-gates.mjs`, with a twenty-control discriminating suite
- [x] raw baseline and noise measurements retained — §7
- [x] semantic safety retraction complete for its declared scope — §8, with the remaining structural
      gap recorded as U-7 rather than claimed closed
- [x] maintainer/orchestrator identities and program-state/evidence custody accepted — §1, §9
- [x] worktree/branch/CI/merge/stack/restack policy accepted — §9
- [x] first foundational charter, context packet and stack placement accepted — §9, §10
- [~] no unresolved public/semantic/identity/lifetime/cache/compatibility/gate issue — **not clean.**
      U-1 and U-4 exceed the §11 bar and are named as such. U-9 no longer does — ruling R-12
      rescoped the amendment obligation off this block before acceptance, leaving that row
      informational. No *gate* issue is open: the gate file is complete and locked
- [ ] exact SHA/tree architecture and adversarial evidence accepted — pending the three review
      mandates against one unchanged candidate SHA and tree
- [x] no agent may self-accept, weaken gates, or merge private atomic layers independently — §9

The three `[~]` rows and the two `[ ]` rows are the honest state of this record at draft. A checklist
ticked complete while U-1 sits unratified would be exactly the failure mode this program's
verification rule exists to prevent.


---

# 13. Gate-file extension register

`performance-gates.toml`'s SCOPE header allows cells to be ADDED for later blocks and requires each
addition to carry "a new lock record digest and the same independent review class". This section is
that register: it is the record's list of gate-file extensions accepted after this record's original
acceptance. Adding a row changes this file's bytes and therefore its digest, which is exactly the
mechanism the SCOPE header asks for. **No row here may weaken, reweight, subset or reinterpret an
existing cell**, and none does: every extension below is strictly additive, and `[primary_suite]`,
`[runner]`, `[statistics]` and the A6 cell are untouched by all of them.

| # | Cell(s) added | Owner | Landed | Threshold source |
|---|---|---|---|---|
| E-1 | `BF2_VUE_ORACLE_MANIFEST_GENERATE`, `BF2_SVELTE_ORACLE_MANIFEST_GENERATE` | BF1 (for BF2) | `630595072` | A 10-invocation session of the BF1-owned, already-authored `generate-official-case-manifests.mjs` against the pinned oracle sources — a reference tool that is not BF2's candidate harness |
| E-2 | `B6_COMPILER_ROUTE_OVERHEAD` | B6 | this record's amending commit | Absolutes from the already-locked A6 cell's per-component product budget; relatives from a frozen formula instantiated on a neutral B5-direct calibration session, confirmed by a disjoint holdout |

**E-1 is recorded retroactively.** That extension landed without amending this record, so the
"new lock record digest" half of its own SCOPE rule went unsatisfied at the time. Recording it here
does not re-open or re-review it — its cells, thresholds and evidence are unchanged — it closes a
bookkeeping gap that would otherwise make this register look complete while omitting the first
extension that ever happened. The gap is disclosed rather than quietly backfilled.

**E-2 threshold provenance, in full.** This is the extension this amendment exists for, and it is the
one case in this file where the block that will be MEASURED by a cell is not permitted anywhere near
the choice of that cell's numbers.

- **Absolute wall `20_000_000` ns.** `A6_META_COMPILE_40_COLD_RUST` locks 100 ms for 40 components,
  i.e. 2.5 ms per component, for a **heavier** workload: a fresh host, upsert, load, per-component
  metadata, then a host-backed batch compile. The route-overhead cell's direct arm is
  `StandaloneCompiler::compile` over eight local sources with no host, no component-meta and no VFS.
  A strictly lighter path may not be budgeted slower than an already-locked heavier one at the same
  per-file product rate, so the budget is 8 x 2.5 ms.
- **Absolute peak RSS `134_217_728` bytes.** Half of A6's 256 MiB catastrophe stop for a 41-file
  host process, for an eight-file process with no host or session at all. Like A6's, this is a
  catastrophe stop; the tight fence is the relative bound.
- **Relative bounds.** `max(3.0000, 2 x population CV)` — `[statistics].no_regression_floor_percent`
  and `noise_multiplier` — frozen in
  `docs/arch/refactor/rev11/evidence/B6/cell-lock/pre-measure-registration.md` section 7 and
  committed **before** the calibration session ran. Instantiated on the B5 direct leg, the
  pre-existing one-shot path B6 replaces: wall CV 5.1678% gives
  **10.3356%**, peak-RSS CV 0.5986% gives **3.0000%**. Truncated at
  four decimal places, never rounded up: verification.md 8.3 is an upper bound.
- **What none of them is.** No threshold is `k x` any B6 observation, and none was read from B6's
  own measurement evidence. B6's existing timing and RSS figures additionally failed the
  idle-machine protocol and are retained as contaminated audit evidence only.

**E-2 sessions.** Calibration 30 cold invocations, median wall 0.3663 ms,
max peak RSS 6.13 MiB. Disjoint holdout 30 cold invocations,
median wall 0.3651 ms, max peak RSS 6.11 MiB. The
holdout is the pass/fail evidence and it passes both absolutes with the observed
holdout-to-calibration wall drift at 0.3186%, inside the 10.3356% bound. Every
invocation in both sessions reproduced the pinned output digest, so the correctness oracle held
throughout. Raw per-invocation samples and control readings are committed under
`docs/arch/refactor/rev11/evidence/B6/cell-lock/`.

**Which E-2 gates can actually fail.** Recorded here because a reader judging a future B6 run needs it,
and because the honest version is less flattering than the headline. BOTH wall metrics have near-zero
teeth, and the ABSOLUTE is the weaker of the two: 20 ms sits 54.77x above the holdout median
(0.3651 ms) and first trips at roughly a 5377% regression, while the
10.3356% relative bound rests on a 5.1678% wall CV — 3.50x A6's
1.4757% measured noise floor, so the bound is 3.45x wider than A6's 3.0%. That is scale, not
sloppiness: the operation is ~0.3651 ms against A6's ~70 ms, so cold-process startup
jitter dominates. The peak-RSS ABSOLUTE is weak too (20.95x headroom) and is a catastrophe stop,
as at A6. E-2's real discriminating power is the output oracle, the two-sided work counters
(8 / 8 / 5384 exact equality), the peak-RSS RELATIVE bound (3.0000% against a
0.5986% CV and a 1.29% observed excursion), and the three structural
route counters. A block wanting a tight wall bound adds an in-process arm excluding process startup and
calibrates it under this discipline; it does not narrow this bound after the fact, which ADR-016 forbids.

**E-2 forward hazard: the corpus pin versus the three unmeasured arms.** `corpus_fingerprint` pins
harness git-blob `6c69bd6e6b0f674eec20d92aff9080aad0f877ad`, and A6's discipline treats a run whose harness blob differs as not
this cell. That blob deliberately REFUSES `--arm prepared-first|prepared-repeat|batch` — which is why
no fabricated baseline exists for them — so measuring the three arms E-2 gates necessarily requires a
different harness blob and therefore necessarily breaks the pin. Neither this record, the ruling, nor
the registration says how that resolves, and this register does not invent a resolution. **Owner: B6**,
which must settle it before claiming E-2's arm metrics, by an explicit route (re-pin under the
recalibration rule with the direct arm's numbers reproduced, or a successor cell id) rather than by
silently measuring against a different blob.

**E-2 evidence caveat.** `route.direct.payload_bytes` is gated at exact equality 5384 but has no
per-invocation column in the raw sample rows: the recorded per-invocation evidence is the output digest,
and identical digests imply identical code bytes and therefore identical payload length. The section 10
condition-4 claim for payload_bytes is sound BY IMPLICATION from the digest, not by direct measurement,
and is stated that way rather than presented as a recorded number.

**E-2 outstanding governance step.** The SCOPE header requires an extension to carry a new lock record
digest AND the same independent review class (ADR-016). This register delivers the digest half. The
independent performance reviewer's sign-off on this specific addition is an OUTSTANDING follow-up, in
the same posture the BF2 banner records for E-1 — the cell is locked and binding, but no claim is made
here that that review class has signed it off.

**What this register does not do.** It does not accept B6, amend B6's charter, alter the DAG, or add a
ledger block row. B6 is still measured against E-2 later, on its own idle-machine run.
