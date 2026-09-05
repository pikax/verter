<!-- unified-charter-v2
id=CVO2
name=Non-gating CI benchmark observation artifacts
phase=compiler
train=compiler.validation-observability
product=validation_observability
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CVO1,CVO1S,CPER0M
owner=compiler.validation-observability:test-only validation and observability lane
conflict_domains=validation_observability,release_orchestration,github_projection_state
resource_class=rust-mixed
review_profile=semantic-3
gate_profile=targeted-domain
implementation_effort_min=medium
implementation_effort_default=high
review_effort_min=medium
review_effort_default=high
verification_effort_min=medium
verification_effort_default=medium
confirmation_effort_min=medium
confirmation_effort_default=medium
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=pinned_vue_benchmarks_checkout,pinned_svelte_benchmarks_checkout
charter=charters/compiler-validation-observability/CVO2.md
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CVO2 — Non-gating CI benchmark observation artifacts

Readiness comes only from trusted implementation-ledger rows. A READY node may start; tooling does not validate commit locators, Git identity, receipts, leases, external state, or runtime admission.

## Independently acceptable outcome

Benchmark numbers recorded alongside the workload probes as machine-readable observation artifacts with correctness labeling and reproducibility metadata, without becoming performance acceptance gates. The current owner is **no recorded benchmark history for probed workloads**. The final and sole owner is **the non-gating benchmark observation lane and its correctness-labeled artifacts**. This charter accepts one boundary; it contains no independently dispatchable subblocks.

## Concrete surfaces and APIs

- Production surfaces: none.
- Benchmark scripts deliberately not reused: `packages/benchmark` `bench:json` emits a pass/warning/fail status and `bench:svelte:compiler` applies thresholds and exits non-zero, both of which are performance verdicts this node must not carry. Observation capture is a raw adapter in the probe crate over the CVO1/CVO1S selections, driven through the CVO1 `probe-driver.mjs` boundary extended by this node with one additive, backward-compatible observation command it owns: the request line gains `observe?: { memory: boolean }` and the compile frame gains `memory?: { peak_bytes, live_bytes }`, present only under `observe.memory` and taken in the driver's own process as `memoryAuditEnable()` once at driver start, `memoryAuditResetHighWater()` immediately before the bracketed `compileRequests` call, and `memoryAuditSnapshot()` immediately after it (`crates/verter_napi`, owned by CPER0M; `memoryAuditSites` is not used); CVO1's own runs never send `observe`.
- Named API/data boundaries: `crates/verter_validation_probe/src/observe.rs` — `ObservationArtifact` written to `target/validation-probe/observations.json`, with `ObservationArtifact::validate(manifests)`, which binds the artifact to the selected manifest grid: exactly one `cold` and one `warm` row per selected case of each framework, unique row ids, sample cardinality one for `cold` and five for `warm` — a smaller artifact is invalid, never a smaller inventory; the artifact carries an immutable `artifact_id = "<verter_commit>/<corpus_digest>/<workflow_run_id>/<run_attempt>"` where `corpus_digest` is the lowercase-hex SHA-256 of the UTF-8 bytes `"svelte=<sha>\nvue=<sha>\n"` — framework keys sorted ascending bytewise, one `key=value` line each, LF-terminated, the same encoding in the writer and in every validator — over the `corpus_revisions = { vue: <rev>, svelte: <rev> }` map that the header also stores in full, and every row carries its own framework's `corpus_revision`; `validate` rejects a missing framework revision, a row whose revision differs from the header entry for its framework, and swapped framework revisions and one `ObservationRow` per `(case_id, mode)` with `row_id = "<case_id>@<mode>"` where `case_id` is the manifest's existing id verbatim (`vue/<relative-path-in-corpus>` or `svelte/<relative-path-in-corpus>`, never re-prefixed) and `mode ∈ { cold, warm }`, so the join key is the manifest key; observation identity is the pair `{ artifact_id, row_id }`, unique across commits and runs; `cold` is one fresh worker process per case issuing one request, `warm` is five in-process repeats after one discarded request in one worker per case; if that worker dies or times out mid-row the remaining warm samples record `absence = compile_terminated` and the worker is not restarted for that row (a restart would silently mix cold and warm), so the row stays total; each row carries `samples: Vec<Sample>` (never a single aggregate) where `Sample { terminals: BTreeMap<Dimension, Terminal> (exactly one entry per CVO0 `Dimension`, all six, in every sample including absence samples, unreached dimensions carrying `NotRun { blocked_by }` and inapplicable ones `NotApplicable`; `validate` rejects a missing or extra key), measurement: Option<Measurement { elapsed_ns, memory: Option<{ peak_bytes, live_bytes }> }>, absence: Option<AbsenceReason { load_failed | compile_terminated | reference_terminated | driver_error }> }` — `measurement` is present exactly when the bracketed `compileRequests` call completed (a compile frame was ingested), `absence` is present exactly when some phase terminated abnormally, and the two coexist only for `reference_terminated` (the compile measurement is real; only the reference died); `load_failed`, `compile_terminated`, and `driver_error` carry no measurement, a sample with neither field is invalid, and memory validation applies to measured samples only — so every termination keeps its sample (totality) with a typed reason and no invented timing, and no completed timing is ever discarded — the CVO0 `Terminal` serialized whole as a tagged union (`{ kind: "class", class, secondary: [...], evidence: [{ source, message }] } | { kind: "not_run", blocked_by } | { kind: "not_applicable", reason }`), so blocked dimensions and secondary evidence survive in the artifact records the per-dimension outcome of that very invocation (cold and every warm repeat alike), `sample_count`, `comparison_eligible`, and, inside each `Sample`'s `measurement`, that invocation's own optional `memory: { peak_bytes, live_bytes }` copied from its compile frame's `memory` field (the observation command this node owns, in the exact `memoryAuditEnable` → `memoryAuditResetHighWater` → call → `memoryAuditSnapshot` order; owned by CPER0M) — there is no row-level memory field and no aggregation, and `validate` rejects a row whose samples disagree on memory presence; the artifact header is the typed, fully required `ArtifactHeader { artifact_id, verter_commit, corpus_revisions, corpus_digest, workflow_run_id, run_attempt, rust_version, node_version, addon_version, os, arch, execution_mode, sample_plan: { cold: 1, warm: 5 }, request_vue, request_svelte, template_digests: { vue, svelte }, throughput: { cases, wall_ns } }`, every field validated by `ObservationArtifact::validate` (a missing or mistyped header field, or a `sample_plan` that differs from the rows, is rejected), the `throughput` entry covering the whole selection in one process. The schema has no status, verdict, threshold, or baseline field, so a performance verdict is structurally unrepresentable. Retention and retrieval boundary: the workflow uploads the file as the GitHub Actions artifact `validation-probe-observations-<workflow_run_id>-<run_attempt>` under the repository's artifact retention; nothing else stores it. Retrieval is defined end to end: `ObservationInventory::fetch(owner, repo, dir)` lists every artifact with `gh api --paginate repos/{owner}/{repo}/actions/artifacts` (GitHub's `name` query is an exact match, so no server-side prefix filter is used) and keeps those whose `name` starts with `validation-probe-observations-`; a listing test feeds differently suffixed and unrelated names through that filter; it then downloads each kept non-expired artifact through `actions/artifacts/{id}/zip`, extracts it to `target/validation-probe/inventory/<artifact_key>/observations.json` where `artifact_key = "<verter_commit>-<corpus_digest[0..16]>-<workflow_run_id>-<run_attempt>"` is the filesystem-safe single-component encoding of the identity (the full `artifact_id` stays inside the file and is what every identity check uses), runs `ObservationArtifact::validate` on each and binds each file to trusted run metadata rather than to its own claims — the artifact listing carries only the run id, repository ids, branch, and SHA, so for each distinct `workflow_run.id` the fetcher performs one `GET repos/{owner}/{repo}/actions/runs/{id}` and reads the default branch once from `GET repos/{owner}/{repo}` (`default_branch`); before any download it requires the run's `path` to be `.github/workflows/validation-probe.yml`, `event` `push`, `conclusion` `success`, `head_branch` equal to the default branch, and `run_attempt` equal to the artifact-name suffix; then the embedded `verter_commit` must equal that run's `head_sha`, the embedded `workflow_run_id` and `run_attempt` must equal the run's id and the name suffix, the header's `corpus_revisions` must equal the `external_revision` of each framework manifest as it stood at that run's trusted `head_sha` (read with `git show <head_sha>:crates/verter_validation_probe/manifest/<framework>.toml`, never from the working tree), the `corpus_digest` is recomputed from those run-bound revisions (not from the header), and an older artifact recorded under a previously pinned corpus revision therefore stays a valid historical observation after a re-pin instead of aborting retrieval (a two-revision retention fixture proves it); the currently supplied manifests are used only for current-state decisions, and the complete `artifact_id` is recomputed from these independently validated fields and must equal the embedded one; the same `{ artifact_id, row_id }` appearing in two downloaded files is rejected rather than collapsed (planted negatives: a stale file re-uploaded under a newer Actions identity, an artifact whose `verter_commit` differs from `head_sha`, and an artifact from a different workflow) — and yields the exact set of `{ artifact_id, row_id }` identities plus `retrieval_window = { started_at, oldest_created_at, artifact_count }`. Cutoff rule, by immutable timestamps only: the inventory is every trusted artifact with `created_at <= started_at` and `expires_at > started_at`, so an artifact uploaded during the fetch is excluded deterministically (one planted concurrent-new-artifact fixture) and an expiry observed mid-fetch does not change membership; the listing has no stable sort or cursor guarantee, so the fetcher performs bounded repeated full scans (`per_page=100`, walked to the end) and accepts an inventory only when two consecutive scans yield the same eligible artifact-id set, aborting after three non-matching scans; a transient page failure retries up to three times with backoff and otherwise aborts the fetch as a whole (a partial inventory is never returned; a planted fixture inserts a post-cutoff artifact that shifts a page boundary between scans), and the join record carries the retrieval window so a truncated window is visible rather than silent. A durable archive beyond the Actions retention period is out of this train's scope and belongs to the performance authority (`compiler.compiler-perf`), which owns performance evidence; until then observations are evidence for the retention period only, and CVO4 states which window it joined.
- Test/CI homes: `crates/verter_validation_probe` (`src/observe.rs`, cases under `tests/cases/observe.rs` through the single `tests/main.rs`) and the observation job in `.github/workflows/validation-probe.yml` (a `release_orchestration` root this node leases). `packages/benchmark` and `crates/verter_bench` are not touched by this node.
- Mutation boundary: test/CI/evidence bytes only; production LOC is zero.

## Exact predecessor contracts

- **CVO1:** implemented ledger row for "Pinned vue-benchmarks external workload probe lane"; observations ride that lane's pinned slice, stable case ids, and summary artifact. Ledger presence alone satisfies the predecessor. Its locator metadata remains non-authoritative.
- **CVO1S:** implemented ledger row for "Pinned svelte-benchmarks external workload probe lane"; observations cover the Svelte slice with the same per-framework labeling, so no framework is reported without the other. Ledger presence alone satisfies the predecessor. Its locator metadata remains non-authoritative.
- **CPER0M:** implemented ledger row for "NAPI memory-audit snapshot coherence"; it proves only that the native memory-audit peak and live-bytes pair is coherent, so the sole memory fields this node may record are that peak and live-bytes high-water pair. Allocation counts, allocation-site sampling, or any per-lifetime attribution are not backed by it and are omitted until their own measurement path is named and proven by the CPER train. Ledger presence alone satisfies the predecessor. Its locator metadata remains non-authoritative.
- **External requirements:** agents check `pinned_vue_benchmarks_checkout` and `pinned_svelte_benchmarks_checkout`; tooling does not validate external state.

## Source-specific scope

- **Intent:** visibility and historical evidence, not performance policy.
- **Problem:** without recorded observations there is no historical evidence for later performance-authority nodes to adopt, and wall-clock claims cannot be checked for correctness-equivalence after the fact.
- **Solution and architecture decisions:**
  - capture through the raw adapter: cold and warm compile samples per case, selection throughput, sample counts, and workload size/case count; the existing suites' native statistics are not imported because they are computed under threshold policies;
  - memory data only as the CPER0M-backed peak/live-bytes pair; no allocation counts or sampling; no new instrumentation invented solely to increase the metric count;
  - every artifact identifies Verter commit, external workload revision, Rust/Node/toolchain versions as relevant, runner OS/architecture, execution mode, warm/cold state, sample/run count, and the canonical request templates verbatim (`request_vue`, `request_svelte`) with their `template_digests`, every row additionally carrying its own `request_digest`, so the compiler options that selected the workload are bound to the numbers;
  - correctness labeling separates two facts: `observed_outcome: BTreeMap<Dimension, Terminal>`, a deterministic projection — per dimension, the highest-precedence terminal across all of the row's samples under one total order: a concrete failure terminal ranks by the CVO0 precedence of its class, a `NotRun { blocked_by }` ranks by the precedence of `blocked_by`, a `NotApplicable` ranks below every failure class and every `NotRun` and above `pass` (any actual failure or blocked execution outranks inapplicability; mixed `NotApplicable`/`NotRun` and `NotApplicable`/failure samples are planted projection fixtures), when a concrete terminal and a `NotRun` tie on class the concrete one wins (it is the direct observation; `NotRun` is derived), and when two concrete terminals tie on class the stored terminal carries that class with the union of their `secondary` classes deduplicated and sorted in CVO0 precedence order, and the union of their `evidence` deduplicated by `(source, message)` and sorted lexicographically by `source` then `message`, so no causal evidence is lost and the recomputation is canonical (a tied-class fixture with differently ordered evidence proves it) and exactly one canonical terminal is chosen and stored whole — written by the adapter and re-derived by `ObservationArtifact::validate`, which rejects a row whose stored projection differs from the recomputation, and `comparison_eligible`, which is `false` by default for every result and may be `true` only when two separate citations are present on the row: `semantic_basis = { authority, atom }` (the framework's product authority, e.g. `vue.runtime-client-product`), which `validate-probe-authorities.mjs --observations` validates against the case's framework and the `Structural` dimension, and `equivalent_work_basis = { authority, atom }` (`compiler.equivalent-work-ledger`), validated against `Performance` with framework `any`. For a row claiming `comparison_eligible = true` both cited owners must have `implemented` ledger rows; a pending citation is permitted only on a row with `comparison_eligible = false`, and two planted rows (pending semantic owner, pending equivalent-work owner) are rejected by `validate-probe-authorities.mjs --observations`. Eligibility additionally requires evidence, not only citations: every sample of the row must carry `pass` at `Route`, `Compile`, `Structural` (the last under the authorized comparator), and `Performance` (the measured invocation itself completed) — no failure class, no `NotRun`, anywhere on the row — and the row's framework manifest must declare `comparison = structural` — a Svelte row (`comparison = none`) is ineligible until CVO4 binds a Svelte structural path. A row that claims `comparison_eligible = true` with either basis missing, with a basis whose catalog row does not cover the required framework or dimension, with any sample whose `Structural` terminal is not `pass`, or for a framework without a structural comparison, is rejected; a row with `comparison_eligible = false` and no bases is an ordinary, valid observation; a semantic owner is never asked to cover `Performance` and the equivalent-work owner is never asked to cover a semantic dimension. A probe observation never sets it; a workload with `comparison_eligible = false` may record timing as observation but must not be ranked as evidence that Verter is faster or slower for equivalent work.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **CVO2-AC1 — sole-owner outcome:** no threshold, rebaseline policy, or pass/fail performance verdict exists anywhere in the observation lane; artifacts record and label only. Static enforcement preferred.
- **CVO2-AC2 — positive contract:** an observation run emits one artifact with the full reproducibility metadata block, per-row `observed_outcome` equal to its recomputed per-dimension projection, and `comparison_eligible = false` on every row lacking either basis or any passing-structural sample; planted rows claiming `comparison_eligible = true` with the semantic basis missing, with the equivalent-work basis missing, with a basis of the wrong dimension, with one cold sample at `Structural = semantic_mismatch`, with mixed warm samples (four `pass`, one `semantic_mismatch`), with one warm sample at `Performance = timeout`, with a sample carrying neither `measurement` nor `absence`, with `absence = compile_terminated` beside a measurement, and for a Svelte row are each rejected by the artifact validator, as are an artifact missing one warm row, an artifact with a duplicated row, and a warm row carrying four samples; three termination artifacts are planted positives that must validate as total rows: load failure and compile crash mid-warm with typed `absence` and no `measurement`, and reference death with `absence = reference_terminated` beside its retained, completed compile `measurement`; repeated runs on identical inputs are structurally comparable.
- **CVO2-AC3 — incremental equivalence:** not applicable; the lane owns no incremental, cache, cancellation, or publication authority.
- **CVO2-AC4 — bounded work:** not applicable; no new counters or instrumentation paths are created.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated. Do not add implementation mirrors or duplicate permutations.
- Test homes: `crates/verter_validation_probe/tests/cases/observe.rs` via `tests/main.rs`.

## Deletions and forbidden designs

- Delete or structurally reject: **threshold or rebaseline logic inside the observation lane**.
- Delete or structurally reject: **ranking of non-correctness-equivalent results as equivalent-work evidence**.
- Never introduce a "must be within N%" gate, an automatic benchmark rebaseline, a performance claim derived solely from CI wall clock, an obligation to optimize a failing benchmark, or new measurement instrumentation to inflate metric count. Numeric baselines and regression gates remain with the performance authority (CPER train).
- Do not implement successors or silently enlarge this charter. Discovery of a second independently acceptable outcome requires an amendment and a new DAG node before mutation.

## Budgets and mandatory rescope

- Target ceiling: 0 production LOC, 0 production files, 0 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or when public/wire, unsafe, concurrency, or lifetime work is combined with another major concern.
- Correctness budget: zero observation artifact without correctness labeling; zero equivalent-work ranking for non-correctness-equivalent cases.
- Performance budget: the observation lane adds zero work to the required gate: it is a separate non-required workflow job, and the default `cargo nextest` run contains no observation test.

## Abort conditions

- Stop before mutation if the existing measurement path cannot produce a trustworthy number for a metric; omit the metric rather than inventing instrumentation. Stop if an ancestor lacks an implemented ledger row or the complete diff will not fit one review context.
- Abort the candidate if any change starts gating on the recorded numbers; that authority belongs to later performance nodes.

## Targeted verification

1. Produce one observation artifact from the pinned slice locally and confirm the metadata block and correctness labels are complete.
2. Run every final command in the bound `targeted-domain` profile on the squashed review candidate; targeted success alone is iteration evidence, not acceptance.
3. Bind the preflight evidence selection and terse rationale in the review report. Behavioral tooling changes require TDD with a failing discriminating regression before the change; do not invent a test solely to populate evidence.

## Review and lower-severity findings

Apply `semantic-3`: 2 fresh distinct harness tasks covering exactly `adversarial`, `conformance`. P0/P1 block final acceptance. A P2 follows the owning review policy and must have a named owner when deferred; otherwise it blocks. P3 follows the currently binding owning policy and must be recorded when that policy requires it. Any post-review content change invalidates every verdict. Final acceptance requires the complete current-round profile to contain independent clean PASS reports on the squashed review candidate, plus `targeted` confirmation when required. A failed review/fix cycle is complete only after all assigned lenses and a FIX_REQUIRED disposition.

## Trusted implementation ledger

Before squashing or review, the implementation patch transitions this node's predeclared row in `authority/state/implemented.toml` from `status = "pending"` to `status = "implemented"` with the planned squash commit message, approximate date with timezone, and optional pull-request number. The transitioned row is the implementation fact. Commit metadata is a loose locator only and is never resolved or validated against Git or GitHub. Reviewers inspect the squashed candidate patch without SHA-, tree-, ancestry-, receipt-, lease-, or digest-bound orchestration manifests.

## Reconciled source-plan contract

**Intent:** record correctness-labeled benchmark observations with reproducibility metadata so later performance-authority nodes have historical evidence, while keeping every number non-gating.

**Problem:** unrecorded observations cannot be audited for correctness-equivalence later, and premature thresholds would create performance policy before an equivalent-work methodology is owned.

**Solution and architecture decisions:**

- raw observation adapter in the probe crate for cold/warm samples, throughput and workload size; no reuse of threshold-bearing benchmark scripts;
- memory only as the CPER0M-backed peak/live-bytes pair;
- full reproducibility metadata on every artifact;
- `observed_outcome` and `comparison_eligible` (default `false`; `true` only with a cited owning contract) on every result;
- no thresholds, no rebaselines, no optimization obligations.

**Suggested predecessors:** `CVO1` and `CVO1S` for the two lanes, `CPER0M` for the coherent peak/live-bytes pair.

**Normative source decomposition:**

1. **CVO2-A — Metric capture.** The raw adapter over the pinned selections and the memory-audit lifecycle.
2. **CVO2-B — Artifact schema.** `ObservationArtifact`, stable row ids, `validate`, reproducibility metadata, correctness labeling.
3. **CVO2-C — Observation CI lane.** Separate non-gating lane producing and retaining artifacts.
4. **CVO2-D — Non-gating proof.** Structural demonstration that no threshold/rebaseline logic exists.
5. **CVO2-E — Independent review.** Confirm no performance policy leaks in.

**Acceptance:** artifacts exist for the probed workloads with complete metadata and correctness labels; nothing gates on the numbers; historical observations remain observations.

**Forbidden:** thresholds, rebaselines, wall-clock performance claims, new instrumentation, equivalent-work rankings for partial-correctness cases.

**Deletion/abort:** delete any gating convenience added along the way; abort on untrustworthy measurement rather than reporting an invented number.
