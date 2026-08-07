# Errata to architect-v3.txt — apply before/while implementing

Final verification verdict: **READY TO IMPLEMENT.** All seven blocking items CLOSED. The
base-to-session propagation protocol is SOUND. New bad citations: essentially none.

These corrections were raised in the final pass and are folded in here rather than through a fourth
architect round (the auditor stated explicitly that they "do not require another audit round").

---

## MANDATORY — fold into Block 1A before implementing it

### `FileSourceEnv` is in the wrong compaction domain

B2's terminal-domain mapping puts `FileSourceEnv` in the **Content** domain under
`ContentGeneration`. That is unsound.

`FileSourceEnv` carries `parse_env_hash`, `parser_version`, `file_language_id` — env dimensions, not
content. The two production paths that move them deliberately do NOT advance `content_generation`:

- `configure_projects` → `configure_resolver` → `publish_snapshot`
  (`crates/verter_workspace/src/engine.rs:528-546`) republishes the env-hash tables inside
  `mutate_resolution_world` with NO `content_generation` bump. It bumps `project_generation`
  stamp-only and relies on `ProjectGeneration` facts to invalidate.
- `WorkspaceChange::ConfigChanged` (`engine.rs:1911-1915`) sets `base_changed` but NOT
  `content_changed`.

(`set_default_resolve_extensions:516` *does* bump content, with a documented rationale — which is
the tell that the general config path does not.)

> **SUPERSEDED in part (Block 1A-i).** `set_default_resolve_extensions` is NOT a source-env
> producer in the landed tree: it calls `rebuild_and_publish`, which advances the counter as its
> own first statement, so a second bump there was unfalsifiable and was deleted. The two real
> producer sites are `rebuild_and_publish` and `publish_snapshot`. See "Producer-coverage note"
> below.

**Failure mode:** a content-domain-compacted signature whose precise `FileSourceEnv` facts were
replaced by `ContentGeneration{N}` survives a parse-env / file-language change, unless that
signature independently carries a `ProjectGeneration` fact in the still-precise workspace-shape
domain. That is likely true in practice for >1024-fact signatures but is NOT guaranteed by
construction. Since those signatures are non-admitted today, this would be a NEW poisoning class —
exactly what BLOCKING-α exists to prevent.

**Fix:** give `FileSourceEnv` its own domain (`SourceEnvGeneration`), OR fold it into the
workspace-shape `ProjectGeneration` domain.

> **SUPERSEDED in part (Block 1A-i).** The own-domain arm was adopted. Its producers are
> `rebuild_and_publish`, `publish_snapshot` and `WorkspaceChange::ConfigChanged` — NOT
> `set_default_resolve_extensions` (see above). Recorded as ADOPT-NOW under "Block 1A-i outcomes".

**Do NOT** make config changes advance `ContentGeneration`. That counter gates
`built_at_content_generation` servability and route-surface edge currency; the blast radius is wrong.

Block 1A's TDD step 5 would surface this empirically, but do not rely on discovery — apply the
domain reassignment up front. It also removes the plan's dependence on an undecidable question (see
"Open" below).

---

## Non-blocking — pin during the owning block

### Block 2B — make the base-gate invariant explicit

The safety of `assert_eq!(stable % 2, 0)` at `engine.rs:815` under base-driven session propagation
rests on an unasserted invariant: *every session publication holds the base gate*. That is true
today — all three `mutate_resolution_session_locked` call sites (`engine.rs:804`, `:1779`, `:1806`)
hold it. Make it structural (a base-gate witness parameter) or at minimum `debug_assert` it in the
same block.

### B8 — record that `resolution_sessions` is never pruned

There is no `.remove` on `resolution_sessions` anywhere. Harmless today (production interns exactly
one session domain per engine — `default_resolution_session`, `engine.rs:427-433`), but the
base-mutation critical section holds the global base gate for the whole traversal, so if
multi-session ever lands the window grows linearly with accumulated, never-reaped domains. One
sentence beside the `Base-to-session propagation` bound row.

---

## Cosmetic — fix in the owning block, no design impact

1. **Threshold off-by-one.** `FACT_SIGNATURE_CAP = 1_024` with `len() > CAP` → overflow
   (`fact_read_set.rs:83`, `:352`), so 1024 is admissible and the 1025th trips. B2's prose says
   "precise through 1,023 … its 1,024th compacts." Immaterial to the design (the plan redefines the
   threshold per-domain) but the prose should be right.
2. **`NonCacheableRefusal` does not exist in the tree** — it is a name this plan introduces.
   `NonCacheableReadReason` and `NonCacheablePropagation` do exist. B5's "The returned
   `NonCacheableRefusal` exposes…" reads as a citation; mark it as new.
3. **R22 mis-attribution.** The scanner-naming prose is R3's
   (`.claude/skills/type-cache-architecture/SKILL.md:117`), not R22's (`:976`). Already covered by
   "Amend R3" in the same block; just correct the pointer.
4. **`impl sealed::Sealed for crate::VerterHost {}`** (`resolver_context.rs:716`) is missing from
   Legacy Deletions #15 alongside the `ResolverContext` impl. One-line residue the compiler will not
   flag.

---

## Verified correct — do not re-litigate

- Mapping covers every `FactVersionRef` variant: `fact_cache.rs:101-123` has exactly 7, and
  `ResolveImportsFactRef` (`:64-72`) is the 2-arm split, so v3's 8-row table is exhaustive with no
  phantom rows.
- `WorkspaceEngine.content_generation` (`engine.rs:333`) is genuinely the content chokepoint —
  `bump_content_generation_for` (`:597`) is documented as such and every content arm routes through
  it.
- The four producer files are right and complete.
- `RecoveryScope` precision is preserved; the `INITIAL` contract assertion is live at
  `resolution_currency_contract_tests.rs:786-791`.
- `ResolutionFactRoot` is embedded in BOTH `ResolutionWorldRoot` (`resolution_currency.rs:587`) and
  `ResolutionSessionRoot` (`:547`), so in-root edges get the structural atomicity claimed.
- Deleting `reverse_graph_not_wired_to_invalidation` orphans NOTHING — it is not in
  `CRITICAL_RULE_GUARDS`; the three registered guards under "Canonical Dependency Cache Rule" all
  survive. The only second consumer (`workspace_bookkeeping_invariants.rs:179-193`) is deleted in the
  same block.
- `query_ms_from_stdout` is real (`trace-component-corpus.mjs:273`), and `classifyExitStatus`
  (`:138-146`) returns `"ok"` only when `sawDoneLine`, so `Q_run` has no null hole.
- The replay primitive at `resolver_core/mod.rs:1013` is exact and runs for leader AND follower —
  it sits outside the `Follower`-only fold at `:1009`.
- **Base-to-session propagation is sound, and stronger than v3 argued.** `capture_resolution_world`
  (`engine.rs:1412-1459`) requires the base epoch stable before the base load AND unchanged-and-stable
  after the session load, for BOTH populations — so while the base epoch is odd, no capture can
  complete at all. The bad state is UNREACHABLE, not merely un-paired. A session root created during
  the window is safe: `bootstrap` starts with an empty `ResolutionFactRoot`, and any later
  publication re-enters through `mutate_resolution_session`, which takes the base gate first. And the
  mechanism is a generalization of something already in production —
  `apply_changes_with_preflight` (`:1764`) already publishes session roots inside the base odd window
  under base-before-session order.
- The nine panic arms are exactly as v3 lists them
  (`resolver_context.rs:805/836/864/958/1001/1034/1061/1087/1151`) against `EXPECTED_PANIC_METHODS`'s
  seven in `no_bare_host_resolver_shims.rs`.

---

## Open, to settle empirically in Block 1A

Whether every >1024-fact signature also independently observes a `ProjectGeneration` fact. That is
the practical mitigation for the `FileSourceEnv` gap and it is NOT statically decidable — it depends
on which consumers produce over-cap signatures. Applying the domain reassignment above removes the
dependence entirely, which is why it is the better answer than measuring.

---

## Corrections raised during Block 0B

### B5's replay citation covers only half the mechanism

B5 says of the `RequestOnly` taint replay: "The implementation pattern already
exists in `resolver_core/mod.rs:1013`, inside the stable-flight path reached by
leader and follower. Reuse that mechanism."

That site replays the `cache_refusal` for both leader and follower roles, but it
does **not** replay the leader's fact signature to the follower, and
`cache_runtime/` contains no signature-replay call at all — signature replay is
hand-placed at specific caller seams (`owner_import_surface.rs:257`,
`analysis_io.rs:424`, the `route_facts` sites).

Design B's `CallerReplay` needs BOTH halves: the refusal AND the read signature.
Do not treat `mod.rs:1013` as a complete template.

### ADOPT-NOW — Block 3D: scheduler-boundary fact + taint propagation

A live stale-serve defect proven on `2de3b2d07` is folded into this cutover as a
new block after 3C. `produce_vue_macro_codegen` runs inside
`Scheduler::execute_scoped_cache_node`, whose closure executes via
`rayon::ThreadPool::install` (`scheduler.rs:1703` → `pool.rs:189`), so the
producer installs its fact tracer on a different thread; `ACTIVE_TRACERS` is
thread-local with no bridge (`fact_tracer_tls.rs:26-58`). `fact_footprint`
(`vue_macro_codegen/runtime.rs:670-685`) holds the producer's
`Vec<FactVersionRef>` and discards it, keeping only canonical strings plus a
`facts_cacheable` bool that has zero production readers. The outer compile slot's
`ReadSetSignature` therefore omits every fact rooted at a transitively-reached
file, and a warm slot serves stale output after an edit to such a file.
Reproduced end-to-end; cause proven by a single-line negative control replacing
`cpu_pool.install(run)` with inline `run()`. Blast radius: `Session` mode,
bundler `Runtime` prop validators and the `Tsc` surface; the LSP TSX path is
likely protected only by accident of demand and that is unproven. A second,
independent class from the same seam: a fenced serve or lease miss inside macro
resolution never reaches the outer compile's `non_cacheable_read_observed()`
(`virtual_file_pipeline.rs:1733`). New acceptance IDs XT-1..XT-4.

**Sequencing consequence.** The block order becomes
`0B → 1A → 1B → 2A → 2B → 2C → 2D → 3A → 3B → 3C → 3D → 4A → 4B → 5A → 5B → 5C → 6`.
Block 3D consumes the cross-thread taint carrier Block 3B builds, so 3B's carrier
must be general enough to serve a scheduler-boundary consumer — it must not be
scoped narrowly to prepared bundles.

### Block 0B — the principal growth fixture is positive-heavy, not negative-heavy

Part C Block 0B specifies the signature-growth fixture as "More than 1,024
authored `./_chunks/*.mjs` imports". The specifier COUNT in that line is wrong
and the mechanism behind it needs stating.

The corpus is positive-heavy: 24 of 25 `./_chunks/*.mjs` specifiers RESOLVE, via
the declaration-companion substitution in `verter_workspace::resolver`
(`resolve_declaration_companion`, `.mjs` → `.d.mts`) — the runtime `.mjs` is
absent from the published package, the `.d.mts` sibling is present.

The Decision DAG changes the measured arithmetic. Each authored chunk drives
two resolution queries — the authored `.mjs` query and the normalized `.d.mts`
target query — and the witness records one derived `Decision` per query instead
of either query's transitive leaf set. The 180-specifier fixture therefore
carries 360 Decision facts, remains below `FACT_SIGNATURE_CAP`, and is rootable.
The pre-DAG calibration was 1,084 observations for the same owner (4 + 6n), so
the fixture still discriminates the compaction without rescaling the workload.

The landed fixture (`verter_session::resolution_signature_growth_tests`) uses 180
positively-resolving specifiers, asserts every one of them resolves to its
`.d.mts` companion, asserts the two-Decision-per-specifier calibration, and
proves the prepared-bundle and component-meta warm profiles.

### Block 0B — `bundle_cold_flight_runs` cannot express the overlay memo gate

The `RequestOnly` red-at-tip pairing was briefed as `bundle_cold_flight_runs > 1`
for a non-cacheable OVERLAY bundle, citing the overlay memo insert gate
(`if !non_cacheable { … overlay_bundle_memo_insert(…) }` in
`host_manage/prepared_decl.rs`). Those two do not meet: the overlay branch
returns from `materialize_prepared_decl_bundle_via_ctx` before entering the
singleflight lane, and that lane is the only site that bumps
`bundle_cold_flight_runs`. Measured, a non-cacheable overlay bundle moves the
counter by 0 on every touch.

The pairing therefore lands on the BASE bundle path
(`verter_session::non_cacheable_bundle_reuse_tests`), where the counter the brief
names is genuinely the oracle: a FENCED (`ReturnOnly`) `IndexedReady` serve makes
the bundle complete-but-refused, and each of three touches inside one request
world runs its own cold flight (3 total) while every touch correctly marks the
enclosing tracer non-cacheable. Blocks 3B/3C must move that to 1 cold flight
WITHOUT losing the per-touch non-cacheable verdict.

The overlay `:487` gate keeps its existing coverage in
`overlay_bundle_memo_tests` case (f) (`non_cacheable_materialization_is_not_memoized`),
which Block 1B already owes a restaging onto a surviving genuine refusal.

### Block 0B — repeated materializer dispatch has no existing observable

Block 0B's characterization list includes "Repeated materializer dispatch, using
an existing observable if possible or the Block 6 counter once landed." There is
no existing observable that expresses it. `MetaProvenance` carries no
per-settled-member dispatch counter; the closest rails —
`materialize_structure_fact_tracer_installs` and
`dispatch_dep_signature_fact_tracer_emissions` — count tracer installs and
fan-out calls across several unrelated sites and cannot be mapped onto "settled
member identity" without inventing the mapping the counter itself is supposed to
provide.

No fixture is landed for this item in Block 0B. It is covered by the B7
per-request counter keyed by settled member identity, landing with Block 6
(`META-1`), which is the plan's own stated fallback. This is a recorded gap, not
a silent omission.

### Block 0B — the landed fixtures must be INVERTED, never deleted

Both Block 0B fixture families assert the CURRENT defective observable, so
the implementing block reddens them. That reddening is the intended signal
that the acceptance criterion landed. Neither family may be deleted,
`#[ignore]`d, or weakened to accommodate the change.

`verter_session::resolution_signature_growth_tests` — Block 1A (`SIG-1`,
`SIG-3`) must invert these five assertions in the two `over_cap_*`
tests:

| Assertion | Today | After Block 1A |
|---|---|---|
| `owner_import_route_witness_for_tests` | `is_none()` | `is_some()` |
| `candidate_signatures_for_key(owner)` | `is_empty()` | non-empty |
| warm-pass `bundle_cold_flight_runs` delta | `== first_flights` (5) | `== 0` |
| warm-pass `component_meta_result_cache_hits` delta | `== 0` | `== 1` |
| `derived_raw_cache().cached_resolved_meta.len()` | `0` | non-zero |

The two `below_cap_*` controls already assert the post-change profile and
must stay green throughout — they are what proves the target profile is
reachable, so a change that reddens a control has broken the control's
owner, not satisfied the criterion.

`verter_session::non_cacheable_bundle_reuse_tests` — Block 3B/3C (`RM-1`,
`RM-2`) must invert the per-touch flight delta from `1` on every touch to
`1` on the first and `0` on the rest, while the per-touch non-cacheable
tracer assertions stay EXACTLY as written. A change that reddens those
tracer assertions is a taint-laundering regression, not progress.

Both fixture modules carry a plant-verified mutation recipe in their module
docs; re-run the recipe after inverting to confirm the fixture still
discriminates against the post-change tree.

---

## Block 1A-i outcomes

### ADOPT-NOW — `FileSourceEnv` is its own `SourceEnv` domain, against B2's table

Recorded as an explicit disposition rather than a silent disagreement. Part B §B2's
terminal-domain table maps `FileSourceEnv` to the **Content** domain under
`ContentGeneration`. The landed implementation maps it to its own **SourceEnv**
domain under a separate `source_env_generation` counter. The implementation is
correct and the plan's table is wrong; the MANDATORY item at the top of this
errata already required the split, and this records that it was adopted.

- **Scope change:** one additional `CompactionDomain` variant, one additional
  engine counter, and one additional producer set. No acceptance ID moves.
- **Acceptance/test:** `fact_cache::compaction_domain_tests::every_leaf_variant_maps_to_its_compaction_domain`
  asserts `FileSourceEnv -> SourceEnv` and separately asserts it is NOT `Content`;
  `each_engine_owned_compaction_domain_has_a_live_producer` asserts the counters do
  not ride each other.
- **Evidence:** reverting `compaction_domain` to B2's table (folding `FileSourceEnv`
  into `Content`) previously left the whole 711-test suite green — the
  misclassification was invisible. It now fails the mapping test. Inert while no
  non-resolution domain has a basis; a live stale-serve once those domains are armed.

### Correction — B2's precision-threshold prose is off by one

B2 says a domain "remains precise through 1,023 distinct facts" and that "its
1,024th distinct fact replaces only that domain's precise bucket". The landed
threshold is precise through **1,024**, lifting on the **1,025th**
(`count > FACT_DOMAIN_PRECISE_MAX`, with `FACT_DOMAIN_PRECISE_MAX == FACT_SIGNATURE_CAP == 1_024`).

The implementation stands and the prose is corrected here. The threshold value is
arbitrary; what is not arbitrary is that it reads with the same `>` comparison, at
the same constant, as the pre-existing signature refusal it replaces — so "the size
at which a single-domain observation set used to be refused" and "the size at which
that domain now compacts" are one boundary rather than two that can drift.

### Correction — the resolution stamp is ROOT IDENTITY, not a ledger counter

B2 describes `ResolutionGeneration` as "a new per-root family owned by
`ResolutionFactRoot`". Implemented as `AggregateStamp::ResolutionRoots { base, session }`
carrying the captured world's root ids instead.

A counter maintained at `ResolutionFactRoot::advance` / `remove` is **incomplete**:
`CapturedResolutionWorld::fact_version` routes a `ContextSelection` key to the
separate `context_versions` map instead of `ResolutionFactRoot::version`, so a
published-context replacement moves the context version while the ledger's mutators
are never called. Such a counter reads unchanged across it, and a resolution-compacted
signature would keep validating against a world whose selected context had been
replaced. Root ids advance on base and session publication — the boundary
`replace_published` crosses — so they cover the whole domain, `ContextSelection`
included. Pinned by
`resolution_stamp_moves_on_context_replacement_the_ledger_never_sees`, whose negative
control asserts the ledger did NOT move on that path.

### Producer-coverage note

`set_default_resolve_extensions` does not carry its own source-env bump: it calls
`rebuild_and_publish`, which already advances the counter, so a second bump there
would be an unfalsifiable claim (removing it changed no observable). The two real
producer sites are `rebuild_and_publish` and `publish_snapshot`, each with its own
discriminating assertion; `publish_snapshot` is driven directly because every other
arm reaches `rebuild_and_publish`.

### Open, to establish at 1A-ii (not assumed)

- ~~**`file_language_id` producer completeness.**~~ **ESTABLISHED — see below.**
- ~~**Session-population aggregates end to end.**~~ **DISCHARGED at 1A-ii-c — see
  "Open item discharged" in the 1A-ii-c outcomes.** It was: no production path mints one
  yet, so `RequestStoreView` routing a `Session(fp)` aggregate through
  `self.base.validates` is argued, not exercised. Fail-closed, so the risk is wasted work
  rather than staleness — prove it when it is armed.

### ESTABLISHED — `file_language_id` producer completeness (SourceEnv domain)

**Verdict: the three `source_env_generation` producers over-cover `file_language_id`
today, and the coverage is VACUOUS — a runtime `FileLanguage` change is impossible in
this tree, not merely routed. The safety comes from the ABSENCE of a capability
producer, not from a structural rail, and nothing forces the future producer to bump
the counter.**

Evidence:

- `file_language_id` is a pure function of the canonical PATH.
  `FileArtifactKey::derived_file_language_id` (`file_artifact_store.rs:196`) is
  `LanguageRegistry::global().classify_static(canonical).static_resolution()`, and
  `classify_static` (`verter_language/src/registry.rs:228`) is a `ends_with` scan over
  a private row table.
- `LanguageRegistry::global()` is a `OnceLock` with no setter and no re-init
  (`registry.rs:218`); `built_in()` (`:152`) constructs only `fixed` and `carrier`
  rows. **Zero `RowClassification::Gated` rows exist in production** — `LanguageRow::gated`
  is called only from tests — so `static_resolution()`'s `Gated` arm is dead code and
  classification is total, pure and constant per path.
- The capability channel is unarmed: `host_construction.rs:264` builds the sole
  `HostLanguageClassifier` with `ProjectCapabilitySnapshot::empty()`;
  `ProjectCapabilitySnapshot::from_capabilities` has zero production call sites; the
  field is plain and every `VerterHost` method takes `&self`, so it is unreachable for
  mutation. `FrameworkAdapterRegistry` is `Arc`-held with no `&mut self` API, and
  language rows live in `LanguageRegistry` regardless.
- The one real runtime relabel channel does NOT reach the fact.
  `UpsertRequest.file_language` is caller-supplied and the host treats a byte-identical
  relabel as a change (`host_upsert.rs:590`), but it lands on `HostSourceData`, not on
  the artifact key — a documented limitation at `file_artifact_store.rs:176-186`.
- Of the three `FileSourceEnv` dimensions, only `parse_env_hash` genuinely moves:
  `parser_version` is the compile-time `CURRENT_PARSER_VERSION` (`file_artifact_store.rs:311`),
  and `file_language_id`'s comparison in `validates_file_source_env`
  (`resolver_store.rs:3086`) is a tautology today. `parse_env_hash` is exactly what the
  three bump sites cover.

**Contingency, and where the hook belongs.** Two documented follow-ups would each make
a per-file language change possible with NO content bump and NO env republication —
i.e. would silently break this domain:

1. The first `LanguageRow::gated` row plus a real `ProjectCapabilitySnapshot` producer.
   The snapshot is a host-construction-time value with no config-change path; nothing
   wires it to `ConfigChanged` / `rebuild_and_publish` / `publish_snapshot`.
2. The live-row threading TODO at `file_artifact_store.rs:176-196` (threading
   `HostSourceData.file_language` into key construction). That arrives on the UPSERT
   path, which is none of the three sites, and a byte-identical relabel moves no content.

`compaction_domain`'s exhaustive match is a compile rail for assigning a domain to a new
fact VARIANT; it is not a rail for bumping a counter when an existing variant's PAYLOAD
moves. If the invariant is to be held structurally rather than by absence, the bump
belongs where `file_language_id` enters a key — `FileArtifactKey::base`
(`file_artifact_store.rs:157`) and `FileArtifactKey::overlay_scoped` (`:233`) — not at
the compaction boundary. Recorded here so whoever arms either follow-up inherits the
obligation.

### ESTABLISHED — SemanticImports / RouteSurface producer inventory

The second item the 1A-ii brief said to establish rather than assume. Five findings,
two of which change the design.

**A. SemanticImports has one clean write chokepoint and NO production removal.**
Every write funnels `ResolvedImportFactsDb::admit` (`resolved_import_facts.rs:347`) ←
`admit_resolved_import_facts_for_owner` (`resolved_import_facts_producer.rs:110`) ←
`set_import_dependencies` (`host_manage/analysis_io.rs:2277`), whose only production
entries are external (`verter_lsp` ×5, `verter_napi:1915`, `verter_wasm:599`). There is
no per-canonical invalidation, no generation eviction, and no clear: the db is absent
from `PROJECT_TYPE_STORE_DB_INVENTORY` and from `all_dbs_for_invalidation`, and
`ResolvedImportFactsDb::clear` has no production caller. The store is APPEND-ONLY,
bounded only by the per-slot FIFO at `resolver_core/mod.rs:1545-1553`
(`CANDIDATE_CAP = 4`) — which is itself a validity-flipping event for a fact whose
candidate ages out. Key dimensions move via `upsert` (`content_hash`),
`configure_projects` / `set_workspace` (`parse_env_hash` / `resolve_env_hash`);
`RESOLVED_IMPORT_FACTS_RESOLVER_VERSION` is a compile-time const.

**B. RouteSurface arm 2 (`EffectiveExportSet`) is DEAD in production.** The only
insert is `#[cfg(test)]` (`route_db/effective_export_set.rs:147`), the module header
says it "deliberately exposes no cold compute/publish funnel" (`:6-7`), and every
`FactKey::EffectiveExportSet` construction site is under `tests/`. So
`lookup_effective_export_set_fingerprint` always returns `None` and the arm returns
`false` unconditionally. **Latent defect recorded, not fixed here:** `session_id` is
`None` at every production `build_coherent` call site, so this arm's scope is always
`Base` even for a session-overlaid view — a producer/validator scope mismatch against
arm 1, which uses the overlay-set fingerprint (`resolver_store.rs:2272-2280`).
Unobservable while there is no writer; a real bug the moment one is added.

**C. RouteSurface arm 1 has two clean chokepoints.** Publication:
`install_augmenter_set` (`file_artifact_store.rs:3141`, module-private, reachable only
via `ensure_augmentation_index_populated` `:3328` and `populate_augmenter_set` `:3224`).
Retirement: `retire_augmenter_keys` (`:3194`), via
`invalidate_augmentation_index_at_epoch` (`:3491`) and `clear_augmentation_index`
(`:3521`); artifact-driven retirement additionally funnels the single removal
chokepoint `retire_artifact_keys` (`:2110` → `:2186`). Unlike SemanticImports, this
index IS epoch-versioned relative to the captured `FileArtifactRoot`.

**D. THE FINDING THAT CHANGES THE DESIGN: RouteSurface mutates INSIDE an active fact
tracer.** Three production sites, all same-thread reentrancy, not cross-thread:

1. `project_semantic_dispatch/build.rs:3087` — `ensure_augmentation_index_populated`
   inside `collect_augmentation_contributions`, which takes
   `host_for_fact_tracer_install()` (`build.rs:3059`) and whose own doc says it
   observes the augmenter-set fingerprint onto the ACTIVE tracer. Enclosing scopes:
   `project_semantic_dispatch/mod.rs:1773`, `:2292`, `semantic_query_memo/mod.rs:1592`.
   It installs a new `AugmenterSet` under a fresh epoch and can bump
   `artifact_generation` (`file_artifact_store.rs:3414-3416`).
2. `build.rs:3328` — the self-heal `populate_augmenter_set` re-publication in the same
   scope. Fingerprint unchanged, so no `artifact_generation` bump, but
   `install_augmenter_set` still reserves an epoch and retires the prior version,
   moving the entry's `birth` forward relative to already-captured roots.
3. Artifact publish → augmentation invalidation: `prepared_decl.rs:2165-2167`
   (`FileArtifactStore::insert` → `retire_artifact_keys` → `:2186`, plus publish-side
   `:2582`), reached from inside traced computes at `build.rs:3110-3113` and
   `raise.rs:2172-2175`.

So a producer running inside a `FactReadSet` scope mutates the very index the fact it
is about to record validates against. SemanticImports shows no such same-thread
reentrancy (its chokepoint is a top-level host API).

**Design consequence — the install-time snapshot is mandatory, not defensive.**
Reading a domain's stamp at FINALISE time is a stale-serve: the aggregate would assert
"the domain held as of G+1" while standing in for facts read at G. So the stamp is
snapshotted when the tracer is INSTALLED, and observations are never post-stamped with
a generation they predate.

> **CORRECTED — the original drop-and-stay-precise rule was UNSOUND and is withdrawn.**
> This entry previously said: "before `finalise`, re-read and DROP from the basis any
> domain whose stamp moved — that domain does not compact, its bucket stays precise,
> and the existing size gate handles it." That is wrong and must not be implemented.
> Letting a mutation-unstable attempt fall through to the size gate converts a
> STABILITY failure into a CARDINALITY failure: the attempt is refused for the wrong
> reason, under a rail `SIG-1` exists to remove, and the caller cannot tell the two
> apart.
>
> **The correct rule** (AMENDED — see "Architecture ruling — `MutationUnstable` is
> terminal (Rule B)" below; the previous wording said the attempt "is RETRIED", which
> the ruling replaced because it cannot truthfully describe zero retries):
>
> > If any domain's generation moves between tracer installation and finalization, the
> > whole attempt is `MutationUnstable` and MUST NOT be admitted. For this cutover,
> > `MutationUnstable` is terminal on the first unstable attempt; no automatic retry is
> > performed. It must never surface as signature overflow or any other cardinality
> > failure. A future retry must be bounded and owned by a fresh-attempt boundary that
> > recreates attempt state and defines budget semantics.
>
> **A tracer-local "this trace caused the bump, so ignore it" exception is UNSOUND.**
> Do not build one. A trace cannot exempt itself from a generation it moved.
>
> Correctness bar: **zero genuine advances between tracer installation and finalization
> for any ADMITTED attempt.**

> **SUPERSEDED (Block 1A-ii recon).** The next sentence placed the check in
> `install_fact_tracer`. That seam does not hold — see "BLOCKED — the retry protocol
> cannot be installed in `install_fact_tracer`" below, whose third reason (the helper is
> not the signature-consuming chokepoint) applies to ANY check installed there, movement
> detection included.

The check belongs in `install_fact_tracer` (`fact_signature_helpers.rs:100`), not in
`FactReadSet`, which keeps the neutral value-only bridge intact.

**E. Unresolved, flagged not concluded.** The SemanticImports producer keys on the
LIVE per-canonical env (`host_view_env_hashes_for`, `host_construction.rs:754`) while
the validator keys on the view-captured WORKSPACE-level
`project_env_root.env_hashes.parse_env_hash` (`resolver_store.rs:2670-2675`) rather than
the per-canonical `parse_env_hash_for` (`store_view_roots.rs:322`). In a multi-project
workspace whose owning project differs from the workspace default, producer and
validator would compose different keys. No test pins either behaviour. This predates
the compaction work and is not caused by it; recorded so it is dispositioned rather
than absorbed silently.

### MEASURED — RouteSurface in-tracer mutation frequency, and FIFO aging

Load-bearing evidence for the "arm RouteSurface or not" ruling. Produced with
throwaway atomics plus a per-thread tracer-depth/tick counter wired into
`with_fact_tracer_cell` (scope enter/exit) and each of the three mutation sites;
instrumentation reverted, zero residue, both suites re-run green afterwards.

**Workload, stated honestly:** the full `verter_session --lib` suite — 60,000 tracer
scopes, in-process, over varied real fixtures. Representative of cold behaviour across
many short-lived hosts. It is NOT the nuxt-ui corpus, and it is NOT a long-lived LSP
session.

| Observable | Count | Share |
|---|---|---|
| Tracer scopes total | 60,000 | denominator |
| Scopes with a route mutation on the same thread | 3,471 | **5.8%** |
| Site 1 `ensure_augmentation_index_populated` — calls | 4,838 | |
| — warm hits (no mutation) | 3,467 | 71.7% |
| — cold installs | 1,371 | 28.3% |
| — cold, in tracer | 1,328 | 96.9% of colds |
| — bumped the generation, in tracer | 1,328 | |
| Site 2 self-heal `populate_augmenter_set` — calls | 7 | **0 in tracer** |
| Site 3 artifact-publish invalidation — calls | 10 | 7 retired, 3 in tracer |
| FIFO admissions | 9,851 | |
| FIFO evictions | 568 | **5.8% of admissions** |

**Conclusions.**

1. ~~**RouteSurface is NOT effectively disarmed.** 94.2% of tracer scopes see no route
   mutation, so the domain compacts normally in the overwhelming majority of cases.~~
   **OVER-CLAIMED — scoped down.** What was actually measured: 5.8% of ALL tracer
   scopes saw an `artifact_generation`-class mutation. That is a SUPERSET of the
   population that matters, which is attempts whose RouteSurface bucket EXCEEDS the
   per-domain threshold — the only attempts that would compact at all. It also counts
   first-time index materialization, which the corrected RouteSurface clock (below)
   must not count as a semantic advance. So "94.2% compact normally" does not follow
   from this data and is withdrawn. The narrower hypothesis it was raised against —
   that RouteSurface "might almost never be mintable" — is not supported by this
   measurement either, in either direction. It remains unmeasured whether production
   generates any RouteSurface bucket over the threshold at all.
2. **Site 1 dominates completely** — 4,838 calls against 7 and 10. Site 2 never fired
   in-tracer at all, despite the static reading placing it inside the same scope. Any
   narrowing, if one is ever wanted, is a site-1 change; sites 2 and 3 need nothing.
3. **5.8% is an UPPER BOUND.** Site 1's cost is first-touch-per-key, not per-compute:
   71.7% of calls already warm-hit and the ratio was still climbing across the run.
   This workload is many short-lived hosts each starting cold; a long-lived session
   warms far more of the index, so the real in-session rate is lower.
4. **Nesting property, found incidentally and not previously stated anywhere:** 1,328
   mutation events marked 3,471 scopes — **≈2.6 scopes per event**. An inner mutation
   destabilises EVERY enclosing scope on the tracer stack, not just the innermost one,
   so the retry protocol's cost is superlinear in nesting depth: one inner advance
   forces the whole enclosing stack to retry. This survives the conclusion-scoping
   above unchanged — it is a property of the mechanism, not an inference from the
   rate.
5. ~~**FIFO aging must be a first-class mutation event for SemanticImports.**~~
   **PREMISE FALSE — see the stamp ruling below.** Eviction happens INSIDE the same
   insertion transaction (the oldest candidate is drained and the new one pushed in one
   `rcu`), so there is no admit-free validity flip and no second validity dimension is
   needed. The rate below is retained as telemetry, not as a stamp requirement.
   5.8% of admissions evict. For a store with a single write chokepoint and NO
   production removal, eviction is the only bound — and it is the only interesting
   failure mode, since it flips validity for any view whose recorded fact set matched
   the aged-out candidate. It gets its own discriminating plant in the producer audit,
   never folded into the `admit` chokepoint test.

**Not measured:** the `parse_env_hash` asymmetry. The probe sat on
`validates_resolve_imports_domain_for_content_hash`, which this workload reaches once —
one sample, zero divergent. That is not a measurement and is not reported as one; the
asymmetry's disposition stays open pending the targeted multi-project fixture.

### REJECT — the `parse_env_hash` key asymmetry is structurally unreachable

Disposition for the pre-existing producer/validator asymmetry flagged in the
SemanticImports inventory. **REJECT**, with the mechanism that prevents it.

The asymmetry is real as written: the producer keys per-canonical
(`host_view_env_hashes_for` — owning project's env array, else workspace default)
while the validator composes its `ResolvedImportFactsKey` from the view-captured
workspace-level `project_env_root.env_hashes.parse_env_hash`, NOT from the
per-canonical `ProjectEnvRoot::parse_env_hash_for`, which exists and mirrors the
producer exactly.

It cannot diverge because **`parse_env_hash` does not depend on the project at all**.
`IdeProjectConfig::parse_env_hash` folds exactly a constant salt plus
`EnvHashInputs::parser_flags` and never reads `&self`; both `compose_env_hash_tables`
and `compose_env_hash_tables_from_configs` build `EnvHashInputs` from the
workspace-wide `WORKSPACE_PARSER_FLAGS` and pass the same `inputs` to every project.
So every project's `parse_env_hash` is byte-identical to every other project's and to
the workspace default. Confirmed empirically on a two-project workspace: both owning
projects and the unowned/default path produce the identical hash.

It is therefore a READABILITY defect at most — the validator reads a field that
happens to equal the value it should conceptually be reading. No correctness change
is made.

**Not left as prose.** `parse_env_hash` is the ONLY one of the four env dimensions
with this property (`resolve_env_hash` folds `base_url`, `paths`, aliases and
references — all per-project), so a future change that folds per-project state into
`parse_env_hash` would look entirely reasonable in isolation and would silently turn
the asymmetry into a live cache-miss bug: every admitted resolved-import bundle for a
non-default project would become unfindable. The permanent fixture
`verter_session::…::parse_env_asymmetry_tests` pins the MECHANISM and fails at exactly
that moment, naming the consequence and the fix (route the validator through
`ProjectEnvRoot::parse_env_hash_for`). Plant-verified: appending `self.root` to the
hash buffer makes the two projects diverge and the test red.

### REJECT — a capability flip cannot move an artifact key's `file_language_id`

Disposition for the third "establish, do not assume" item. **REJECT**, structurally
impossible — and the mechanism is NOT the one the earlier trace identified.

The producer is `FileArtifactKey::derived_file_language_id` =
`LanguageRegistry::global().classify_static(path).static_resolution()`. And
`StaticClassification::static_resolution` maps `Gated(candidate) => candidate.fallback`:
it deliberately DISCARDS the capability dimension, because consumers below the host
seam never see project-gated candidates. So the key's `file_language_id` is
capability-independent **by construction, regardless of registry contents** — adding
the first `Gated` row tomorrow would not change it, because the producer takes the
ungated fallback either way.

That is strictly stronger than "no gated rows exist in production". The absence of
gated rows is also true today (`built_in()` registers only `fixed`/`carrier` rows) and
so is the absence of a capability producer (`ProjectCapabilitySnapshot` is built empty
at host construction, `from_capabilities` has zero production call sites) — but those
are contingent facts. The `static_resolution` arm is a structural one.

**SourceEnv's producer set is therefore COMPLETE as it stands.** Of its three claimed
dimensions: `parse_env_hash` moves only on env-table republication, which the three
producers cover; `parser_version` is a compile-time constant; `file_language_id`
cannot move at runtime at all.

**The consequence worth recording.** Because the producer discards capabilities, the
per-file classification column does not currently track a capability flip — the
machinery described in CLAUDE.md ("a framework-capability flip misses exactly the
affected files' artifact slots") is built but unarmed. SourceEnv stops being complete
the moment `derived_file_language_id` is routed through the host-level
`HostLanguageClassifier` instead of `static_resolution`, which is precisely the change
that would arm it. At that point SourceEnv needs a producer on the capability-flip
path.

Pinned permanently by `verter_session::…::file_language_capability_tests`, in the same
mechanism-not-conclusion style as the `parse_env_hash` guard: it asserts the
`Gated => fallback` arm directly and names the consequence in the failure message.
Plant-verified — changing that arm to return `candidate.candidate` turns it red.

---

## Architecture ruling — RouteSurface stays; SIG-1 unchanged

**PRESERVE SIG-1 for all six domains. ADOPT-NOW a corrected Option 2.** RouteSurface
remains the sixth compaction domain. Its semantic generation is SEPARATE from
`artifact_generation`, and a genuinely mutation-unstable trace is TERMINALLY REFUSED as
`MutationUnstable` on its first unstable attempt (see the corrected stability rule above,
as amended by Rule B — no automatic retry), never degraded into a cardinality refusal.

### RouteSurface clock semantics

`RouteSurfaceGeneration` advances for changes to the semantic augmentation WORLD, not
for every index or cache mutation:

| Event | Bump? |
|---|---|
| First-time materialization of an index row from an unchanged artifact corpus | **no** |
| Same-fingerprint self-heal re-publication | **no** |
| Cache-only clearing / repopulation | **no** |
| Artifact publication or retirement that may change augmentation contributors or their target set | **yes** |
| Project / env / population changes altering visible augmentation semantics | yes, or covered by the aggregate's other typed dimensions |

The same-fingerprint self-heal is a no-bump because an older captured root still
resolves the retired same-fingerprint version through the version chain
(`file_artifact_store.rs:1592`) — birth-epoch movement alone is not a semantic validity
flip. The 1A-i measurement agrees: site 2 never fired in-tracer.

**Do NOT copy `artifact_generation` wholesale.** It is deliberately excluded from
external-supersession promotion precisely because cold computations publish their own
artifacts (`resolver_store.rs:567`); a RouteSurface clock that inherited that shape
would bump on its own consumers' cold work.

### SemanticImports stamp — a single monotonic generation, and the FIFO premise was false

One monotonic `SemanticImportsGeneration` suffices; there is **no separate eviction
stamp**, because eviction is not an admit-free validity flip: the oldest candidate is
drained and the new one pushed inside the SAME insertion transaction
(`resolver_core/mod.rs:1541`). The earlier claim that eviction needs its own
discriminating dimension is WITHDRAWN.

- Advance once per successful membership-changing `admit`, whether or not it evicts.
- Do NOT advance for refused admissions or genuine identical-candidate skips.
- Any future `clear` / removal / GC must also advance.
- Keep eviction telemetry and tests — as telemetry, not as a validity dimension.
- Preserve the composite stamp.

**Concurrency caveat:** a naïve post-insert atomic increment is insufficient — it opens
a mutation/stamp gap. Mutation publication and generation advancement need a serialized
or seqlock-style protocol, with install/finalize accepting only the same stable
generation.

**Recorded, not fixed here:** the per-slot FIFO caps candidates PER KEY, not total keys,
so distinct-key growth is unbounded. That is a memory-lifecycle concern independent of
stamp correctness.

### Four ADOPT-NOW fixes, all landing BEFORE any counter

1. **Delete the dead `EffectiveExportSet` surface.** No production publisher
   (`route_db.rs:10`); sole insert is `#[cfg(test)]` (`effective_export_set.rs:144`).
   Remove/update the exhaustive fact mappings and the RC-4 tests. RouteSurface stays a
   ONE-ARM domain — that is legitimate, and the dead sibling does not justify folding
   `ModuleAugmentationIndexShape` elsewhere. *Correction to the 1A-ii inventory above:
   the production validator is not literally `false`; it performs a lookup
   (`resolver_store.rs:3283`) that is operationally always absent.*
2. **Fix the `resolve_env_hash` asymmetry — the LIVE sibling of the rejected
   `parse_env_hash` one.** The validator takes `resolve_env_hash` from the workspace
   default (`resolver_store.rs:2665`); the producer uses the owning project's full env
   bundle (`resolved_import_facts_producer.rs:140`). Unlike parse, resolve hashes fold
   project paths, aliases, compiler options and references (`env_hash.rs:145`), so this
   one is REACHABLE. Fix via a shared captured per-canonical env-key composer, before
   the SemanticImports counter. No runtime reproduction exists yet — only source
   dataflow establishing reachability — so a targeted fixture pinning it is worth
   having.
3. **Observe NEGATIVE RouteSurface facts.** The collector returns before observing
   `ModuleAugmentationIndexShape` when the installed set is empty (`build.rs:3092`) and
   again when no contributor supplies the requested declaration (`build.rs:3337`); the
   shape fact is emitted only afterward. So a cacheable "no augmentation" result lacks
   the fact needed to reject it when an augmenter later appears or gains that
   declaration. Observe the empty / non-contributing set fingerprint BEFORE both
   returns.
4. **Make SemanticImports dedupe atomic and candidate-correlated.** It checks whether
   ANY candidate holds the witness, then separately compares the LAST candidate's
   payload (`resolved_import_facts_producer.rs:300`) — not necessarily the same
   candidate, and the slot can change between the two queries. Replace with one slot
   snapshot / predicate over a single `(witness, payload)` candidate, before any
   counter instrumentation.

### Sizing — 1A-ii-pre adopted

The four ADOPT-NOW fixes land as **1A-ii-pre**, their own gated unit. Counters,
providers, validators, population rules and the retry protocol follow as 1A-ii. Each
fix is a correctness change to live code with its own observable, so bundling them with
the counters would blur which change established which property — the same argument
that produced the 1A-i/ii/iii split.

---

## Forward-audit rulings — three plan statements are FALSE

### The 1A ordering is redrawn; deleting overflow early would pass SIG-1 vacuously

`fact_read_set.rs` deliberately refuses to mint an aggregate for a domain with no live
producer, and the ONLY production `set_aggregate_basis` call supplies the single
resolution domain from `ResolutionTransaction`. **Every session tracer installer begins
with all six bases absent.** So deleting overflow before the domains are armed would
admit arbitrarily large UNCOMPACTED signatures: SIG-1 would go green while establishing
nothing, because nothing would have been compacted.

Binding order:

1. **1A-ii** — live producers, validators, effective population, and MOVEMENT DETECTION
   for **all six domains**, not only the session-side three.
2. **1A-iii** — one coherent install-time basis supplied to **every** session tracer and
   cacheability scope, proving base/session minting, validation and advancement for all
   six.
3. **1B** — only then delete overflow, `ReturnOnly`-for-size, and the downstream refusal
   graph.

A partial-basis intermediate may land internally, but **an unarmed domain must retain
the legacy cap until armed. There must never be an intermediate state in which a domain
is both uncompacted and unbounded.**

**Population translation moves into 1A**, earlier than the plan's Block 2B.
`aggregate_population` currently defaults the non-resolution domains to `Base`; effective
`Base` vs `Session(fp)` translation is part of basis installation, including the
base-to-session propagation seeds.

### "The compiler is the completeness oracle" is FALSE — three size gates produce no compile error

Part B §B2 says deleting the `Overflow` variant and carrier field "will make the
remaining direct and indirect consumers compile errors; the compiler is the completeness
oracle", and Block 1B repeats it. Verified false in three places:

1. **`FactReadSet::would_overflow()`** — a live size refusal that deleting `Overflow`
   does not remove. Three production readers fold it into a `bool`
   (`fact_signature_helpers.rs:204`, `:366`, `:394`), feeding **17**
   `with_cacheability_scope` / `named_cacheability_scope!` sites — including
   `prepared_decl.rs:483`, directly above the overlay memo gate. None compile-errors.
2. **A second `ValidatedFactCache` size gate** at `resolver_core/mod.rs:1570`, beyond
   the `:1494` the plan cites.
3. **`import_route_witness.rs:197-204`** — a THIRD size gate, on the exact path SIG-1's
   own fixture measures. The witness is a bare `Vec<FactVersionRef>` that never passes
   through a `FactReadSet`, so the 1A-i `compact_domains` pass is **inert** there. This
   matters directly for the inversion table recorded above: making
   `owner_import_route_witness_for_tests` flip `is_none()` → `is_some()` requires
   removing THIS gate, not the `FactReadSet` one — the mirror resolves straight through
   it to `decline_import_route_witness()`.

Consequence: 1B needs an explicit, enumerated migration list for the size gates. The
compiler will not produce one.

### Test-force knobs are `#[cfg(test)]`, not `test-support`

`TestForceKnobs` is gated `#[cfg(test)]` (`host_test_force.rs:16-18`, module doc: "so
they compile to nothing"), NOT `feature = "test-support"`. So any test needing a fence
or overflow force must live **in-crate** under `crates/verter_session/src/**` and can
never live in `crates/verter_session/tests/cases/`. This constrains 1B's restaging of
`non_cacheable_materialization_is_not_memoized` and Blocks 3B/3C. The plan says nothing
about it. (The 1A-i taint fixture already complies — it is an in-crate module — but by
accident of where it was placed, not by design.)

### Block 5A is a PRODUCTION-caller migration, not a test migration

Part B's ruling section states "Bare-host removal is a test migration, not a
production-caller migration", and §B7 repeats "This is pure test migration". **False.**
Avatar was reproduced with a full backtrace: `capture_component_meta_inputs` → raw
analysis snapshot → template enrichment → `build_template_class_semantic_facts` →
bare-host `prepared_value_decl` → the release-only panic. The binding is
`analysis_io.rs:68`, whose indexed-present branch passes `self: &VerterHost` into the
resolver-tier builder.

It is **cache-presence-dependent**, which is exactly why a binder documented as a test
bridge survived a call-site census — the production path only takes it when the artifact
is already indexed. This also supersedes the 1A-i baseline's framing of the Avatar crash
as "one of the nine bare-host panic arms (Block 5A)" pending Block 5C diagnosis: the
diagnosis exists now, and 5A owns a production migration.

### Recorded for later blocks, not yet briefed

- **Q4:** the reuse rail generalizes the EXISTING admission carriers rather than minting
  a third vocabulary — `ComputeAdmission` already models the same three states across 51
  production sites.
- **Q10:** `Decision(Q)` is the caller-visible witness on BOTH cold and warm paths, so
  deleting `absorb` is a cold-path semantic cutover, not a warm-path optimisation.
- **Q3:** the context memo belongs on `PublishedRoot`, not `ResolutionWorldRoot`.

---

## 1A-ii-pre outcomes

### CORRECTION — the `resolve_env_hash` asymmetry is LATENT, not a live regression

The ADOPT-NOW fix landed, but the consequence must be stated at the strength of the
evidence. Two claims are separated:

- **Measured (store level):** on a two-project workspace whose project `A` carries a
  workspace alias plus a `baseUrl`, the producer admits under `A`'s per-canonical
  `resolve_env_hash` and a lookup composed from the workspace default finds NOTHING.
  The key divergence is real and reproducible.
- **NOT measured, because it does not exist today:** any consumer-visible effect. The
  `ResolveImportsFactRef::Semantic` arm this validator serves is **production-dead**.
  Every `Semantic` CONSTRUCTION in the tree is inside a test module or test file; the
  only production construction of `FactVersionRef::ResolveImports(..)` is the
  `Resolution` arm, which never composes this key; and
  `SessionView::resolved_import_facts` has zero production callers.

So the divergence causes neither staleness nor wasted work at present. It is repaired
now, while the arm is inert, so the first production consumer to record one of these
facts does not inherit a silent whole-slot miss for every non-default project. A
characterisation of this fix as a measured production regression is wrong.

**Scope note that generalises.** "Reproduce before you fix" was satisfied by reproducing
the MECHANISM. Reproducing the CONSEQUENCE is a separate, stronger obligation, and the
two must not be conflated in a landing claim.

### Fix scope — the composer covers BOTH readers of the slot

The asymmetry existed at two readers, not one. Besides
`validates_resolve_imports_domain_for_content_hash`, the shared
`session_view::resolved_import_facts_for_view` composed the same key inline from the
view's `env_hashes`, which all four `SessionView` constructors set to the
workspace-level bundle. Both now route through
`HostStoreView::resolved_import_facts_key_for`, and the `SessionView::resolved_import_facts`
trait doc no longer states the workspace-level composition as contract.

### ~~DEFER~~ ADOPT-NOW — the `ZERO_HASH` permissive branch on the resolve-imports validator

> **SUPERSEDED — the ruling landed and it is ADOPT-NOW, not DEFER.** The row below is
> retained as the finding's provenance; its disposition is replaced by "Ruling — `ZH-1`
> is ADOPT-NOW and reopens 1A-ii-pre" immediately after it. The `DEFER` framing, the
> "semantics genuinely undecided" conclusion and the OWED ruling reference are all
> discharged.

`resolver_store.rs` → `validates_resolve_imports_domain_for_content_hash` ends its
slot-miss arm with `None => return *expected_hash == ZERO_HASH`: a miss ACCEPTS when the
consumer recorded a zero hash. No production emitter mints a zero-hash Semantic fact
today, so the branch is unreachable — but the only in-repo precedent for minting one is
the Parse-domain `None => zero_hash()` at `fact_signature_helpers.rs:775`, and writing
the Semantic emitter that obvious way would turn a slot miss on a TRACKED file into a
stale ACCEPT.

**No second opinion exists in-tree.** An earlier draft of this entry claimed
`CapturedResolutionWorld::validates_resolve_imports_fact`
(`verter_workspace/src/resolution_currency.rs:943`) "disagrees" by returning `false` for
the same arm. That is WRONG and is corrected here: that `false` rejects the entire
`Semantic` VARIANT on ownership grounds — "a `Semantic` fact belongs to the session's own
resolve-imports producer and is never this world's to validate" — a world-scope authority
rejection, not an answer to the slot-miss question. The zero-hash arm therefore has
exactly ONE decision site and no in-tree precedent arguing the other way, which makes the
semantics genuinely undecided rather than merely inconsistent.

**Disposition: DEFER.** Not fixed in 1A-ii-pre — it is a semantics decision about what a
zero-hash observation MEANS (genuine "absent" assertion vs sentinel), it is unreachable
today, and changing it without the emitter it is meant to serve would be speculative.

- **Acceptance ID:** `ZH-1`.
- **Owner block:** 1A-ii, which arms the producers/validators and is where a zero-hash
  emitter would first become possible.
- **Resolution gate:** no later than 1A-ii close.
- **Planned test:** `zero_hash_semantic_fact_is_rejected_on_a_slot_miss_for_a_tracked_file`
  — admit nothing for a TRACKED canonical, record a Semantic `ResolveImports` fact whose
  `expected_hash` is `ZERO_HASH`, and assert `StoreView::validates` REJECTS it. Fails
  against the current permissive arm; passes once the arm is decided as `false`. If
  1A-ii instead decides zero-hash means a genuine "absent" assertion, the test inverts to
  assert acceptance ONLY for an UNTRACKED canonical and rejection for a tracked one — the
  discriminating pair either way.
- **Ruling reference: OWED, NOT OBTAINED.** The repo's disposition rule requires a
  codex-DEFER ruling for a `DEFER`, and none has been sought — this agent does not
  dispatch review legs. Until that ruling is recorded this row is INCOMPLETE by the
  rule's own terms and must not be read as a discharged deferral. Obtaining it is a
  precondition of 1A-ii close, alongside the test above.
- **Obligation inherited by whoever writes the first Semantic zero-hash emitter:** decide
  this arm in the same change.

### Ruling — `ZH-1` is ADOPT-NOW and reopens 1A-ii-pre

The ruling came back **ADOPT-NOW**. It supersedes the `DEFER` row above, discharges the
OWED ruling reference, and REOPENS Block 1A-ii-pre — so it lands BEFORE the
SemanticImports producers and validators are armed, i.e. before 1A-ii-d.

**The change.** `resolver_store.rs` →
`validates_resolve_imports_domain_for_content_hash`, the slot-miss arm, becomes:

```rust
None => return false,
```

**The reasoning is stronger than the concern that raised it.** The deferral rested on
"what does a zero hash MEAN", which read as genuinely undecided. It is not, because the
question the arm actually answers is different: a missing whole `ResolvedImportFacts`
slot is **absence of evidence**, not evidence that a particular Semantic fact is absent.
That holds under EITHER reading of zero — even if a future zero hash is defined as a
genuine "absent" assertion, such an assertion needs a current authoritative slot or an
explicit negative carrier to stand on. Two further facts settle it:

- The validator's **own documentation already says "Cache slot absent for the composed
  key → reject. The cache was the recording site; absence means the consumer observed a
  stale slice."** The implementation accepted zero anyway — the doc and the code
  disagreed, and the doc was right.
- Unresolved imports already carry the explicit `UNRESOLVED_SENTINEL` fact with a real
  semantic hash, so zero is NOT needed as the negative-resolution rail. Nothing is lost
  by rejecting it.

**Discharge conditions** (all three required):

1. The planned test —
   `zero_hash_semantic_fact_is_rejected_on_a_slot_miss_for_a_tracked_file` — admits
   nothing for a TRACKED canonical, records a Semantic `ResolveImports` fact whose
   `expected_hash` is `ZERO_HASH`, and asserts `StoreView::validates` REJECTS it.
2. Its mutation recipe restores the permissive arm
   (`None => return *expected_hash == ZERO_HASH`) and is EXECUTED against the landed
   tree to prove the test goes red.
3. A positive control shows a present candidate with a matching real Semantic fact still
   validates — so the fix is a narrowing of the miss arm, not a blanket rejection.

**Explicitly NOT decided by this fix:** the UNTRACKED-canonical zero behaviour (the
separate `whole_hash(..) == None` arm in `validates_resolve_imports_domain`, whose
optimistic accept is the R26 untracked-file window). Do not extend into it.

### Architecture ruling — `MutationUnstable` is terminal (Rule B)

The fork left open by the retry-seam STOP is ruled: **Rule B — `MutationUnstable` is
terminal on the first unstable attempt. No automatic retry is performed.** The amended
rule text is folded into "CORRECTED — the original drop-and-stay-precise rule was
UNSOUND" above; this section records what the ruling adds beyond that wording.

**"Bound = zero" was REJECTED as framing.** The recommendation put to the consult was
the ratified rule with the retry bound set to zero. That is a **deviation requiring an
errata amendment**, not an application of the existing rule: "the whole attempt is
RETRIED" cannot truthfully describe zero retries. The amendment is made rather than the
deviation absorbed silently.

**B is not implemented by changing a constant.** Universal movement detection and typed
refusal must still cover EVERY admission boundary. Recorded as a new acceptance ID:

- **`MU-1`** — every admission-owning tracer or cacheability scope detects relevant
  generation movement; instability produces a DISTINCT typed `MutationUnstable`, admits
  and publishes nothing, and never maps to overflow. Owner: 1A-ii / 1A-iii.

**There is no `MutationUnstable` arm today.** Neither `verter_audit::NonAdmissionReason`
nor `FactReadSetFinalise` carries one. B needs a NEW typed carrier; folding it into an
existing refusal is forbidden. Truthful causality is one of the three properties this
rule protects — the others being (a) no admission on observations paired with a
superseded stamp, and (b) no retry-induced semantic degradation.

**NEW FINDING — `CacheabilityProbe::non_cacheable()` must participate.** It is Boolean
only today, and it **can authorise writes inside its closure**. An exit-only
finalization check would therefore run too late for those writes. This lands in the
composite-stamp units (1A-ii-d / 1A-ii-e) rather than in a finalization-only check.

**`MU-R1` — the rate metric, owner Block 6.** Measure

```text
terminal MutationUnstable attempts
────────────────────────────────────────────────────────────────────────
otherwise-complete, otherwise-cacheable attempts whose final signature
would actually compact at least one domain
```

counting **logical attempts**, not nested tracer scopes. Threshold **1.0%**; at or above
it, Block 6 cannot close until a bounded retry exists at an attempt-owning seam. The
existing 5.8% all-scope figure measured in "MEASURED — RouteSurface in-tracer mutation
frequency" **cannot** answer this metric: its denominator is every tracer scope, not
attempts that would compact, and its numerator counts scopes rather than logical
attempts.

### Corrections — the retry-seam block's supporting claims were partly overstated

The BLOCKED conclusion stands and the seam is still disqualified. Three of its supporting
claims were wrong or imprecise, and the record is corrected rather than left to be
inherited as fact.

1. **The ref-cycle overflow example is WRONG as stated.** The claim was that re-running
   `component_meta_caches.rs`'s traced closure duplicates fence rows and self-roots into
   the entry's fact carrier, inflating it toward `FACT_SIGNATURE_CAP` and manufacturing a
   SPURIOUS OVERFLOW. It does not follow: duplicated self-roots are collapsed by
   `ref_cycle_read_set`, each retry would carry a fresh tracer whose facts are
   canonicalised and deduplicated, and `compute_fence` is not fed into that ref-cycle
   fact carrier at all. What survives: the accumulators DO append, the duplicated
   dispatch fence IS incorrect state, and the hard `FnOnce` bound
   (`C: FnOnce(&mut Vec<…>, &mut Vec<…>) -> bool`) remains independently disqualifying.
2. **The budgets are NOT sticky for the whole request.** `ConnectedDemandState::begin`
   RESETS `work_used` and `tripped` at each outer connected-demand root, so the
   "never-reset counter … sticky for the remainder of the request" description was
   wrong. The conclusion is unaffected: they are not reset between retry ATTEMPTS inside
   that root, so a retry still double-charges and can still flip a complete result into a
   `PROJECTION_WORK_LIMIT` partial. `RequestBudget::check_projection_op_count()` genuinely
   is request-scoped and monotonic, as stated.
3. **"EIGHT production sites" is an OVERCOUNT.** Four of the listed component-meta entry
   sites bind `_output_read_set` and discard it — they neither finalise it nor admit from
   it, so they are not signature consumers. **FOUR** genuine raw
   `host.with_fact_tracer(...)` signature/admission consumers remain: the semantic
   relation memo, the component-meta result DB, session compile, and encoded output. The
   conclusion is unchanged and still load-bearing — `install_fact_tracer` is NOT the
   signature-consuming chokepoint, and any future work assuming it is inherits the same
   blind spot.

### Recorded — Fix 4's fixtures prove correlation, not atomicity

The landed `holds_candidate_matching` IS atomic (one `entries.get` + one
`candidates.load()`, conjunction per candidate, no second slot read), but that property
is held structurally by the implementation, not demonstrated by a race-exercising test.
The fixtures prove CORRELATION only. Stated in the fixture's module doc so the evidence
scope is not overread.

### DONE — the dead `EffectiveExportSet` surface is deleted (1A-ii-pre-a)

The first of the four ADOPT-NOW fixes is complete. Deadness was established from the
tree before anything was removed, on two independent rails:

- **Static.** `insert_effective_export_set` (the sole write, `#[cfg(test)]`),
  `get_effective_export_set` and `effective_export_set_len` had **zero callers
  anywhere — not even tests**. Nothing ever wrote the table. Every
  `FactKey::EffectiveExportSet` construction lived under a test module or test file.
- **Behavioural.** After real resolution work the table was empty
  (`effective_export_set_len == 0`) and the production validator returned `false` for
  every `expected_hash`, including zero. This confirms the arm was an
  operationally-always-absent LOOKUP, not a literal `false` — an earlier report said
  literal `false` and was wrong.

`RouteSurface` remains a legitimate ONE-ARM domain whose sole fact is
`ModuleAugmentationIndexShape`. `build_module_augmentation_index_shape_fact_key` was
RELOCATED into `route_db.rs` (not deleted), preserving the existing
`resolver_core::route_db::…` path so both call sites — the production observation site
and the negative-observation fixture — resolve unchanged.

**Cascade worth recording:** the captured `StoreViewSnapshotRoots.route_db` handle
became unreadable-by-anyone once the arm went, because that arm was its only reader. It
and its capture plumbing were deleted rather than left as a never-read field.

#### Deliberate consequence — the cross-consumer grid narrows 10×5 → 10×4

`cross_consumer_fact_matrix_complete` required a slice per (consumer, fact-kind) over
five fact kinds, TWO of which were `RouteSurface`-domain: `route_surface` and
`module_augmentation_index_shape`. The grid carried two RouteSurface columns **because
the domain had two arms**. The `route_surface` column named the arm just deleted.

The column is retired, and this is recorded so a future reader does not encounter a
narrowed completeness guard and reasonably conclude erosion:

- **The alternative was worse.** Refiling the five affected slices against the surviving
  arm would have produced five tests that are *literal duplicates of their siblings* —
  same struct, same permissive view, same fact variant, differing only in a hash
  constant. They cannot fail differently from tests already present. That satisfies the
  grid while discriminating nothing, making the guard LOOK complete while its new cells
  are incapable of catching anything. A guard padded with duplicates is worse than an
  honestly smaller guard.
- **No coverage is lost.** The surviving arm keeps full per-consumer coverage through
  the `module_augmentation_index_shape` column, which is untouched.
- **The five behavioural survivors are retained**, outside the grid. They were never
  about the deleted fact: they assert RouteDb route WALKING (barrel routes), and were
  only filed under that column by name. They are renamed to what they actually test
  (`*_barrel_route*`, `app_config_proof_observes_no_route_facts`) so the next reader is
  not misled by a filename naming a fact kind that no longer exists.
- **The invariant is now stated in the guard's own doc**: `REQUIRED_FACT_KINDS` tracks
  the LIVE registry, so the grid's width is a consequence of the domain rather than a
  number to preserve. Narrowing is admissible ONLY when the fact kind is gone from the
  registry — never to accommodate an uncovered consumer, and never by filing
  non-discriminating duplicates. That converts the next narrowing from a judgement call
  into a check against a stated rule.

#### Recorded, not fixed — the route-surface arm's behavioural rail is single-threaded

Found by adversarial plant after the deletion landed. Inverting the surviving arm's
slot-miss result — `None => false` becomes `None => true` in
`validates_route_surface_domain` — is a STALE-ACCEPT: an unpopulated augmentation index
would validate a warm candidate instead of forcing recompute.

That mutation survives almost everything. It passes the domain-routing plant (which
exercises `FactKey::domain()`, not the comparison), it passes the rewritten R26 guard
(a source-grep, structurally incapable of catching an inverted comparison), and it
passes the ENTIRE `--test main` at 2397/0. **Exactly one test catches it:**
`cross_file_augmentation_merge_equivalence_tests::augmenter_gaining_a_declaration_invalidates_the_warm_non_contributing_result`.

This is NOT a regression from the deletion — the pre-image guard was a source-grep too,
so the rail was equally thin before. It is recorded because the deletion made the domain
one-arm, which concentrates the whole domain's behavioural coverage onto that single
test, and because the guard rewritten in the same change is NOT that rail and must not be
mistaken for it.

**Related one-arm residual.** `RouteSurface`'s single arm now sits above a `_ => false`
catch-all. DELETING that arm would compile clean and silently turn the entire domain into
a permanent miss — every route-surface fact rejected, every consumer recomputing forever,
no test failing except the one named above. A one-arm match over a catch-all has no
compile-time floor.

**Queued successor (out of scope for a deletion, better than either option considered).**
Derive the grid's column set STRUCTURALLY from `FactKey` via an exhaustive match, so a new
variant fails to COMPILE until its column decision is made. That satisfies the
landed-guards-are-structural rule, replaces the current filename scanner, and would have
made this adjudication mechanical rather than a judgement call. It also gives the one-arm
domain above a compile-time floor. Note the subset caveat: the grid is deliberately a
representative cross-section (4 of 15 live variants), so the structural derivation must
model "covered / explicitly waived", not "one column per variant".

---

## Block 1A-ii — recon findings, and the retry-seam STOP

Recorded so the next implementer does not re-derive them. Every claim below was
verified against the tree, not inherited from a report.

### BLOCKED — the retry protocol cannot be installed in `install_fact_tracer`

The corrected stability rule (above) said a mutation-unstable attempt is RETRIED. The
natural seam is `install_fact_tracer` (`fact_signature_helpers.rs:100`). It does not
hold, for three INDEPENDENT reasons — any one of which is disqualifying.

> **Read with "Corrections — the retry-seam block's supporting claims were partly
> overstated" above.** Reason 1's ref-cycle OVERFLOW example is wrong as written, and
> reason 2's "never-reset / sticky for the remainder of the request" lifetime is wrong.
> Both conclusions survive on their remaining grounds (the hard `FnOnce`; the
> per-attempt double-charge inside one connected-demand root). Reason 3's count is EIGHT
> only if four discard-the-read-set sites are counted; the genuine number is FOUR, and
> reason 3's conclusion is unaffected. Reason 3 also generalises beyond retry: it
> disqualifies the helper as the seam for ANY admission-boundary check, movement
> detection included.

**1. Three production sites are incorrect on re-run, not merely wasteful.**

The clearest is `component_meta_caches.rs:3346-3349`: `compute_fence` and
`observed_self_roots` are declared OUTSIDE the traced closure, which is
`|| compute_bfs(&mut compute_fence, &mut observed_self_roots)`, and the BFS APPENDS. A
second run duplicates fence rows and self-roots, which feed the entry's fact carrier —
inflating the signature toward `FACT_SIGNATURE_CAP` and producing a SPURIOUS OVERFLOW
and refused admission. **The retry manufactures the exact failure class `SIG-1` exists
to remove.** The bound is also hard `FnOnce`:
`C: FnOnce(&mut Vec<…>, &mut Vec<…>) -> bool`.

The others: `project_semantic_dispatch/mod.rs:2292` (duplicate
`ModuleAugmentationStitched` audit events; augmenter-set epoch reservation and
retired-chain growth per repeat) and `host_manage/component_meta_request_impl.rs:296`
/ `:465` (provenance double-count, and a FRESH `CanonicalCompletionOverlay` per run, so
the two attempts do not observe the same world).

**2. The budget double-charge is cross-cutting and ANSWER-CHANGING.**

This defeats the seam even at sites that look safe. `charge_connected_work`
(`project_semantic_dispatch/mod.rs:510`) increments a never-reset counter
(`state.work_used.set(work_used + 1)`) and, on exhaustion, sets `state.tripped` — which
its own first lines treat as STICKY for the remainder of the request.
`check_projection_op_count` (`request_budget.rs:82`) is a monotonic `fetch_add` against
a fixed cap.

So re-running any closure that dispatches semantic work charges that work twice against
caps that never reset, and can flip a genuinely COMPLETE result into a
`PROJECTION_WORK_LIMIT` partial. Since this cutover treats `BudgetExceeded` as a genuine
partial refused warm admission, that is not "wasteful but correct" — it changes the
answer. Do not read a double-charged budget as harmless.

**3. `install_fact_tracer` is NOT the signature-consuming chokepoint.**

EIGHT production sites consume a fact signature through raw
`host.with_fact_tracer(...)`, bypassing the helper entirely:

`semantic_query_memo/mod.rs:1592`, `component_meta_result_db.rs:635`,
`host_resolve/virtual_file_pipeline.rs:1678`, `host_manage/component_meta_entry.rs:397`
and `:441`, `host_manage/component_meta_entry_resolution.rs:378` and `:426`,
`meta/output_api.rs:295`.

A retry installed in the helper would cover none of them, so the correctness bar — zero
genuine advances between install and finalization for any ADMITTED attempt — would be
UNMET on those paths while APPEARING satisfied. That is the precise failure mode this
cutover exists to eliminate, reproduced inside its own fix.

**Also worth knowing:** `install_fact_tracer_named` and
`install_fact_tracer_cacheability_named` are `#[cfg(test)]` — the production arms of
`named_fact_tracer!` / `named_cacheability_scope!` expand to the unnamed helpers.

**Disposition: RULED — Rule B, `MutationUnstable` is terminal on the first unstable
attempt; no automatic retry.** See "Architecture ruling — `MutationUnstable` is terminal
(Rule B)" above for the amendment, the `MU-1` acceptance obligation, the new-typed-carrier
requirement, the `CacheabilityProbe::non_cacheable()` finding, and the `MU-R1` rate
metric. Bound-zero was REJECTED as framing (it is a deviation requiring the amendment,
now made) while being adopted as the behaviour. `1A-ii-a` — a retry protocol — is
therefore CLOSED as not-to-be-built; what replaces it is movement detection plus a typed
terminal refusal, owned by the composite-stamp units and 1A-iii. The tracer-local "this
trace caused the bump" exception remains unsound under every ruling.

### Substrate as it stands (verified), for the remaining units

- `CompactionDomain` (`verter_workspace/src/fact_cache.rs:26`) already carries all six
  variants: `Content`, `SourceEnv`, `SemanticImports`, `Resolution`, `RouteSurface`,
  `WorkspaceShape`.
- `AggregateStamp` (`fact_cache.rs:81`) documents its own limit: `Generation(u64)` is
  "sound only for a domain with exactly ONE producer — `Content`, `SourceEnv`,
  `WorkspaceShape`". `ResolutionRoots { base, session }` is the resolution stamp. The
  two composite stamps (SemanticImports, RouteSurface) have no carrier yet.
- ~~`aggregate_population` (`fact_read_set.rs:105`) is the unsound default~~ **FIXED in
  1A-ii-b** — see "1A-ii-b outcomes" below. It was: answers `ResolutionPopulation::Base`
  for everything except an aggregate naming its own population and a `ResolveImports`
  resolution fact.
- `set_aggregate_basis` (`fact_read_set.rs:299`) is per-domain first-write-wins via
  `.or(...)`; every session tracer installer still begins with all six bases ABSENT.

**Invariant status: currently SAFE.** No domain is armed, so none is both uncompacted
and unbounded — the legacy cap still covers every domain. Each unit must preserve that
property at its own commit boundary, arming a domain only together with its basis,
producer and validator.

### Unit ordering after the two rulings

`1A-ii-a` is CLOSED as not-to-be-built (Rule B above). The remaining order is:

| Unit | Content | Gate |
|---|---|---|
| `1A-ii-b` | Population translation — the view-derived population, and no-population-no-mint | — |
| `1A-ii-c` | The four single-producer domains (`Content`, `SourceEnv`, `WorkspaceShape`, and the root-wide `Resolution` stamp) | **LANDED** |
| `ZH-1` | The resolve-imports slot-miss arm becomes `false` (reopened 1A-ii-pre) | **LANDED** (precedes `1A-ii-d`) |
| `1A-ii-d` | `SemanticImports` composite stamp + movement detection + the typed `MutationUnstable` carrier (`MU-1`) | after `ZH-1` |
| `1A-ii-e` | `RouteSurface` composite stamp + movement detection (`MU-1`) | — |

`ZH-1` is ordered before `1A-ii-d` because `1A-ii-d` arms the SemanticImports producers
and validators, and arming a permissive slot-miss arm is exactly the moment its
unreachability stops protecting it.

`MU-1` is NOT satisfiable by a finalization-only check: `CacheabilityProbe::non_cacheable()`
can authorise writes inside its closure, so the composite-stamp units own movement
detection at the probe as well as at finalisation.

---

## 1A-ii-b outcomes

### The population is now a typed, view-derived input — and its absence refuses to mint

`aggregate_population` no longer answers `Base` by default. It is exhaustive over
`CompactionDomain` and returns `Option<AggregatePopulation>`:

| Domain | Population source |
|---|---|
| `Resolution` | the BUCKET — its precise facts carry a population in their own keys |
| `WorkspaceShape` | GLOBAL (`View(Base)`) — a project generation is a whole-host scalar no overlay shadows |
| `Content`, `SourceEnv`, `SemanticImports`, `RouteSurface` | the VIEW — `AggregateGenerations::view_population`, and `None` until one is supplied |

Three new carriers in `fact_cache.rs`:

- `SessionOverlayFingerprint` — a newtype whose constructor REFUSES the zero
  fingerprint, because "no overlays installed" IS the base view. Admitting zero would
  partition base entries from themselves. The check lives in the constructor rather than
  at each call site, mirroring `augmentation_population_for_view`'s `fingerprint() != 0`
  gate structurally instead of by convention.
- `ViewPopulation { Base, SessionOverlay(..) }` — deliberately NOT `ResolutionPopulation`.
  **This separation is STRUCTURALLY FORCED, not stylistic**, and the reasoning is recorded
  so a future "why are there two population types?" refactor does not undo it:
  1. **`ResolutionPopulation::Session` cannot EXPRESS per-overlay-set identity.**
     `default_resolution_session` is minted exactly once per `Engine::new()`
     (`engine.rs:430`), so the variant carries one fingerprint for the whole engine's
     lifetime. It has no room for "which overlay set is installed" — the very thing the
     view population must distinguish.
  2. **`verter_session` cannot CONSTRUCT one.** `SessionFingerprint::fresh` is
     `pub(crate)` to `verter_workspace`. The view population originates in the session
     crate, from `SessionView::fingerprint()`. There is no legal path from the producer of
     the value to the type that would have carried it.

  Either point alone forecloses reuse; together they make it a compile-time
  impossibility rather than a preference. Reusing the type would also have been a lie
  across two identity spaces owned by two different producers.
- `AggregatePopulation { Resolution(..), View(..) }` — the closed union
  `DomainGenerationFact.population` now carries.

**A request-completion overlay gets no variant, on purpose.** `CanonicalCompletionOverlay`
is append-only *within* a request, so it has no identity stable for the life of a scope.
A scope running under one supplies no view population and its view-derived domains simply
stay precise. That is the fail-safe direction and it is recorded rather than left implicit,
because a future implementer looking for the missing variant should find the reason.

`CapturedResolutionWorld` now refuses a resolution-domain aggregate carrying a `View`
population outright, instead of settling it against a stamp it happens to hold.

**Invariant preserved at this commit boundary.** No domain is armed in production: the
sole production `set_aggregate_basis` caller (`ResolutionTransaction::finish`) still
supplies only the resolution stamp and no view population, and `finalise`'s legacy
`FACT_SIGNATURE_CAP` refusal is untouched. Nothing is both uncompacted and unbounded.

### Discrimination — every recipe was EXECUTED, not authored

Seven plants against the landed tree, each reverted by inverse edit and each revert
verified byte-exact against a pre-plant checksum:

| Plant | Red |
|---|---|
| view-derived arm → unconditional `View(Base)` (the pre-change catch-all) | 3 |
| `SessionOverlayFingerprint::new(0)` returns `Some` | 1 |
| `WorkspaceShape` routed through `view_population` | 1 |
| `Resolution` routed through `view_population` | 5 |
| bucket key loses its population component | see note |
| `AggregatePopulation::View(_) => true` in the resolution world | 1 |
| mint filter loses `stamp_for(..).is_some()` + `.expect` → `Generation(0)` | **0 before 1A-ii-c, 1 after** |

**The bucket-key row carries no count.** It was recorded as 5, then corrected to 6, and
the correction is withdrawn: the recipe as written ("replace `aggregate_population(...)`
with a constant") is UNDER-SPECIFIED — which constant, and whether the mint site is
changed with it, both move the number — so independent re-runs do not agree and no count
is reproducible from the recipe text. The row is retained because the recipe does redden
the population-axis tests; only the count is withdrawn. Rewriting the recipe belongs to
1A-ii-b, which is closed.

The last is a ONE-TEST rail, the same shape the route-surface arm was recorded as having:
`a_view_population_aggregate_is_refused_by_the_resolution_world` is the only test in the
crate that catches it.

**One test was rewritten because its recipe would not discriminate.** A first draft
asserted "an over-threshold content domain lifts while resolution facts stay precise",
with the recipe "bucket by `CompactionDomain` alone". Executing it showed the recipe is
inert for that test — dropping the population component does not merge two DIFFERENT
domains, so the test stayed green and was merely restating the existing domain-wise
property. It was replaced by `two_populations_in_one_domain_lift_independently`, which
puts both buckets in ONE domain — the only arrangement where the population axis is
load-bearing — and the recipe then genuinely reddens it. Recorded because "the recipe
did not apply" and "the code is correct" are the two outcomes a plant must be able to
tell apart, and here they were only told apart by running it.

### Pre-existing flake, confirmed not caused by this unit

`filesystem::tests::concurrent_resolutions_are_not_refused_for_retry_exhaustion`
(16 threads × 24 rounds, asserts ZERO retry exhaustions) is **flaky but NOT reproducible
on demand**, and its rate is not characterised.

An earlier draft of this entry recorded "1 failure in 12 runs at baseline" as though it
were a rate. That is withdrawn — it was a single observation from a small sample, and a
matched A/B could not reproduce it: baseline **0/16 quiet and 0/24 under load**,
post-change **1/16 and 0/24**. Those are statistically indistinguishable and neither
supports a rate.

What IS established is that this unit cannot be the cause, mechanistically rather than
statistically: the test's signatures are far below the domain threshold so
`compact_domains` mints nothing either way, and `ResolutionRetryExhausted` is produced
upstream by world-capture retry logic this unit does not touch. Noted so a later red is
not misattributed — not as a measurement.

### Recorded, NOT fixed — the resolution basis holds one stamp for a domain that can hold two populations

Found while rewriting the bucketing. `AggregateGenerations::stamp_for(domain)` returns ONE
stamp per domain, but the mint loop mints per `(domain, population)`. The resolution domain
is the only one whose buckets can legitimately partition, so a signature holding BOTH a
base and a session resolution bucket would stamp both aggregates with whichever population's
stamp the basis captured (`ResolutionTransaction::new` captures `root.resolution_stamp(root.population)`).

**Why it is not fixed here.** It is FAIL-SAFE today, not a stale-serve, and the proof is in
the stamp's own shape: `resolution_stamp(Base)` is `{base, None}` while
`resolution_stamp(Session(fp))` is `{base, Some(session)}`. A bucket stamped from the wrong
population therefore compares unequal at validation in BOTH directions and is rejected. The
cost is a refused warm entry, never a wrong answer.

> **The proof has a named structural dependency, and is not self-maintaining.** It holds
> because `ResolutionPopulation` has exactly TWO arms whose stamp shapes differ in the
> `session` discriminant — `Base` always yields `session: None`, `Session(_)` always yields
> `session: Some(_)`, so the two are distinguishable without comparing the fingerprint. **A
> THIRD arm that also produced `session: Some(_)` would break it**, and the failure would be
> silent: two populations sharing a stamp shape could then cross-validate. Anyone adding an
> arm to `ResolutionPopulation` inherits the obligation to either preserve the
> shape-disjointness or fix the pairing properly. "Provably fail-safe" is a statement about
> today's two-arm enum, not a property of the design.

It is also unreachable today: `ResolutionTransaction::observe` stamps every key with the
transaction's own population, and the candidate slot key (`LazyResolutionCacheKey`) includes
population, so an absorbed run cannot carry a foreign one.

**It is out of scope for population TRANSLATION** — the translation decides which population
labels a bucket, and it is now correct for all six domains; this is about which STAMP the
basis can vouch for, which belongs with the per-domain bases. Owner: whichever of `1A-ii-c`
/ `1A-ii-e` / `1A-iii` gives the basis its population-aware shape (the natural fix is
`stamp_for(domain, population)`, symmetric with the validator's existing
`resolution_stamp(population)`, which already returns `None` for a population it cannot
answer for).

**This unit introduces no new instance of it.** The four view-derived domains take their
bucket population FROM the basis, so stamp and population are paired by construction, and
`WorkspaceShape` is always `View(Base)`. The new test
`two_populations_in_one_domain_lift_independently` deliberately puts only ONE bucket over
threshold so it does not depend on the unfixed half.

**SECOND INSTANCE of the same class — an absorbed aggregate can outlive its population.**
`aggregate_population` answers `Some(aggregate.population)` for an already-minted
`DomainGeneration`, so an absorbed aggregate buckets under its OWN population, not the
absorbing scope's. A warm candidate minted under `View(SessionOverlay(A))` and absorbed
into a `View(Base)` scope therefore lands in a bucket of its own, is never collapsed by the
base bucket, and SURVIVES INTO THE BASE SCOPE'S SIGNATURE.

Unreachable today for exactly the reason the first instance is: no production path supplies
a view population, so no view-derived aggregate is ever minted to be absorbed. Fail-safe if
it were reachable — a base view has no validator for a session-overlay aggregate, so the
whole entry is refused rather than wrongly accepted; the cost is a permanently-unusable warm
entry, not staleness.

Same owner block and same fix shape as the first instance: a basis that is population-aware
per domain also gives the absorbing scope a principled answer for "is this absorbed
aggregate mine to carry?". Recorded together so they are dispositioned together rather than
rediscovered separately.

---

## 1A-ii-c outcomes

### The three single-producer domains get a producer and a validator; the two composites fail closed

`Resolution` was already complete from 1A-i (`CapturedResolutionWorld::resolution_stamp`
plus its validator), so this unit armed the other three.

**Producer side.** `WorkspaceAccess::source_env_generation() -> Option<u64>` is the seam
the session reads; `Engine::current_source_env_generation` loses its `#[cfg(test)]` gate.
`Content` needed nothing new (`WorkspaceRead::content_generation` already exists) and
`WorkspaceShape` is session-owned (`ProjectTypeStore::project_generation`).

The accessor is `Option`, not `u64`, and it lives on `WorkspaceAccess` rather than
`WorkspaceRead`:

- **`Option`, because "no producer" must be representable and must not be spelled `0`.**
  A constant stamp is a witness nothing can ever advance and therefore nothing can ever
  invalidate. The trait default is `None`, which correctly disarms the domain.
- **`WorkspaceAccess`, because only the host-level session seam reads it.** The
  resolution-time readers (transaction, overlay-snapshot, frozen-snapshot) have no
  source-env concern, and putting it on the read trait would have forced
  `FrozenFilesystemResolutionReader` to choose between reporting its CAPTURED value and
  the LIVE one for a dimension it does not revalidate — an arbitrary choice with no right
  answer.

The `None` default is also the hazard, so each production workspace has its own assertion
(`memory_workspace_exposes_…`, `filesystem_workspace_exposes_…`): a forgotten override is
not a compile error, it silently disarms the domain for every host on that workspace.
Both recipes executed — deleting either override reddens exactly its own test and leaves
the sibling green.

**Validator side.** `HostStoreView::validates_domain_aggregate` is exhaustive over
`CompactionDomain` — a new domain cannot compile without stating how the view validates
it, which is the compile rail that stops a new domain inheriting a permissive or blanket
answer. Two independent gates: the aggregate's population must equal
`HostStoreView::view_population()` (the new shared derivation, mirroring
`augmentation_population()` so producer and validator cannot disagree), AND its stamp must
equal the generation the view captured. `SemanticImports` / `RouteSurface` return `false`
until their composite stamps land.

### `source_env_generation` is deliberately NOT a `StoreViewValidationToken` dimension

Recorded because it looks like an omission and is not.

Of the three dimensions a `FileSourceEnv` fact carries, `parser_version` is a compile-time
constant and `file_language_id` cannot move at runtime (both established earlier in this
errata). `parse_env_hash` is the only one that can actually change — and it is already
folded into the token's `env_hash_fold` via `fold_env_hashes`. So every source-env change
capable of invalidating the domain ALREADY supersedes the view.

The counter bumps more eagerly than that: any env-table republication moves it, including
a byte-identical one. Promoting it to a token dimension would therefore supersede every
cached store view on a no-op republish — and widen the singleflight lane identity, since
`external_supersession_fingerprint` folds the same set — while protecting nothing
`env_hash_fold` does not already protect.

### `RequestStoreView` refuses view-derived aggregates instead of delegating

Its `DomainGeneration` arm previously delegated wholesale to the base view, under a comment
asserting that "a per-canonical overlay never alters a project-wide generation". That is
true of `ProjectGeneration` and of a `WorkspaceShape` aggregate. It is FALSE of a `Content`
aggregate, which stands in for exactly the per-canonical whole hashes, derived hashes and
parse facts the completion overlay shadows.

The four view-derived domains now refuse outright; `WorkspaceShape` and `Resolution` still
delegate. The refusal holds even on an EMPTY overlay — the case that most looks safe to
delegate — because the overlay is append-only WITHIN a request, so "empty right now" is
sound only until the first completion lands. This pairs with the mint side, where a
request-completion overlay supplies no view population and so mints no such aggregate at
all.

### Discrimination

Five recipes executed against the landed tree, each reverted by inverse edit and re-run
green:

| Plant | Red |
|---|---|
| delete the `MemoryWorkspace` `source_env_generation` override | 1 (sibling stays green) |
| delete the `FilesystemWorkspace` override | 1 |
| drop the `population == self.view_population()` conjunct | 2 |
| `SourceEnv` arm reads `unwrap_or(0)` instead of the `Option` | 1 |
| `RequestStoreView` delegates every aggregate to its base | 1 |

**One recipe is recorded as WEAK, in the test's own doc.** For the `WorkspaceShape` arm,
"compare against `self.view_population()` instead of the literal `ViewPopulation::Base`"
does NOT discriminate: on a base view the two coincide. The test says so and names the
recipe that does discriminate (routing its stamp elsewhere). Recorded rather than quietly
left, because a recipe that cannot redden its own test is exactly the failure mode the
plant discipline exists to catch.

### The Block-6 wall-clock flake, characterised

`cold_synthesis_terminates_within_500ms_for_50_member_heritage` failed once in the
`verter_session --lib` run for this unit (4780 passed, 1 failed — the +8 over the previous
4772 is this unit's new tests).

**It is UNCHARACTERISED, and two attempts to characterise it were both wrong.** "Passes
5/5 in isolation" was wrong — it failed 2 of 6 ISOLATED runs here at load average 51. The
replacement, "passes below some core-saturation threshold", is also wrong — an
independent run got 6/6 isolated passes at load average **66**, above the load that
failed here. Load average is a lagging one-minute figure and a poor proxy for contention
during a 0.5-second measurement, so it does not explain the observations either way.

What is left, stated without a mechanism: a 500ms WALL-CLOCK assertion living in a
correctness suite, which fails intermittently inside the full parallel run. No rate and
no threshold is attributed to it. Owner: Block 6. Not chased here.

### Open item discharged — session-population aggregates are now exercised

1A-ii-b's "Open, to establish at 1A-ii (not assumed)" §2 asked for session-population
aggregates to be proven end to end once armed rather than argued. **Disposition:
ADOPT-NOW**, discharged here, because this unit is what arms the validator.

It was initially MISSED — 1A-ii-c shipped its validator with the population gate tested
in one direction only, and the outcomes section neither discharged the item nor carried
it forward. That is the silent-drop the repo's explicit-finding-disposition rule exists
to prevent, and it is recorded rather than quietly fixed.

**What was untested was the PRODUCER half of the gate.** `view_population()` had two call
sites — the validator, plus one test assertion that exercised only its `None`/`Base` arm
(an earlier draft of this entry said "exactly ONE call site"; corrected). And
`ViewPopulation::SessionOverlay` was constructed in exactly two places in the session
crate: the production line and a test helper hard-coding a synthetic fingerprint. Every
test built a BASE view, so the `Some(fingerprint)` branch was dead in test — which is the
load-bearing claim, and the plant confirms it. Collapsing it to `ViewPopulation::Base` left
the whole suite green.

The consequence of that plant is a genuine stale serve in the direction the tests did not
cover. The existing tests check an overlay-LABELLED aggregate against a base view; the
dangerous direction is the reverse — a base-derived Content aggregate satisfying a
session-overlay read, serving base-rooted content under a view whose overlay shadows
exactly the per-canonical facts that aggregate collapsed.

`a_real_session_overlay_view_derives_its_population_and_refuses_base_aggregates` closes
it, using a real `OverlaidView` whose fingerprint is DERIVED rather than chosen, and
asserting producer and validator together — the gate's contract is that the two agree,
and either half alone is satisfiable by a constant. Plant verified: the collapse now
reddens exactly this test.

### Invariant preserved at this commit boundary

Stated explicitly because this is the first unit to arm anything, and because the rule is
that the invariant must hold at EVERY commit boundary rather than at the end.

No domain is armed in the full sense the errata defines (basis + producer + validator).
This unit ships PRODUCER and VALIDATOR only. There is still no basis installer: the sole
production `set_aggregate_basis` caller remains `ResolutionTransaction::finish`, which
supplies the resolution stamp and no view population, so no view-derived domain mints
anything. `finalise`'s legacy `FACT_SIGNATURE_CAP` refusal is untouched. Nothing is both
uncompacted and unbounded; the newly-landed validators are reachable only by an aggregate
that no production path can currently produce.

### Recorded — the `source_env_generation` token argument inherits the `parse_env_hash` dependency

The argument for keeping `source_env_generation` out of `StoreViewValidationToken` rests
on `parse_env_hash` being the only runtime-movable source-env dimension AND already being
folded into `env_hash_fold`.

That inherits the dependency recorded above under "REJECT — the `parse_env_hash` key
asymmetry is structurally unreachable": `parse_env_hash` folds only a constant salt plus
workspace-wide parser flags, and is byte-identical across projects. A future change that
folds per-project state into it was already known to break the resolve-imports key. It
would break THIS argument too — a per-project `parse_env_hash` means the single
workspace-level `env_hash_fold` no longer covers every project's source-env change, and
the counter would have to become a token dimension after all. That second consequence was
unrecorded; `parse_env_asymmetry_tests` is the fixture that fires first.

### Recorded, NOT fixed — Content compaction makes the self-root axis validate vacuously

A forward risk for 1A-iii / 1B, created by arming Content's validator and not previously
recorded anywhere. Fail-safe in both of its consequences, but it must be dispositioned
before the basis lands.

`ReadSetSignature::validate_with_self_roots` routes a `FileWholeHash` for a LISTED
self-root through the strict `validates_self_root_whole_hash`. A Content-compacted
signature has no `FileWholeHash` facts at all — the lifting deletes every one of them,
self-roots included — so that axis validates VACUOUSLY for a compacted entry.

The codebase already enumerates this hazard class:
`has_view_discriminating_self_root`'s doc names three carriers that "could only ever
validate vacuously". Content compaction creates a FOURTH, which is not on that list.

Two consequences:

1. **A global content generation is not a per-canonical trackedness assertion**, so it is
   not an equivalent substitute for the self-root axis. The aggregate says "the content
   domain held as of N"; the self-root axis says "THIS canonical is tracked and hashes
   thus". The first does not imply the second.
2. **The in-flight joiner gate would see `has_view_discriminating_self_root == false` for
   every Content-compacted carrier and FORK EVERY FOLLOWER** — disabling coalescing for
   exactly the large entries `SIG-1` exists to make reusable. A performance inversion, not
   a correctness break.

Owner: 1A-iii (which installs the basis and so decides when Content can first compact).

### Carried forward from 1A-ii-c — the population gate's fingerprint-EQUALITY axis is uncovered

Not a defect in the landed code, and not in 1A-ii-c's scope (which was the producer half).
Recorded so a follow-on unit picks it up rather than rediscovering it.

Degrade `population == self.view_population()` in
`HostStoreView::validates_domain_aggregate` to a DISCRIMINANT-only comparison — base
matches base, any overlay matches any overlay — and **every test in the tree stays green**.

The reason is structural, not an oversight in any one test. The two population tests pit
`View(Base)` against an overlay view and `View(SessionOverlay(..))` against a base view;
both pairs differ in DISCRIMINANT, so neither exercises fingerprint equality. There is
exactly one non-zero fingerprint constant anywhere in the workspace's tests, so
overlay-A-versus-overlay-B is asserted nowhere.

What that would let through: two DIFFERENT session overlays cross-validating each other's
compacted witnesses — session B serving content rooted in session A's overlay set. The
fix needs two distinct real overlay views, which is why it is a unit of its own rather
than one more assertion.

### Residual on the discharged `ZH-1`-adjacent item — label vs behaviour

The 1A-ii-b open item discharged at 1A-ii-c named its subject literally as
"`RequestStoreView` routing a `Session(fp)` aggregate through `self.base.validates`".
That routing NO LONGER EXISTS: 1A-ii-c changed the arm to refuse view-derived aggregates
outright rather than delegate. So the item is discharged behaviourally — session-population
aggregates are now exercised end to end — but its literal subject was deleted in the same
unit that discharged it. Recorded rather than left as a silent mismatch between the item's
words and the tree.

---

## ZH-1 outcome

### The slot-miss arm rejects; the sentinel had no other role

`validates_resolve_imports_domain_for_content_hash`'s slot-miss arm is now
`None => return false`, replacing `None => return *expected_hash == ZERO_HASH`.

The `ZERO_HASH` constant in that function became DEAD on the change and was removed. That
is a small corroboration of the ruling's reasoning: within this validator the sentinel had
no role other than the permissive miss it is being removed from — it was not participating
in the hit path at all.

**Reachability was proven by the red, not assumed.** The function bails earlier when the
view captured no `resolved_import_facts` root (`None => return false`), and a fixture that
tripped that bail would reject for the wrong reason and pass under BOTH arms. The two
rejection tests failing against the pre-change tree is what establishes the fixture
genuinely reaches the slot-miss arm; a vacuous fixture would have been green from the
start.

### Discharge conditions — all three met

1. **Tracked / no-slot / zero-hash rejects.**
   `a_zero_hash_semantic_fact_is_rejected_on_a_slot_miss_for_a_tracked_file`, with BOTH
   preconditions asserted rather than assumed — the canonical is tracked (else the
   separate untracked arm settles it) and no bundle is admitted (else the lookup hits).
2. **Recipe EXECUTED against the landed tree.** Restoring the permissive arm reddens both
   rejection tests and leaves the positive control green. The first anchor attempted for
   the plant matched TWO sites and the scripted assertion aborted before writing — the
   plant was re-anchored on the unique comment above the arm and re-verified present.
3. **Positive control.** `a_present_candidate_with_a_matching_fact_still_validates` builds
   its fact from the payload the producer ACTUALLY admitted (via `set_import_dependencies`),
   not from a hand-written hash, so it cannot pass against a fabricated expectation. Its
   own recipe — make the whole function reject — reddens it while the two rejection tests
   stay green, which is the pair that distinguishes "rejects a miss" from "rejects
   everything".

A third test, `a_slot_miss_rejects_regardless_of_the_recorded_hash`, pins that the miss is
the reason and the hash value is not: zero, `[7; 16]` and `[255; 16]` all reject. Without
it the arm could be re-read later as "zero is special".

### Out of scope, and kept out

The UNTRACKED-canonical zero-accept in `validates_resolve_imports_domain`
(`whole_hash(..) == None => *expected_hash == ZERO_HASH`) is the R26 untracked-file window.
It answers a different question — is this canonical tracked at all — and is untouched. The
tracked-canonical precondition asserted in the fixture is what keeps these tests off it,
so a future change to that arm cannot be mistaken for a regression here.

---

## 1A-ii-d outcomes

### The SemanticImports stamp is a composite, and the reason is the KEY, not the membership

`AggregateStamp::SemanticImports(SemanticImportsStamp)` pins `semantic_imports +
content + source_env + resolution + workspace_shape`.

A bare membership counter is unsound because the store answers per KEY —
`(canonical, content_hash, parse_env_hash, resolve_env_hash, resolver_version)` —
and every key dimension lives in ANOTHER domain. A witness pinning only the
membership counter keeps validating across a content edit or an env
republication that re-keys every slot it stands in for, because no admission
happened. That is the exact case
`an_edit_refuses_a_previously_captured_composite` exercises.

One derivation (`HostStoreView::semantic_imports_stamp`) serves producer and
validator, so the two cannot disagree about what the domain's stamp means. A
missing component yields `None` and REFUSES, never a substituted constant —
a fabricated component would reintroduce the very under-pinning the composite
exists to remove.

### The clock, and the bracket

`ResolvedImportFactsDb` is the domain's sole write chokepoint (confirmed: no
production `clear` caller, no `remove`, absent from every invalidation
inventory). One monotonic generation covers it:

| Event | Advance? |
|---|---|
| A candidate ENTERS the slot (including when it evicts the oldest) | yes |
| A REFUSED admission (empty / over-cap witness) | no |
| An identical-candidate producer SKIP | no — it never reaches `admit` |
| `clear` | yes, unconditionally |

Eviction is confirmed NOT a second validity dimension: the drain happens inside
the same `rcu` as the push, so five admissions produce five advances, not six
(`eviction_rides_the_insertion_that_causes_it_rather_than_advancing_separately`).

**The concurrency caveat is discharged by a BRACKET, not a post-increment.**
`resolver_core::bracketed_generation::BracketedGeneration` keeps the counter ODD
for the whole duration of a mutation and EVEN otherwise; a membership-changing
mutation leaves it two higher, a no-change mutation restores it, and an UNWIND
advances (membership is unknown, so claiming a new generation is the
conservative direction). Writers are serialised, and that is load-bearing rather
than incidental: two concurrent `fetch_add`s would make the counter EVEN in the
middle of both mutations, recreating the stable-looking window the type exists
to eliminate.

`stable()` returns `Option<u64>` — `None` while a mutation is in flight — so
"a mutation is running" is a state a reader OBSERVES rather than a race it
loses. An installer that snapshots `Some(g)` and an admission boundary that
re-reads `Some(g)` therefore prove no membership-changing mutation ran between
them.

The gap is demonstrated CLOSED rather than held structurally
(`bracketed_generation_tests`): a barrier-pinned test asserts no stable stamp
exists for the whole duration of a mutation — the assertion a post-mutation
increment cannot satisfy at all, since such a protocol has no in-flight state —
plus a real concurrent writer/reader test asserting the pairing invariant
(`membership observed == the membership that generation denotes`) directly on
observed state.

### `MU-1` — movement detection at BOTH seams, with a typed terminal refusal

New carriers: `FactReadSetFinalise::MutationUnstable` and
`NonAdmissionReason::MutationUnstable`. Neither folds into `SignatureOverflow`,
and `install_fact_tracer` does NOT emit `FactSignatureOverflow` for it — a
stability failure reported as a cardinality failure is refused under exactly the
size rail this cutover removes.

Both seams check, and they are not variations of one another:

- `install_fact_tracer` rechecks before `finalise`;
- `CacheabilityProbe::non_cacheable()` rechecks AT THE PROBE, because it can
  authorise a write from inside the scope's closure. The plant proving this
  (deleting the probe-side recheck) leaves the EXIT verdict correct and reddens
  only the in-scope assertion — i.e. an exit-only test would not have caught it.

`AggregateGenerations::any_named_domain_moved` examines only domains the basis
NAMES (a domain that never compacts cannot be corrupted by its generation
moving) and destructures with no `..`, so a new field on the struct is a compile
error rather than a silently unexamined dimension. There is no tracer-local
"this trace caused the bump" exception, and one must not be added.

`live_aggregate_basis` is ONE composer used at BOTH ends, so a domain it cannot
answer for is absent on both sides and never registers as spurious movement. It
reads through `.current()`, not the allowlist-confined `into_owned_view()`: a
view that is not current names no domain, which also makes a view that becomes
non-current mid-scope register correctly as movement.

### Invariant preserved at this commit boundary

No production path supplies a per-domain basis — the sole
`set_aggregate_basis` caller is still `ResolutionTransaction::finish`. Movement
detection short-circuits on an empty basis, so production behaviour is
unchanged and the legacy `FACT_SIGNATURE_CAP` refusal still covers every domain.
Nothing is both uncompacted and unbounded. The mechanism is exercised in-crate
by installing the real basis through the real accessor onto the real tracer, and
a no-basis scope is asserted UNAFFECTED by the same mutation — which is what
pins the short-circuit rather than assuming it.

### The fingerprint-EQUALITY gap is CLOSED

Carried forward from 1A-ii-c. `two_distinct_session_overlays_do_not_cross_validate_each_others_aggregates`
builds two REAL overlay views whose fingerprints are derived (never chosen) and
asserts session B refuses a witness minted under session A. Plant executed:
degrading `population == self.view_population()` to a discriminant-only
comparison reddens exactly this test and nothing else in the module — confirming
the errata's diagnosis that the axis had no coverage at all.

### Recorded — the `content` component has NO isolating behavioural plant

Stated at the strength of the evidence rather than claimed. Measured: an
`upsert` moves the content generation AND republishes the resolution world
(`content 2→3`, session `ResolutionWorldId 5→6` on the same edit). No public
host API reaches a content mutation that leaves the resolution world alone, so
`an_edit_refuses_a_previously_captured_composite` proves the composite refuses
across an edit but does NOT attribute the refusal to `content`.

The consequence is recorded in the fixture's own doc: hard-coding `content`
inside `semantic_imports_stamp` leaves the whole module GREEN (executed and
confirmed). `content`'s participation rests on the whole-stamp equality proven
by the omit-a-field plant plus its derivation from `self.content_generation`,
not on a behavioural isolation this host shape can express. `workspace_shape`
and `semantic_imports` DO isolate cleanly and each has an executed hard-coding
plant. An earlier draft of the behavioural test asserted the content arm
WITHOUT checking isolation and passed under its own plant — recorded because
"the fixture bails for another reason" and "the code is correct" are the two
outcomes a plant must distinguish, and here only an explicit isolation
assertion distinguished them.

### Residual — the probe seam's refusal is Boolean, not typed

`CacheabilityProbe::non_cacheable()` folds instability into the existing
`bool` alongside non-cacheable reads and overflow, so a probe-only caller cannot
tell the three apart. The typed distinction exists at the signature-consuming
boundary (`FactReadSetFinalise::MutationUnstable` →
`NonAdmissionReason::MutationUnstable`) and every reason-propagating site names
it truthfully rather than as overflow. Widening the probe's return to a typed
verdict is a 17-site migration on surfaces `1B` restructures anyway; it is
recorded here rather than done twice.

---

## 1A-ii-e outcomes

### The RouteSurface stamp is a composite, and its clock is NOT `artifact_generation`

`AggregateStamp::RouteSurface(RouteSurfaceStamp)` pins `route_surface + content
+ source_env + workspace_shape`. No resolution component: the augmentation index
is addressed by artifact identity and project env and is never composed against
the resolved-import world.

`RouteSurfaceGeneration` is a NEW clock on `FileArtifactStore`, deliberately
separate from `artifact_generation`. The 1A-ii inventory's finding D is what
makes the separation load-bearing rather than tidy: the index mutates INSIDE
active fact tracers on the same thread, and an inner mutation destabilises every
enclosing scope on the stack. A clock that inherited `artifact_generation`'s
shape would make the domain refuse its own consumers' cold work — the exact
performance inversion `SIG-1` exists to prevent.

| Event | Advance? | Landed rule |
|---|---|---|
| First-time materialisation of an index row | **no** | `prev == None` ⇒ unchanged |
| Same-fingerprint self-heal republish | **no** | `prev.fingerprint == new` ⇒ unchanged |
| Cache-only `clear_augmentation_index` | **no** | not routed through the clock at all |
| A published set replaced by a DIFFERENT fingerprint | yes | |
| Artifact retirement removing index contributors | yes | `invalidate_augmentation_index_at_epoch`, `removed > 0` |

Both publishers (`populate_augmenter_set` and the cold
`ensure_augmentation_index_populated`) route through ONE bracketed seam,
`publish_augmenter_set`, so neither decides the rule for itself. The existing
`artifact_generation` gates are untouched — they answer a different question
(store-view reuse) and keep answering it.

**The corrected clock is proven at the seam it exists for.**
`warming_the_augmentation_index_inside_a_scope_does_not_destabilise_it` runs a
real cold index materialisation INSIDE a real tracer with a real installed
basis and asserts the scope still admits. Plant executed: changing
`publish_augmenter_set`'s `is_some_and` to `is_none_or` — literally the
`artifact_generation` rule — reddens exactly that test. That is the direction
that would otherwise be lost silently, because every clock test in isolation
still passes under it.

### Discrimination — six recipes EXECUTED against the landed tree

| Plant | Red |
|---|---|
| `publish_augmenter_set`: `is_some_and` → `is_none_or` (the `artifact_generation` rule) | 1 clock test + the in-tracer stability test; the same-fingerprint sibling stays GREEN |
| `publish_augmenter_set`: `changed` unconditionally `true` | 2 (both no-advance tests); the change control stays green |
| `publish_augmenter_set`: `changed` unconditionally `false` | 1 (the change control) |
| `invalidate_augmentation_index_at_epoch`: `(removed, false)` | 1 (the retirement test) |
| `RouteSurface` arm → `matches_view_counter(...)` | 3 |
| `route_surface_stamp`: `workspace_shape: 0` | 1 (the isolated-shape test); the perturbation test stays green |

The first row's asymmetry is recorded because an earlier draft claimed the
same-fingerprint test failed under it too. It does not — that plant moves the
`None` arm alone — so each no-advance test carries its own recipe rather than
sharing one that only reddens its sibling.

### The one-arm residual is unchanged, and now has a second consequence

`RouteSurface` remains a legitimate ONE-ARM domain whose single
`ModuleAugmentationIndexShape` arm sits above a `_ => false` catch-all, so
deleting that arm still compiles clean and silently makes the PRECISE side a
permanent miss. The aggregate side is now separately covered
(`validates_domain_aggregate` is exhaustive over `CompactionDomain`), so the
compile floor exists for the DOMAIN but still not for the arm. The queued
structural successor — deriving the cross-consumer grid's columns from `FactKey`
via an exhaustive match — remains the fix.

### Recorded, NOT fixed — a NEW augmenter appearing retires nothing

Found while establishing the retirement rule. Retirement is driven by an
augmenter's PRIOR artifact being retired, so a brand-new augmenter file (no
prior artifact) retires no index rows and advances neither
`artifact_generation` nor the route-surface clock. The already-published index
row for its target stays stale, and the PRECISE `ModuleAugmentationIndexShape`
fact stale-accepts for exactly the same reason — so this is a pre-existing
index-invalidation gap, not one the composite introduces.

The composite is nonetheless protected by over-coverage: a new augmenter file
arrives by `upsert`, which moves `content_generation`, so the composite's
`content` component refuses. That is the fail-safe direction, and it is recorded
rather than relied upon silently — the precise arm remains uncovered, and
whoever closes the index gap should not read this note as saying it is closed.

### Invariant preserved at this commit boundary

Unchanged from 1A-ii-d: no production path supplies a per-domain basis, movement
detection short-circuits on an empty basis, and `finalise`'s legacy
`FACT_SIGNATURE_CAP` refusal is untouched. All six domains now have a producer
and a validator; none compacts. Nothing is both uncompacted and unbounded.

---

## 1A-iii — BLOCKED. Two independent structural findings, both verified

Basis installation was attempted, measured, and REVERTED. No 1A-iii code is
landed. Units `1A-ii-d` and `1A-ii-e` stand unchanged and green.

### FINDING 1 — the tracer chokepoint cannot read a store view (MEASURED)

`with_fact_tracer_cell` IS the right chokepoint. It is the one place a tracer
cell is created, so it covers `install_fact_tracer`, `with_cacheability_scope`
AND the four raw `with_fact_tracer` consumers that bypass both helpers — the
blind spot that disqualified the helper as a retry seam. Installing there works
and is the correct seam.

What does NOT work is COMPOSING the basis there. The composer reads the store
view (needed for the view population, and for the SemanticImports composite's
resolution-root component), and a store-view read per TRACER SCOPE violates the
O(1)-store-view-read invariants the tree already enforces.

**Measured, not inferred.** Installing the basis at the chokepoint failed 9
tests, all store-view/self-root counting invariants:

```
output_batch_equals_scalar_and_is_o1_store_view_reads_when_warm
  → "8 items took 10 reads, 4 items took 6 — a per-item read path scales with the batch size"
warm_public_api_batch_from_host_calls_are_o1_not_per_item
view_bound_cold_compute_seeds_from_executor_snapshot_not_a_second_read
query_db_self_root_tests::{declaration_lookup,materialize_memo,owner_collection,resolvability}_failed_revalidation_does_not_leak_live_counter
query_db_self_root_tests::declaration_lookup_straddling_compute_is_not_served_to_the_winner
external_module_augmentation_broken_lease_contributor_folds_cache_suppress
```

Attribution PROVEN by inverse control rather than asserted: replacing the
composer body with `AggregateGenerations::default()` — same chokepoint install,
no view read — cleared all 9. Only the six 1A-iii-local stability fixtures failed
(their basis was empty by construction), plus the known Block-6 flake.

So the basis must be composed from LIVE HOST COUNTERS ONLY (`content_generation`,
`source_env_generation`, `project_generation`, and the two bracketed clocks — all
O(1) atomic loads). Two things do not survive that constraint, and neither is a
detail:

- the SemanticImports composite's `resolution` root-identity component;
- the `view_population`, without which the four view-derived domains mint
  NOTHING and `SIG-1` is vacuous.

A constant `Base` population is UNSOUND, not merely imprecise: a scope computing
under a session overlay would mint `Base`-labelled aggregates from
overlay-read facts, and a base reader would accept them. That is exactly the
stale serve `a_real_session_overlay_view_derives_its_population_and_refuses_base_aggregates`
and `two_distinct_session_overlays_do_not_cross_validate_each_others_aggregates`
were written to prevent.

**No thread-local escape, and it is banned by design.** `fact_tracer_tls` holds
only the tracer stack; `request_context` holds no view, overlay or fingerprint;
and `session_view.rs` records that a thread-local "current view" is FORBIDDEN by
the `request_view_is_retired_from_crate_sources` guard. `HostStoreView::view_population`
is a private fn, not on the `StoreView` trait, with no accessor.

**The available seam** is the explicitly-threaded context:
`ctx.active_session_view().map(|v| v.fingerprint())` reproduces the population
discriminant exactly and is already read at eight production sites. That means
the population must be THREADED to tracer installation from the callers that own
a context — a signature change across all six entry points, and a design the plan
does not specify.

### FINDING 2 — Rail A cannot warm-hit a compacted candidate at all

Independent of Finding 1, and the more consequential of the two. There are TWO
warm-read rails, and they validate against different views:

- **Rail A — `ctx.store_view()` ⇒ `RequestStoreView`.** Prepared-decl bundles,
  `RouteDb`, `ImportedRootDb`, the fallthrough node cache,
  `OwnerImportSurfaceDb`, `MaterializeStructureDb`, `RefCycleResultDb`,
  `semantic_query_memo`, the framework surface/script-fact stores. This is most
  of the cache graph.
- **Rail B — concrete `CurrentHostStoreView` ⇒ `HostStoreView`.**
  `ComponentMetaResultDb::get_with_view` (its parameter is concretely typed, so
  it CANNOT receive a `RequestStoreView`), plus `ResolvedImportFactsDb::get_if_valid`.

`RequestStoreView::validates` refuses every `DomainGeneration` aggregate in the
four view-derived domains — the 1A-ii-c decision, pinned by
`a_request_store_view_refuses_view_derived_aggregates_its_base_would_accept`,
and resting on a real soundness argument (the completion overlay shadows exactly
the per-canonical facts the aggregate collapses, and is append-only within a
request).

An overlay-bearing `RequestStoreView` is installed for essentially EVERY
production request: both `HostResolverContext::{from_current, from_cold_seed}`
and `SessionResolverContext::from_cold_seed` construct one unconditionally, there
is no no-overlay context variant, and the bare-host rail panics in release.

**Consequence.** The moment a population is supplied, every Rail-A warm read
misses on any compacted candidate. Compaction would deliver ADMISSIBILITY
without REUSE across most of the cache graph — precisely the failure the Block 6
acceptance "SIG-1 must deliver reuse, not merely admissibility" names.

**This collides with the ratified `SIG-1` acceptance itself.** The 0B inversion
table's first two rows — `owner_import_route_witness_for_tests` `is_none()` →
`is_some()`, and `candidate_signatures_for_key(owner)` empty → non-empty — are
`OwnerImportSurfaceDb`, which is Rail A. Under the current refusal those rows
cannot invert to a REUSED entry no matter what 1B deletes.

The refusal is not relaxable by loosening the match arm; the overlay-shadowing
argument has to be answered. Three candidate resolutions, none of them this
unit's to choose:

1. give the aggregate a request-overlay population (the errata's own reason for
   refusing one — no identity stable for the life of a scope — has to be
   overturned or bounded);
2. permit delegation when the overlay is provably empty AND frozen for the
   validation, which needs the append-only window closed;
3. re-scope `SIG-1` to Rail B (`ComponentMetaResultDb`) and record that Rail A
   compaction is admissible-but-not-reusable until a later block.

### Why 1B is blocked too, and was not attempted

The binding order exists for this: "an unarmed domain must retain the legacy cap
until armed. There must never be an intermediate state in which a domain is both
uncompacted and unbounded." With no basis installed, nothing compacts — so
deleting the size gates would admit arbitrarily large UNCOMPACTED signatures and
`SIG-1` would go green while establishing nothing. That is the plan's own named
vacuity, so 1B is blocked BY 1A-iii rather than independently.

### State at this record

`1A-ii-d` and `1A-ii-e` are landed and green: all six domains have a live
producer and a live validator, movement detection (`MU-1`) is wired at both
admission-owning seams, and NOTHING compacts. The invariant holds — the legacy
`FACT_SIGNATURE_CAP` refusal still covers every domain.

---

## Rail-A ruling — the blocker is partly OVERTURNED, and the design contract it replaces it with

The 1A-iii BLOCKED record above stands as to Finding 1 (measured) but its
Finding 2 CONCLUSION is overturned. Recorded here rather than rewritten in
place, because four of the ruling's factual premises are under independent
verification and this section must not be read as settled where it depends on
them.

### What I can confirm from the tree directly (not contested)

- **`RequestStoreView` DELEGATES `Resolution` and `WorkspaceShape`.**
  `request_store_view.rs` `DomainGeneration` arm:
  `CompactionDomain::WorkspaceShape | CompactionDomain::Resolution => self.base.validates(fact)`,
  with only the four VIEW-DERIVED domains returning `false`.
  My Finding 2's *statement* ("refuses every view-derived aggregate") was
  accurate; the *conclusion* drawn from it — "Rail A can never warm-hit a
  compacted candidate" — does NOT follow, because `Resolution` is not a
  view-derived domain. That inference is withdrawn.
- **`ResolutionTransaction::observe` pushes only `ResolveImports::Resolution`.**
  Its single push site constructs
  `FactVersionRef::ResolveImports(ResolveImportsFactRef::Resolution(..))`.

### Caveat offered to the verification, not an adjudication

The claim under verification is that the oversized owner witness consists
**exclusively** of resolution-currency facts. `observe` alone does not establish
that: `ResolutionTransaction::finish` also merges every run in `self.absorbed`,
and `absorb` takes a whole `ReadSetSignature` by reference — which is exactly the
mechanism 0B recorded as the growth driver ("the warm-reuse arm feeds it the
reused candidate's ENTIRE signature by reference"). Exclusivity therefore
depends on every ABSORBED run also being Resolution-only, transitively, not just
on the local push site. That is checkable and worth checking; it is not resolved
here.

### Revised `SIG-1` (verbatim, supersedes the prior wording)

> **SIG-1 — Cardinality of dependency facts or self-roots never by itself causes
> non-admission or `ReturnOnly`. Every over-threshold set is replaced by a sound,
> bounded terminal witness. A compacted candidate must warm-hit on the next
> eligible lookup under the same effective validation population; population
> mismatch or mid-scope movement must reject for that typed reason, never as
> overflow.**

### The 0B table is superseded

"After Block 1A" becomes **"After 1A-iii plus the relevant 1B gate migration"**,
and the table is replaced by:

| Acceptance | Required result |
|---|---|
| `owner_import_route_witness_for_tests` | `Some`, bounded; the 180-import declaration-companion fixture contains 360 `Decision` facts (two resolver queries per authored import) |
| `candidate_signatures_for_key(owner)` | Non-empty; candidate contains the 360 Decision facts and retains its precise owner `FileWholeHash` self-root |
| Direct Rail-A prepared-bundle lookup through two fresh EMPTY `RequestStoreView`s | Second lookup: `bundle_cache_hits +1`, `bundle_cold_flight_runs +0` |
| Direct `OwnerImportSurfaceDb` lookup through two fresh EMPTY `RequestStoreView`s | Second performs no new owner-surface tracer installation |
| Public warm-pass `bundle_cold_flight_runs` | Delta `0` |
| Public warm-pass `component_meta_result_cache_hits` | Delta `1` — **Rail B; NOT proof of Rail A** |
| `derived_raw_cache().cached_resolved_meta.len()` | Non-zero |

The original public warm pass cannot prove prepared-bundle Rail-A reuse, because
the top-level component-meta hit may bypass that lookup entirely. (The prose
above the old table also said "exactly four assertions" while listing five;
corrected.)

### Q2 — the basis seam. THREADING, not a per-scope view read

The measured Finding 1 stands and its inverse control was the right attribution:
the breakage came from the READ, not from composing a basis. The ruling removes
the read rather than the composition. `ctx.store_view()` is a BORROW of the
already-bound request view, not a new `StoreViewManager` read — which is the
step the blocked attempt missed.

Required shape:

1. A required, object-safe `StoreView` projection — `aggregate_basis_seed()` —
   returning the already-bound view's captured stamps, currentness, the
   SemanticImports resolution-root identity, and the effective population.
2. A `FactTracerBasisSource` combining that seed with the host's O(1) live
   counters and the completion-overlay revision handle.
3. `with_fact_tracer_cell(source, ...)` installs the initial basis on the one
   newly-created cell; `CacheabilityProbe` retains the same source for
   mid-scope rechecks.

The live SemanticImports recheck uses the workspace's O(1)
`capture_resolution_world()` plus atomics and must NOT rebuild a
`HostStoreView`.

Call sites: the semantic memo, component-meta result DB and output API pass
their existing context/view; the component-meta result APIs must ADD the
context/view parameter their callers already hold but discard before
`compute_and_admit`; the compile consumer takes a typed base-only source from
its existing source/root capture; test/named helper variants take the same
source.

`active_session_view().fingerprint()` alone is INSUFFICIENT — it misses the
completion overlay, base currentness and the captured resolution-root
component.

### Q1 — the `RequestCompletion` population (required regardless of the verification)

```text
RequestCompletion {
    parent: Base | SessionOverlay(fingerprint),
    overlay_id: process-unique exact identity,
    revision: monotonic logical-state revision,
}
```

- An EMPTY completion overlay projects to its PARENT population, so empty
  request views reuse durable Base/Session candidates across requests. This is
  the empty-overlay optimisation, adopted — and it is what makes the two
  new "two fresh empty `RequestStoreView`s" acceptance rows expressible.
- Non-empty uses `(parent, overlay_id, revision)`; reuse only within that exact
  overlay state.
- Writers bracket one logical multi-map update with a sequence/revision
  protocol; revision advances only when EFFECTIVE SHADOWING state changes. Same
  shape as the landed `BracketedGeneration`.
- Tracer installation snapshots the population; every admission check and
  finalisation rechecks it; movement ⇒ `MutationUnstable`.
- **Whole-signature** validation snapshots or LEASES the overlay state ONCE.
  Per-fact checks are insufficient — a writer can straddle the signature.
- Request-specific candidates must not FIFO-evict durable Base/Session
  candidates: request-local retention, or population-aware slots.
- `compat_token` stays the external-coherence/singleflight token and is NOT the
  population identity. Any view-dependent singleflight that can return a
  leader's result without follower validation must additionally key on this
  population, or validate the leader's completed carrier under the follower.

### Q3 — the self-root disposition is OVERTURNED; `StrictSelfRootWorld` replaces it

The vacuity finding was factually wrong: compaction finalises the traced facts
FIRST, and the structural-carrier builders then prepend or preserve their
observed self-root hashes (semantic graph → `semantic_graph_read_set_signature`;
ref-cycle → `ref_cycle_read_set`; materialisation's post-finalise merge retains
producer self-roots). So a completed Content-compacted carrier still lists
`FileWholeHash` self-roots, `validate_with_self_roots` is not vacuous, and
`has_view_discriminating_self_root` stays true. The doc that was cited lists
three no-root RESULT SHAPES, not three carrier types.

**A different and worse problem takes its place, and is NOT deferrable to Block
6:** roots, fence facts and prelude facts are added AFTER tracer compaction, so
once Content compaction lets finalisation succeed those additions can re-inflate
the completed carrier past the bound.

Required — a distinct terminal witness (`StrictSelfRootWorld` or equivalent):

- strictly validate every observed self-root against the exact effective view
  before minting;
- stamp with a collision-free identity of that strict-validation world,
  INCLUDING the Q1 request population;
- any transition capable of changing a strict whole-hash / trackedness answer
  advances that identity;
- warm validation compares the world identity in O(1);
- `has_view_discriminating_self_root` recognises this witness; a generic
  `Content` aggregate does NOT;
- population mismatch forks; same-population followers coalesce.

This concedes the valid half of the trackedness objection: a global content
generation alone is not a per-canonical trackedness assertion; a
producer-proven strict-validation-world witness is.

**1A-iii acceptance must now prove:** an over-cap self-root carrier stays
bounded; deleted/untracked or hash-changed roots reject; same-population warm
reads and followers reuse/coalesce; different request/session populations
reject/fork; and post-finalisation fence/prelude merging cannot re-expand any
compacted domain past the bound.

### Three OPEN items — carry them, do not assume them away

1. Whether current token fields cover every transition affecting strict
   artifact-only self-root validation (that path consults live
   `derived_raw_cache` presence and `file_exists`). **Default to a dedicated
   monotonic authority generation for `StrictSelfRootWorld`; drop it only on an
   exhaustive coverage proof.**
2. No hard existing bound below 1,024 was established for post-finalisation
   self-roots/fences — the structures are unbounded `Vec`s and the default
   projection budget is 2,000. Do not assume they are harmless.
3. Lease contention cost is unquantified. Prefer the optimistic even-sequence
   check (O(1), avoids a manager read) and record the measurement obligation.

### 1B inventory A — the size gates (VERIFIED against the landed tree)

The plan's "the compiler is the completeness oracle" is false, so 1B needs an
enumerated list. This is it, re-verified at current line numbers (they moved
during 1A-ii-d/e):

| # | Gate | Site | Shape |
|---|---|---|---|
| 1 | `ValidatedFactCache::insert_arc_inner` | `resolver_core/mod.rs:1494` | `facts.len() > FACT_SIGNATURE_CAP` ⇒ refuse + `FactSignatureOverflow` event |
| 2 | `ValidatedFactCache::resign_arc_with_kind` | `resolver_core/mod.rs:1570` | same shape — the SECOND gate the plan's `:1494` citation missed |
| 3 | Import-route witness | `host_manage/import_route_witness.rs:198` | `witness.len() > FACT_SIGNATURE_CAP` on a bare `Vec<FactVersionRef>` that never passes through a `FactReadSet`, so `compact_domains` is INERT there |
| 4 | `FactReadSet::would_overflow` | `fact_read_set.rs:571`, `:575` | the non-finalising peek |
| 5 | `FactReadSet::finalise` | `fact_read_set.rs:612` | the `Overflow` mint |

Downstream of gate 4, `would_overflow()` has exactly THREE production readers,
all in `fact_signature_helpers.rs` (`:259` post-scope fold, `:442`
`CacheabilityProbe::non_cacheable`, `:473` the named-scope fold), each folding to
a `bool`. **19** production `with_cacheability_scope` / `named_cacheability_scope!`
/ `install_fact_tracer_cacheability` sites consume that bool — the brief's figure
was 17, and the delta must be reconciled before the number is treated as an
inventory rather than an estimate.

`SignatureAdmission::from_finalise` has **6** direct production callers (the
"~25 sites" figure is the transitive fan-out, not the call count).

`ReadSetSignature::overflow()` has exactly **ONE** production caller —
`project_semantic_dispatch/template_class_facts.rs:177` — used as the
non-cacheable SENTINEL for the `NonCacheable` case as well as the overflow case.
Every reader of that value is already gated on `completeness == Complete`, so
`→ empty()` suffices when `overflow()` is deleted.

### 1B inventory B — raw-insert / re-sign seams (the CLASS, not the named site)

Every PRODUCTION seam admitting a candidate whose signature did NOT come from
`FactReadSet::finalise()`. These bypass the tracer, so `compact_domains` never
runs on them: deleting the size gates without migrating these admits an
UNBOUNDED candidate.

`ValidatedFactCache` signature-carrying mutators, all taking a bare
`Vec<FactVersionRef>` — there is NO type-level distinction between a traced and
a hand-built signature at the cache boundary: `insert` (`resolver_core/mod.rs:1459`),
`insert_arc` (`:1463`, loose — no empty guard), `insert_arc_with_kind` (`:1475`),
the private `insert_arc_inner` (`:1485`), `resign_arc_with_kind` (`:1562`). The
`StableRequestState` wrapper forwards only the latter two
(`resolver_runtime.rs:104`, `:115`).

| Seam | Mutator | Signature origin | Bound today |
|---|---|---|---|
| `host_manage/prepared_decl.rs:924` (built `:857-863`) | `insert_arc_with_kind` | **RAW** | witness TAIL only (`import_route_witness.rs:198`) |
| `host_manage/prepared_decl.rs:1088` (built `:1053-1061`) | `insert_arc_with_kind` | **RAW** | witness TAIL only — the SIBLING the ruling did not name |
| `resolved_import_facts.rs:403` (built `producer.rs:360-365`) | `insert_arc_with_kind` | **RAW** | witness TAIL only |
| `resolver_core/imported_root_db.rs:303` | `insert_arc_with_kind` | **RAW** (`frontier_engine.rs:646-661`, one-to-many facts per traversed canonical) | **NONE** |
| `resolver_core/route_db_singleflight.rs:110` | `insert_arc_with_kind` | **RAW** (same supplier) | **NONE** |
| `resolver_core/fallthrough_resolver.rs:250` | **loose `insert`** | **RAW** | **NONE** — and no strict empty-signature audit either |
| `host_manage/component_meta_methods.rs:2788` | `resign_arc_with_kind` | **HYBRID** — traced base ∪ raw append (`merge_extraction_fact_versions`, `meta_resolve/resolved_state.rs:124-137`), union NEVER re-finalised | **NONE** |
| `host_manage/component_meta_methods.rs:2621`, `:2712` | `insert_arc_with_kind` | TRACED (a filter only shrinks) | bounded before the cache |
| `resolver_core/route_db.rs:764` | `insert_arc_with_kind` | RAW | **NONE** — gated `#[cfg(any(test, feature = "test-support"))]`, so not shipped, but compiled into any `test-support` build |

**Six seams admit an unbounded candidate if the gates go without migration:** the
three with no bound at all, plus the three bounded only on the witness TAIL —
`import_route_witness.rs:198` bounds the witness, never the COMPOSED vector,
which always carries at least one extra `FileWholeHash` head. A witness at
exactly the cap composes to `CAP + 1` and is rejected downstream SILENTLY, since
both prepared-decl sites discard the insert's return value.

The witness's own dedup comment (`import_route_witness.rs:241-243`) claims "the
consuming producer folds these into its own `FactReadSet`". That is FALSE for
three of its four consumers — rows 1, 2 and 6 `.extend()` it into a raw vec.

`owner_import_surface.rs:294` is the seam that already solved this: it DISCARDS
the raw compose at `:589-598` and rebuilds the stored signature from the owner's
own tracer, with the invariant stated at `:584-586`. It is the natural template.

### 1B inventory C — validators bypassing the central whole-signature rail

`StoreView::validates_fact_signature` (`resolver_core/mod.rs:325`) is the central
entry. **It has NO production override** — the trait default is itself
`sig.iter().all(|f| self.validates(f))`, and the only other impl forwards through
`&T`. So today every bypass below is semantically identical and none is a live
defect. They become defects the moment per-signature or aggregate-aware logic
lands behind that method — which is exactly what the Q1 whole-signature
snapshot/lease is. **Centralise them BEFORE that logic, not as independent
cleanup.**

Eleven production sites: `resolver_core/mod.rs:1280-1283`, `:1331-1338` (the
self-root arm — genuinely different logic, check before folding), `:1401-1404`,
`:1441-1444`; `owner_import_surface.rs:218`;
`host_resolve/virtual_file_pipeline.rs:783`; `host_manage/analysis_io.rs:420`
(re-borrows the view PER ITERATION — the widest straddle window);
`host_manage/component_meta_methods.rs:3044-3046`;
`fact_signature_helpers.rs:647`/`:650` and `:705-711`/`:715-721` (the strict
ctx rail `validate_fact_signature_with_self_roots`, which takes the view ONCE but
still loops per fact and never calls `validates_fact_signature`).

The `ValidatedFactCache` sites are holes by INHERITANCE for every store built on
it: `ImportedRootDb::roots`, `RouteDb::routes`, `RouteDb::barrel_surfaces`,
`FallthroughResolverState::cache`, `ResolvedImportFactsDb::entries`, and
`StableRequestState::cache` (prepared-decl bundles + component-meta).

### `absorb` is now structurally constrained (qualifier 2 discharged)

`ResolutionTransaction::absorb` took an ARBITRARY `&ReadSetSignature` and
retained its facts wholesale, with an in-tree test deliberately pushing a
`ProjectGeneration` fact through it. Production was resolution-only by
one-caller accident, not by invariant — and the whole Rail-A fix depends on the
owner witness being Resolution-only-compactable, because ONE non-resolution
aggregate fails the WHOLE signature there.

It now takes `ResolutionOnlySignature` (`fact_cache.rs`), a newtype whose
constructor refuses any fact outside `CompactionDomain::Resolution`. An
already-lifted `DomainGeneration{Resolution}` passes, so absorbing a run that
already compacted stays admissible and bounded. The sole production caller
(`engine.rs`) now DECLINES the reuse when the token cannot be constructed —
never filters, since dropping facts would under-root the witness into a stale
serve.

The foreign-domain test became the refusal test it now is, and the DOMAIN-WISE
precision property it used to carry moved to
`fact_read_set_tests::lifting_one_domain_leaves_every_other_domain_precise`,
which drives `FactReadSet` directly and can still mix domains. Plant EXECUTED:
relaxing the constructor to `Some(Self(signature))` reddens exactly the refusal
test.

**Open (verifier's item 2), now ESTABLISHED:** the four view-derived domains
cannot mint an aggregate at all today. `compact_domains` requires BOTH a stamp
and a population, `aggregate_basis_for_stability` supplies
`view_population: None`, and it is the sole live basis composer. So qualifier 1's
mixed-signature hazard is NOT live today — it becomes live exactly when the basis
starts supplying a population, which is what makes ORDERING (Resolution first,
Content later) an option rather than a race.

---

## Qualifier 4 outcome — the `canonical_id()` projection class, audited whole

The brief named `template_class_facts.rs`'s `owner_only_publication_safe`.
The class is wider, and one member is a REAL defect rather than a latent
one — but not the one named.

### The structural cause, and the fix

`FactVersionRef::canonical_id() -> Option<&str>` collapsed the TWO reasons
a fact names no canonical into one `None`:

* `ProjectGeneration` names none because it DESCRIBES none;
* `DomainGeneration` names none because it STANDS IN FOR the domain's
  precise facts across an unbounded set of them.

Skipping the second does not make a per-canonical projection smaller, it
makes it an UNDER-APPROXIMATION — and no decision site could tell.

`FactAttribution { Canonical(&str) | ProjectScalar | DomainAggregate(CompactionDomain) }`
(`fact_cache.rs`) is now the projection every decision site matches on;
`canonical_id()` is DERIVED from it so the two cannot drift.
`ReadSetSignature::aggregated_domains()` / `aggregates_domain(domain)`
carry the completeness half a bare canonical list cannot.

### The audit, by consumer class

Two classes, and the split is load-bearing:

* **Grouping / eviction** consumers may use the narrower projection.
  Registration and drain walk the SAME projection so they stay symmetric,
  the aggregate rejects at read time on ANY movement in its domain, and
  cache correctness is read-side authoritative. The cost is a candidate
  that lingers until FIFO, never a stale serve. Members, all now carrying
  the decision at the site: `cache_runtime/candidate_store.rs`
  `reverse_index_canonicals`; `semantic_query_memo/reverse_index.rs`
  `register_reverse_index` + `drain_candidate_reverse_index_registrations`;
  `semantic_query_memo/mod.rs` `invalidate_canonical` +
  `record_family_admission_locked`; `typeinfo/vue_macro_codegen/runtime.rs`
  `fact_footprint` (which feeds `sync_transitive_macro_type_dependencies`,
  the legacy per-canonical reverse-dep set).
* **Coverage** consumers must fail closed. An empty projection is
  indistinguishable from a complete one for them, which is the
  silent-wrong-answer shape.

### FIXED — `Engine::refresh_resolution_evidence` (the real defect)

`engine.rs` re-observed only the canonicals a candidate's OWN witness
recorded. A witness whose `Resolution` bucket compacted records none, so
the healing pass would have selected zero targets and returned `false`.
Its own doc states the failure this prevents: "a recorded `Absent` would
otherwise keep validating for the process's lifetime" — an installed
package under `node_modules` never picked up. This is user-visible, and it
arms at step 6 (Resolution first), not later.

Fix, per rule:

* Rule 1 (pending ledger) has a sound superset — the ledger itself. An
  aggregated witness now targets the whole pending ledger; a precise
  witness is unchanged and stays strictly O(its own facts).
* Rule 2 (`Uncovered` backend) does NOT. Its rule is stated over the
  witness's own path observations and no ledger records "every path
  canonical ever observed". So an aggregated witness cannot be certified
  under that backend at all, and `witness_evidence_is_unenumerable` now
  refuses it for REUSE rather than serving evidence never re-read.
  `FilesystemWorkspace` is `Uncovered` on every native target, so this is
  the production arm.

### FIXED — `owner_only_publication_safe`

Correct answer (`false`), reached by accident: an aggregate produced
`None`, `None` failed `is_some_and`. The slot it gates
(`compile_output_pure_content`) is keyed on the owner's content hash
ALONE and carries no fact signature, so a `true` here publishes a
cross-file-dependent compile output under a key that tracks only the
owner's bytes. Now a named arm.

### Correct as they stand — do not re-litigate

* The four `.filter(|f| f.canonical_id() != Some(owner))` sites
  (`host_manage/fallthrough.rs` ×2, `host_manage/component_meta_methods.rs`
  ×2) RETAIN an aggregate, which is right: they build a cross-file
  dependency set, and a whole-domain aggregate is definitionally not the
  owner's own fact.
* `app_config_proof_db.rs` `fact_references_canonical` and
  `semantic_query_memo/family.rs` `carrier_facts_reference_canonical`
  already name `DomainGeneration` explicitly (eviction class).
* `request_store_view.rs`'s `canonical_id()` call sits inside
  `validates_resolve_imports_domain`, whose parameter is
  `&ResolveImportsFactRef` — a `DomainGeneration` cannot reach it.
* `fact_signature_helpers.rs` `has_view_discriminating_self_root`'s
  `_ => false` is conservative-safe AND is exactly what Q3's
  `StrictSelfRootWorld` contract requires ("a generic `Content` aggregate
  does NOT" discriminate). It needs no change now.
* The `other => view.validates(other)` arms in the self-root validators
  are not a strictness loss: the strict path exists only to close the lazy
  `FileWholeHash` untracked-file loophole, and `validates_domain_aggregate`
  has no such loophole — every arm is fail-closed against a counter a
  covered canonical's edit would move.

### RECORDED, not fixed — one diagnostic gap

`host_manage/prepared_decl.rs` `attribute_prepared_decl_bundle_rejection`
has no named `AuditEvent` for a rejecting `DomainGeneration`, so a
compacted candidate's rejection attributes to
`PreparedDeclBundleRejectOther` — whose doc claimed it "must stay 0 in
steady state". NO test asserts that, so it is not a step-6 landmine; the
doc now records the exception. Naming it properly is an `AuditEvent`
schema addition and was kept out of this step deliberately.

---

## E outcome — the bypassing validators are centralized

All eleven sites now route through ONE overridable entry point on
`StoreView`, landed BEFORE any per-signature or aggregate-aware logic can
sit behind it (which is what Q1's whole-signature snapshot/lease is).

### The shape, and why `:1331-1338` did not need its own entry point

The brief flagged the self-root arm as "genuinely different logic — verify
before folding it in". Verified: it is not a different RULE, it is the same
rule with a parameter. The core is

```rust
fn validate_fact_signature(&self, sig: &[FactVersionRef], self_root_canonicals: &[&str])
    -> Result<(), usize>
```

and with an EMPTY `self_root_canonicals` the strict guard
(`self_root_canonicals.contains(..)`) can never fire, so every fact routes
through the lazy `validates` — byte-identical to the plain loop. That is
why the two `bool` forms are wrappers over one rule rather than two rules,
and `the_plain_wrapper_is_the_self_root_wrapper_with_no_roots` pins it: if
they ever diverge, a caller's choice of entry point silently changes the
answer.

`Err(index)` rather than `bool` because `get_if_valid_self_rooted_attributed`
needs to NAME the rejecting fact. Returning the index means the attribution
comes out of the same validation that decided the miss, instead of a second
separately-written loop that could disagree with it.

### The eleven, all migrated

`resolver_core/mod.rs` `get_if_valid_with_admission` / `get_if_valid_self_rooted`
/ `get_if_valid_self_rooted_attributed` / `get_if_valid_with_facts`
(`get_if_valid` already delegated to the first);
`owner_import_surface.rs` `get_with_view`;
`host_resolve/virtual_file_pipeline.rs` `compile_slot_facts_validate`;
`host_manage/analysis_io.rs` (which re-borrowed the view PER ITERATION — the
widest straddle window in the tree, now one borrow for the whole signature);
`host_manage/component_meta_methods.rs` `fact_versions_match`;
`fact_signature_helpers.rs` `validate_fact_signature` (both ctx arms) and
`validate_fact_signature_with_self_roots` (both ctx arms).

### Discrimination

A pre-change probe overriding today's `validates_fact_signature` with a
whole-signature rule (reject any signature naming >1 canonical) was RED on
both the plain and the self-rooted readers — they ignored the override.
The landed suite (`resolver_core::central_signature_rail_tests`) drives all
five readers against a view whose ONLY override is the central rail, plus
an accept-control, the strict/lazy self-root split, and the attribution.

One recipe caught a FALSE PASS in the test itself: the first attribution
fixture put the rejecting fact LAST, so a `facts.last()` mutation passed.
The fixture now puts the rejecting self-root FIRST with two trailing lazy
facts, and the mutation reddens.

### One pre-existing guard repointed

`tests/cases/g_misc1/owner_import_surface_and_negative_route_facts.rs`
`owner_import_surface_db_has_view_aware_lookup` grepped for the literal
`view.validates(fact)`. Its INVARIANT (validate against the caller's view)
is intact and strengthened; only the spelling moved. It now asserts the
whole-signature call AND the ABSENCE of the re-inlined loop. No new
name-keyed scanner was added — this is maintenance of a grandfathered one.

---

## Reconciled — the `would_overflow` consumer count is 16, not 19 and not 17

Both recorded figures are wrong, and the third spelling in the errata's own
list contributes zero.

`would_overflow()` production readers: **4**, not 3 — the errata missed
`verter_workspace/src/fact_read_set.rs:778` (`FactReadSetCell::would_overflow`,
the interior-mutability forwarder). The three session-side folds are
`fact_signature_helpers.rs:259`, `:442`, `:473` as recorded.

Cacheability-scope sites downstream of that bool:

| Spelling | Production call sites |
|---|---|
| `with_cacheability_scope(` | 14 |
| `named_cacheability_scope!(` | 2 |
| `install_fact_tracer_cacheability(` | **0** |
| **Total** | **16** |

The 14: `resolver_core/imported_root_db.rs:189`;
`resolver_core/component_meta_query_engine/registry_decl.rs:373`;
`.../registry_cache_producers.rs:761`;
`resolver_core/fallthrough_request.rs:440`;
`resolver_core/fallthrough_resolver.rs:221`;
`resolver_core/route_db.rs:405`/`:551`/`:722`;
`host_manage/prepared_decl.rs:488`;
`component_meta_caches.rs:685`/`:982`/`:1139`/`:1309`/`:1880`.
The 2: `framework/script_facts.rs:872`/`:1008`.

`install_fact_tracer_cacheability` has NO production caller at all — only
`fact_signature_helpers_tests.rs` and doc references. It is an internal
forwarder to `with_cacheability_scope` (`fact_signature_helpers.rs:492`),
so counting it as a third consumer spelling double-counts the same rail.
`fact_signature_helpers.rs:492` is that forwarder and is not an independent
consumer either.

Earlier counts most likely folded doc-comment mentions into the tally:
`with_cacheability_scope` appears on 22 production lines, 8 of which are
`///` prose in `component_meta_caches.rs` and `resolver_context.rs`.

---

## Handoff after steps 1 and 2

Steps 1 (qualifier 4) and 2 (E) are LANDED and green. They were the
prerequisites; steps 3–7 are untouched. Nothing compacts yet, so the
invariant "no domain is ever both uncompacted and unbounded" still holds
unchanged — the legacy `FACT_SIGNATURE_CAP` refusal covers every domain.

### What the next implementer inherits that the brief did not describe

* **`FactAttribution` is the projection vocabulary now.** Step 5(a)'s
  `StrictSelfRootWorld` and step 6's mixed-signature acceptance both need
  to ask "is this fact an aggregate, and of which domain". That question
  has a typed answer (`FactVersionRef::attribution()`,
  `ReadSetSignature::aggregated_domains()` / `aggregates_domain(domain)`).
  Do not reach for `canonical_id()` for it.
* **`StoreView::validate_fact_signature(sig, self_root_canonicals)` is
  where step 4's whole-signature population snapshot/lease goes.** It is
  the only overridable rule, every warm reader is behind it, and the two
  `bool` forms are wrappers. Step 6's "a mixed Resolution+Content
  signature is refused by `RequestStoreView`" is an override of this same
  method — a `RequestStoreView` override there is now reachable from all
  eleven former bypass sites, which is exactly what step 2 bought.
* **`Engine::witness_evidence_is_unenumerable` already fails closed for
  the `Uncovered` backend.** When step 6 arms Resolution, a compacted
  witness is refused for reuse under `FilesystemWorkspace` (which is
  `Uncovered` on every native target). If that proves too conservative in
  practice, the fix is a ledger of observed path canonicals, NOT relaxing
  the refusal.
* **`aggregated_domains()` returns a `Vec`.** It allocates. Every current
  caller is cold; `aggregates_domain(domain)` is the allocation-free
  single-domain form and is what the hot paths use. Keep it that way.

### Verified counts at this boundary

`verter_workspace --lib` 731/0 (was 726 — plus 5 new).
`verter_session --lib` 4821/1 (was 4812 — plus 9 new; the 1 is the known
Block-6 wall-clock flake `cold_synthesis_terminates_within_500ms_for_50_member_heritage`,
which reports 0.79s / 1.48s / 3.43s across runs depending on parallel load).
`verter_session --test main` 2397/0.

### Not started, and why the order still holds

Steps 3 (Q2 basis seam), 4 (Q1 population), 5 (Q3 `StrictSelfRootWorld` +
relocating the cap), 6 (arm Resolution) and 7 (Block 1B) are untouched.
The ordering constraint the brief states is unchanged by this work: nothing
may arm a population before the cap is relocated (step 5b), because
`FactReadSet::finalise` still applies `FACT_SIGNATURE_CAP` BEFORE the
structural carriers append their roots — so a compacted signature that
finalises under the cap can still be re-inflated past it downstream, and
`ReadSetSignature::new` / the three `ComputeAdmission::Cacheable`
publication paths have no cap check at all.

---

## Step-3 reconnaissance, and one hazard the Rail-A ruling does not name

Recorded from the tree, not attempted. Steps 3–7 are NOT started.

### CONFIRMED — `with_fact_tracer_cell` really is the one chokepoint

`resolver_context.rs:1425` is the only place a `FactReadSetCell` is
created, and `with_fact_tracer` (`:1412`) delegates to it
(`self.with_fact_tracer_cell(|_cell| f())`). So installing at the cell
covers `install_fact_tracer`, `with_cacheability_scope` and every raw
consumer. The errata's claim holds.

### CORRECTION — "four raw consumers" undercounts the THREADING surface

There are FOUR raw consumers that consume the finalised signature:
`host_resolve/virtual_file_pipeline.rs:1678`, `meta/output_api.rs:295`,
`component_meta_result_db.rs:635`, `semantic_query_memo/mod.rs:1592`.
That figure is right for "raw SIGNATURE consumers".

But four MORE call `with_fact_tracer` and discard the read set —
`host_manage/component_meta_entry.rs:397`/`:441`,
`host_manage/component_meta_entry_resolution.rs:378`/`:426` — plus the
`src/tests/dispatch_bridges.rs:33` bridge. A basis is installed on the
CELL regardless of whether the caller reads the signature, so a design
that threads a `FactTracerBasisSource` parameter through
`with_fact_tracer` touches EIGHT production sites, not four. Either give
`with_fact_tracer` a typed base-only default source or budget for all
eight.

### HAZARD — arming a basis also arms a per-recheck store-view read

Finding 1 attributed the nine O(1) failures to composing the basis at
INSTALL time. That is where the failures were measured, but it is not the
only place the read lives.

`fact_signature_helpers.rs:143` `live_aggregate_basis(host)` calls
`host.resolver_store_view_read()`, which is
`HostStoreView::from_host_read(self)` (`resolver_store.rs:4609`) — a real
store-view read, and the same one `capture_batch_fixed_view`'s doc calls
out as the "O(N)→O(1) read AND overlay-COW collapse".

It is reached from `note_basis_recheck` (scope exit) and
`note_basis_recheck_on_cell` (mid-scope, the cacheability seam). BOTH
short-circuit on `!names_any_domain()`. Nothing installs a basis today, so
both are inert — which is exactly why the recheck path did not show up in
the measured nine.

**The moment a basis is installed, every admission boundary and every
scope exit performs a store-view read.** That is the same invariant class
the nine tests guard, on a hotter path than installation. Step 3 must make
the RECHECK O(1) too — the `FactTracerBasisSource` the ruling describes
(seed + live O(1) counters + the overlay revision handle) has to serve
both reads, not only the install — and step 6 must re-run the nine named
tests AFTER arming, not only after step 3.

The nine tests to run as that gate:
`output_batch_equals_scalar_and_is_o1_store_view_reads_when_warm`;
`warm_public_api_batch_from_host_calls_are_o1_not_per_item`;
`view_bound_cold_compute_seeds_from_executor_snapshot_not_a_second_read`;
`query_db_self_root_tests::{declaration_lookup,materialize_memo,owner_collection,resolvability}_failed_revalidation_does_not_leak_live_counter`;
`query_db_self_root_tests::declaration_lookup_straddling_compute_is_not_served_to_the_winner`;
`external_module_augmentation_broken_lease_contributor_folds_cache_suppress`.

### `StoreView` impls a required `aggregate_basis_seed()` would touch

Production: `HostStoreView` (`resolver_store.rs:3267`), `RequestStoreView`
(`request_store_view.rs:808`), `PermissiveStoreView`
(`resolver_core/mod.rs:543`), the blanket `&T` forward (`:457`).
Test/stub: 15 further impls across `src` and `tests/cases` — a REQUIRED
(non-defaulted) method makes every one of them a compile error, so give it
a fail-safe default (name no domain ⇒ compact nothing) or budget the
19-impl edit.

---

## The full `gate.mjs` baseline is NOT clean, and the brief's baseline never measured it

The brief names one tolerated failure (the Block-6 wall-clock flake) plus
the `typeinfo_proto_ts_freshness` byte-pin, against a baseline stated for
the TARGETED runs only — `--test main` 2397/0, `verter_workspace --lib`
726/0, `verter_session --lib` 4812/1. Those three are accurate and this
train meets them. But the FULL 23,004-test `node scripts/gate.mjs` was
never run at that baseline, and it does not pass.

Measured on this tree (`da11c09dc`), full run, 23004/23004 executed:
**22997 passed, 7 did not pass.** Every one is pre-existing or
environmental; none is caused by steps 1–2.

| Test | Disposition |
|---|---|
| `verter_session meta_resolve::slot_binding_graph_tests::cold_synthesis_terminates_within_500ms_for_50_member_heritage` | The KNOWN Block-6 wall-clock flake the brief tolerates. Observed at 0.79s / 1.48s / 3.43s / 5.6s depending on machine load. Counted twice by the gate (once per surface). |
| `verter_session::main cases::output_projector_residual_guards::hot_materialize_scanner_flags_in_memory_injected_offender` | TIMEOUT at 180s under full-suite load; **PASSES in 106s** run targeted. A load-induced timeout on the grandfathered source-tree scanner, not a content failure. |
| `verter_lsp::main cases::tsserver_e2e_generated_outputs::test_e2e_tsserver_scoped_slot_types_from_generated_vue_outputs` | **PASSES** targeted. Real-tsserver flake. |
| `verter_lsp server::server_tests::completion_with_real_tsserver_recovers_when_current_file_sync_was_missed` | **PASSES** targeted. Real-tsserver flake. |
| `verter_type_runtime resilient::resilient_tests::failed_respawn_retries_within_budget_and_recovers` | **PASSES** targeted. Respawn-budget timing flake. |
| `verter_processor_broker tests::real_worker_enforces_exact_and_plus_one_dependency_and_output_bounds` | Fails targeted too, but `verter_processor_broker`'s dependency list is `sha2` + `snow` + `libc`/`windows-sys` ONLY — it does not depend on `verter_session` or `verter_workspace`, so it is STRUCTURALLY unreachable from this train. |
| `verter_tsc::main cases::fallthrough::fallthrough_attrs_accepted_only_where_they_reach_the_dom` | Fails targeted too, and `verter_tsc` DOES depend on both changed crates — so it was checked directly against the pre-change baseline `a4adb1f10`, where it fails IDENTICALLY: `RejectUnknownAttrOnAliasImportedChild.vue: expected an error and got none`. PRE-EXISTING. |

Two process notes worth carrying:

* A first gate run aborted with 7 SIGTERMs and 13,939 tests never run,
  because workspace `cargo clippy` was started while the gate was already
  running. Do not run cargo concurrently with the gate; the verdict is
  worthless and the archive build stretched from 9s to 757s.
* The five flakes above all pass on a targeted re-run. Judging this gate
  needs the per-test disposition, not the verdict line — the verdict is
  FAIL and correctly so, but it is FAIL at the baseline too.

---

## Step 3 outcome — the basis seam is installed and both ends are O(1)

Steps 1, 2 and **3** are LANDED and green. Steps 4–7 are NOT started.
The invariant "no domain is ever both uncompacted and unbounded" still
holds: a seeded basis supplies NO view population, so the four
view-derived domains still mint nothing and the legacy
`FACT_SIGNATURE_CAP` refusal still covers every domain.

### The shape that landed

* `AggregateBasisSeed` (`verter_workspace/src/fact_cache.rs`) — the
  view-derived half of a basis: `Unvouched`, or `Vouched { semantic_imports,
  route_surface }` carrying the two COMPOSITE stamps as the seeding view
  captured them (including the SemanticImports resolution-root component).
* `LiveAggregateCounters` — the live half: five atomic loads, read through
  the new `VerterHost::live_aggregate_counters()`.
* `AggregateGenerations::from_seed(seed, live)` — the ONE composition.
  Scalars live, composites captured-with-own-clock-live, `resolution: None`,
  `view_population: None`.
* `StoreView::aggregate_basis_seed()` — defaulted to `Unvouched`, overridden
  by `HostStoreView` (its two stamp derivations) and `RequestStoreView`
  (delegates the base, `Unvouched` when the base is not current).
* `FactTracerBasisSource` (`fact_signature_helpers.rs`) — `{host, seed}`,
  constructed `from_ctx` / `from_optional_ctx` / `unbound`. Owns
  `live_basis()`, and the `with_fact_tracer{,_cell}` forwards.
* `VerterHost::with_fact_tracer{,_cell}(seed, f)` — the chokepoint, which
  now calls `set_aggregate_basis` on the newly-created cell BEFORE `f` runs.
* `CacheabilityProbe` retains the SOURCE (not the host) for mid-scope
  rechecks.

**Deleted:** `HostStoreView::aggregate_basis_for_stability`,
`route_surface_stamp_from_live_clock`,
`semantic_imports_stamp_from_live_membership`,
`fact_signature_helpers::live_aggregate_basis`, and the eight now-dead
`let host = ctx.host_for_fact_tracer_install();` bindings the source
replaced. No dual path: there is no host-only tracer entry left.

### CORRECTION — the brief's "sole live basis composer" is FALSE

The brief and the Rail-A ruling both state that
`aggregate_basis_for_stability` "is the sole live basis composer" and that
compaction goes live "exactly when you supply a population". Contradicted
by `verter_workspace/src/resolution_currency.rs:1966-1973`:
`ResolutionTransaction::new` composes a SECOND production basis
(`resolution: root.resolution_stamp(root.population)`), and Resolution's
bucket population comes from its precise facts' OWN KEYS
(`fact_read_set.rs::aggregate_population`), not from `view_population`. So
`ResolutionTransaction::finish` can mint a `DomainGeneration(Resolution)`
TODAY, with no view population anywhere — it simply needs a transaction
whose Resolution bucket exceeds `FACT_DOMAIN_PRECISE_MAX`, which one
transaction's own observations rarely do but an `absorb`-accumulating one
can. The errata's own narrower statement at "Open (verifier's item 2)" —
"the four VIEW-DERIVED domains cannot mint an aggregate at all today" — is
the accurate one; the brief generalised it incorrectly. **Step 6 should not
be planned as "arm Resolution for the first time".**

### HAZARD RESOLVED — seeding through `ctx.store_view()` panics in production

The Rail-A ruling's Q2 says `ctx.store_view()` "is a BORROW of the
already-bound request view, not a new store-view read". True for
`HostResolverContext` and `SessionResolverContext`. FALSE for the third
implementer: the bare `impl ResolverContext for VerterHost`
(`resolver_context.rs:916`) owns no view — in test builds it `Box::leak`s a
freshly-read owned `HostStoreView` (a real store-view read AND a leak per
call), and in production builds it **panics**. The tracer chokepoint runs on
every cold compute, including the ones still reached through a bare host
(`host_manage/fallthrough.rs`, `intrinsic_projection.rs`, `eval_env.rs`,
`jsdoc_resolve.rs` construct `ComponentMetaQueryEngine::new(self)`), so
seeding through `store_view()` would have turned that architectural guard
into a live crash.

MEASURED before the fix: seven of the nine named O(1) tests failed with the
`store_view()` seed and pass with the context-level projection.

The fix stays inside the fence — `impl ResolverContext for VerterHost` is
UNTOUCHED. `ResolverContext::aggregate_basis_seed()` is a new DEFAULTED
trait method (default `Unvouched`), overridden by the two request-bound
implementers to forward the view they already hold. The bare-host impl
inherits the fail-safe default.

### ADOPT-NOW — movement detection is scoped to MINTABLE domains

`note_basis_recheck` short-circuits on `names_any_domain`, and
`any_named_domain_moved` examined every domain the basis held a STAMP for.
That was inert while nothing installed a basis. Installing one made it a
live over-refusal: a seeded basis carries four view-derived stamps it can
never mint from, and `SemanticImports` advances on every resolved-import
admission — i.e. inside essentially every cold compute. MEASURED: with the
broad predicate, `external_module_augmentation_broken_lease_contributor_folds_cache_suppress`
failed because a CLEAN augmentation fold set `cache_suppress`.

`AggregateGenerations::can_mint(domain)` now shares ONE predicate with
`compact_domains` (stamp present AND a population available for that
domain's buckets: from the fact keys for `Resolution`, unconditional `Base`
for `WorkspaceShape`, `view_population` for the other four).
`names_any_domain` and `any_named_domain_moved` both go through it.

This is a fix to the documented contract, not a weakening of it: both
methods already said "a domain this scope COMPACTS AGAINST", and a domain
with no population is one `compact_domains` explicitly leaves precise. The
`MU-1` terminality ruling (Rule B) is unchanged. The consequence for step 4
is direct: **supplying the `RequestCompletion` population is what arms
movement detection for the four view-derived domains**, so step 4 must
expect the `SemanticImports`-advances-inside-cold-computes problem to become
live and must answer it (a self-caused advance is still an advance — the
errata's "no self-exemption" rule stands, so the answer is probably that a
scope which mints a SemanticImports aggregate is genuinely unstable and the
domain should be armed LAST, not that the rule should bend).

### The nine O(1) tests, and the recheck path SEPARATELY

All nine pass at `b5d670617`. But they cannot see the recheck path even now
(they exercise flows that do not seed a vouched basis), so the recheck's
O(1)-ness is established by its OWN oracle rather than inherited from them:
`mutation_stability_tests::a_seeded_scope_reads_no_store_view_at_install_or_at_any_admission_boundary`
opens a request-bound scope, asserts it IS seeded (else the assertion is
vacuous), consults the probe at eight admission boundaries, and asserts the
host's `store_view_from_host_reads` delta is exactly `0`. Under the plant
`let _ = self.host.resolver_store_view_read();` in `live_basis` it reports
`9` — one install plus eight boundaries — which is the shape the hazard
predicted.

### Handoff — what step 4 inherits

* **The population goes in the seed.** `AggregateBasisSeed::Vouched` has two
  fields today; step 4 adds the `RequestCompletion` population as a third,
  and `from_seed` stops hard-coding `view_population: None`. Both
  `StoreView` overrides are already in place — `HostStoreView` answers
  `Base`/`SessionOverlay` from its `view_population()`, and
  `RequestStoreView` is the one that must stop delegating and answer with
  its own overlay identity + revision.
* **The step-3 fixture that inverts** is
  `fact_cache::aggregate_basis_seed_tests::a_seeded_basis_supplies_no_view_population`.
  It is written to be inverted, not deleted.
* **`StoreView::validate_fact_signature`** is still the single entry for the
  whole-signature snapshot/lease, unchanged by step 3.
* **Do not assume publication advances anything.** Per the parallel
  Decision-DAG train: a published decision mints no version (removal mints
  the tombstone), observable advance rides
  `current_resolution_fact_generation` as a counter distinct from the mint
  counter, and `CapturedResolutionWorld` now REFUSES a fact whose population
  it has no authority over instead of settling it with `INITIAL`. Step 4's
  population rechecks and step 5's `StrictSelfRootWorld` identity must be
  designed on refusal, not on `INITIAL` settling.
* **`ResolutionTransaction::absorb` is deleted** in that train (plan Legacy
  Deletion #10), superseding the `ResolutionOnlySignature` newtype from
  `a4adb1f10`. Do not extend either. Expect a conflict at `engine.rs:2660`,
  resolved in favour of the DAG version.
* **Six fixtures are step 7's disposition, not step 3's.** The DAG makes the
  resolution witness one decision fact per resolver query. The
  declaration-companion shape drives two queries per authored specifier, so
  its 180 imports produce 360 decisions; the fixtures still no longer exceed
  `FACT_SIGNATURE_CAP` at their authored specifier counts:
  `resolution_signature_growth_tests::{over_cap_positive_chunk_owner_is_refused_and_never_warms,
  over_cap_positive_chunk_owner_recomputes_component_meta_every_pass,
  an_unrootable_witness_leaves_no_warm_bundle_candidate,
  component_meta_admits_nothing_for_an_owner_with_an_unrootable_witness}` and
  `cold_artifact_dedup_tests::{unrootable_wildcard_route_raises_enclosing_cold_compute_suppression,
  unrooted_import_skip_raises_enclosing_cold_compute_suppression}`. Rescaling
  is CLOSED OFF (measured: ~560 specifiers takes 589 s, past nextest's
  timeout). Invert the two growth tests onto the ratified 0B table; restage
  the unrootable arms on whatever refusal trigger survives 1B, or delete them
  with the reasoning recorded if none does.

### Which production sites are BOUND and which are UNBOUND

`from_ctx` at 29 sites; `from_optional_ctx` at the three `script_facts`
scopes; `unbound` at SIX that genuinely hold no context:
`route_db.rs::get_or_build_barrel_surface`,
`owner_import_surface.rs::get_or_compute`,
`virtual_file_pipeline.rs::ensure_compile_artifacts`,
`component_meta_request_impl.rs` (both `compute_component_meta` impls), and
`component_meta_result_db.rs::compute_and_admit`. Those six compact nothing
and detect nothing — the state every tracer was in before step 3, so no
regression, but they are also the sites step 6 cannot arm. Giving them a
context is a prerequisite for arming a domain they must participate in; the
Rail-A ruling already names the fix for two of them ("the component-meta
result APIs must ADD the context/view parameter their callers already hold
but discard before `compute_and_admit`").

### The step-3 gate, and a cleaner baseline than the seven-item one

`node scripts/gate.mjs` at `d472be41a`, run with nothing else on the
machine: **23016 tests run, 23015 passed, 574 skipped, 1 failed.**
Surface 2 (the three `verter_session` libtest binaries executed in-process
from the same archive): 3 suites clean, 0 tolerated failures. The
build-prerequisite preflight reported SATISFIED and the freshness-tooling
preflight reported `already-present — tolerance DISABLED`, so the
`typeinfo_proto_ts_freshness` byte-pin ran genuinely and passed.

The one failure is
`verter_tsc::main cases::fallthrough::fallthrough_attrs_accepted_only_where_they_reach_the_dom`
— item 7 of the predecessor's seven-item baseline, already verified failing
IDENTICALLY at the pre-change tip `a4adb1f10`. Nothing new.

The other six baseline items PASSED this run. Five of them were load-induced
(the Block-6 wall-clock flake, `hot_materialize_scanner`, two real-tsserver
tests, the respawn-budget test) and the machine was quiet;
`verter_processor_broker` passed too. **The honest baseline for a quiet
machine is therefore ONE pre-existing failure, not seven** — the seven-item
list was measured under contention and five of its entries are machine load,
not tree state. A future run should treat any non-pass beyond the
`verter_tsc` row as suspect rather than assuming the six are free.

Workspace `cargo clippy --all-targets -- -D warnings` and
`cargo fmt --all --check`: clean.

---

## Step 4 — slice 4.1 LANDED; 4.2 is where the next implementer starts

Steps 1, 2, 3 and **step-4 slice 4.1** are landed and green. `verter_workspace
--lib` 747/0; `verter_session --lib` 4824/1 (the 1 is the Block-6 wall-clock
flake); workspace check clean.

### What 4.1 landed

`ViewPopulation::RequestCompletion { parent, overlay_id, revision }` plus
`ViewPopulationParent`, `CompletionOverlayState {Empty, Shadowing, InFlight}`,
`OverlayId::fresh()`, and the ONE projection
`ViewPopulation::refined_by_completion(parent, state) -> Option<Self>`.
Pure data in `verter_workspace/src/fact_cache.rs`, seven discriminating
tests, three executed §1a recipes.

Three rules the type encodes, each proven by an executed plant:

* **Empty ⇒ PARENT.** Plant `Empty ⇒ RequestCompletion{..}` reddens
  `an_empty_overlay_projects_to_its_parent_population` +
  `two_distinct_empty_overlays_share_one_population`.
* **InFlight ⇒ NO population.** Plant `InFlight ⇒ Some(parent)` reddens
  `an_in_flight_overlay_names_no_population_rather_than_its_parent` +
  `a_completion_population_arms_the_view_derived_domains`.
* **All three components discriminate.** Plant dropping `overlay_id` reddens
  `the_identity_discriminates_on_parent_overlay_and_revision` +
  `a_shadowing_overlay_is_a_distinct_population_from_its_parent`.

### The recorded refusal this reverses, and why the reversal is sound

`fact_cache.rs`'s `ViewPopulation` doc previously stated the variant was
absent ON PURPOSE, because a completion overlay "is append-only within a
request, so it has no identity that is stable for the life of a scope". The
doc is now REPLACED, not extended, and the reason is recorded there: the
premise is half right. The overlay's KEY SET only grows, but a key's VALUE
is replaced — `request_store_view.rs:553` / `:604` / `:290` all overwrite,
and a retried `run_stable_request` re-completes the same canonical with a
different whole hash producing no key-set growth. So a key-derived identity
was never available; an identity of the exact SHADOWING STATE is, and a
scope whose overlay moves under it is MOVEMENT, which the basis re-check
already reports.

**A revision must therefore be write-driven, not key-count-driven.**

### 4.2 — the next slice, and the ordering hazard that shapes it

Not started. Required, in this order:

1. **Overlay identity + revision.** `CanonicalCompletionOverlay`
   (`request_store_view.rs:117`) is identity-less today. Mint an `OverlayId`
   in `new()` (one edit — every one of its ~30 production construction sites
   goes through it) and add a `BracketedGeneration`
   (`resolver_core/bracketed_generation.rs:53`) for the revision. Its
   `mutate(|| (R, changed))` shape is an exact fit: `changed` is precisely
   "the EFFECTIVE shadowing changed", and `stable() -> Option<u64>` returning
   `None` mid-bracket is exactly `CompletionOverlayState::InFlight`. Note
   `mutate` ADVANCES on unwind (`:95-100`), which is the correct fail-safe
   here.
2. **An emptiness accessor.** The four `_nonempty: AtomicBool` flags already
   exist (`:149-151`, `:195`) and are written under the same write lock as
   their insert, so this is O(1) with no map walk — but they are private and
   there is no accessor. **Decide what "empty" means and record it:** the
   `overlay_bundle_memo` is a pure reuse memo that never enters `validates*`,
   so a shadowing-state predicate should read `whole_hashes ∪ derived_hashes
   ∪ file_facts` and NOT the memo. Getting this wrong makes a memo-only
   overlay claim a distinct population and lose all cross-request reuse.
3. **`HostStoreView::view_population` is a PRIVATE INHERENT method**
   (`resolver_store.rs:2518`, inside `impl HostStoreView` opened at `:1845`).
   `RequestStoreView` is in another module and cannot reach the parent
   population today. Widen it to `pub(crate)`.
4. **Seed carries the population.** `AggregateBasisSeed::Vouched` gains a
   third field; `RequestStoreView::aggregate_basis_seed`
   (`request_store_view.rs:834`) stops delegating the base's seed wholesale
   and answers with its OWN population via `refined_by_completion`.
5. **Validate side BEFORE mint side.** `RequestStoreView` currently refuses
   all four view-derived aggregates unconditionally (`:951-962`) and has NO
   `validate_fact_signature` override. Land the population-aware acceptance
   there first — a mint side that produces aggregates nothing accepts is
   strictly worse than no mint side. The trait doc at
   `resolver_core/mod.rs:359-369` names this exact case ("a whole-signature
   overlay snapshot or lease") as the reason the method is overridable, and
   the whole-signature snapshot must be taken ONCE per signature because a
   writer can straddle a per-fact loop.
6. **Only then arm.** `AggregateGenerations::from_seed` hard-sets
   `view_population: None` (`fact_cache.rs`, the row pinned by
   `a_seeded_basis_supplies_no_view_population`). Supplying it is what arms
   the four view-derived domains.

**The hazard, stated plainly.** Step 3 scoped movement detection to MINTABLE
domains, so arming the population arms movement detection for
`Content`/`SourceEnv`/`SemanticImports`/`RouteSurface` in the same stroke.
`SemanticImports` advances on every resolved-import admission — inside
essentially every cold compute. Expect the `MU-1` seam to start refusing
those computes' admission the moment step 4.6 lands, and do NOT answer it by
bending the no-self-exemption rule: a scope that MINTS a SemanticImports
aggregate while the domain moves under it is genuinely unstable. The
available answers are to arm the domains one at a time (Content first,
SemanticImports last), or to establish that a cold compute's own admission
should not advance the observable clock — which is the same shape as the
Decision-DAG train's B1 replacement, where publication was made to mint
nothing and observable advance moved to a separate counter.

**Two sites argue against 4.2 in prose and must be answered there, not
silently overwritten:** `request_store_view.rs:920-922` rejects "the overlay
is empty right now" as a basis for delegating `Content`, which is exactly
what the empty⇒parent rule relies on; and `:948-950` records the refusal and
the missing population as a MATCHED PAIR, so changing one without the other
breaks the pairing.

**Two further gaps 4.2 does not close and must not pretend to:**

* **FIFO eviction cannot protect durable candidates.** `Candidate`
  (`resolver_core/mod.rs:1270-1274`) carries only
  `signature_fingerprint`/`value`/`fact_dep_signature`; eviction is
  unconditional oldest-first (`:1638-1643`, `CANDIDATE_CAP = 4`). The
  population lives only inside `fact_dep_signature`, which the evictor never
  inspects. "Request-specific candidates must not FIFO-evict durable
  Base/Session candidates" therefore needs a new candidate field or a
  partitioned slot — it is not a policy tweak.
* **`OverlayIdentity`'s docs already claim completion-overlay coverage it
  does not have.** `resolver_store.rs:544-551` and `:560-561` describe
  distinguishing "two requests with DIFFERENT completion overlays", but the
  only producer is `:2145` inside `with_session_overlay` with
  `session_id: self.session_id`. If 4.2 mints a real overlay identity, decide
  explicitly whether it folds into `StoreViewValidationToken` — and note
  `complete_canonical_inner:449-458` currently normalises `overlay_identity`
  OUT of its supersession check.

### The final gate, at the landed tip

`node scripts/gate.mjs` at `2639b5e7e` (steps 1–3 + slice 4.1), nothing else
running: **23023 tests run, 23021 passed, 574 skipped, 2 failed.** Surface 2:
3 suites clean, 0 tolerated failures. Both preflights satisfied, so the
`typeinfo_proto_ts_freshness` byte-pin ran genuinely and passed.

Both failures are baseline items, neither new:

| Test | Disposition |
|---|---|
| `verter_tsc::main cases::fallthrough::fallthrough_attrs_accepted_only_where_they_reach_the_dom` | Baseline item 7. Verified failing IDENTICALLY at the pre-change tip `a4adb1f10`. |
| `verter_session meta_resolve::slot_binding_graph_tests::cold_synthesis_terminates_within_500ms_for_50_member_heritage` | Baseline item 1, the known Block-6 wall-clock flake. Passed on the `d472be41a` run of the same tree-modulo-4.1 and failed here; that is the flake behaving as characterised, not a 4.1 effect (4.1 adds only new types and tests to `verter_workspace` and changes no session code path). |

Test count moved 23016 → 23023: the seven `request_completion_population_tests`.
Workspace `cargo clippy --all-targets -- -D warnings` and `cargo fmt --all --check`: clean.

### Carried into step 5 — do not scope `StrictSelfRootWorld` to anything snapshot-shaped

From the context-memo train: the Q3 owner ruling beats plan B6 because an
LSP view-only rebuild recomposes `PublishedRoot`-level hashes while REUSING
the `WorkspaceSnapshot` `Arc`. A structure scoped to the snapshot can
therefore outlive its own inputs and keep answering with hashes that were
recomposed under it.

`StrictSelfRootWorld`'s identity must be checked against that exact failure
mode before it is committed to. It is a live risk rather than a
hypothetical one, because the strict artifact-only self-root path consults
`derived_raw_cache` presence and `file_exists` — both of which a view-only
rebuild can change while the snapshot `Arc` is unchanged. This is a second,
independent reason for the brief's standing instruction to default to a
DEDICATED monotonic authority generation and to drop it only on an
exhaustive coverage proof.

### Process notes for whoever runs this next

* **Run the canonical gate ONCE, at the end of the train.** Targeted runs
  (`cargo test -p verter_workspace --lib`, `-p verter_session --lib`,
  `--test main`, plus the nine named O(1) tests) are the iteration evidence.
  Two mid-train gate cycles were spent here that targeted runs would have
  covered.
* **Block on the gate in the FOREGROUND.** Backgrounding the wait yields the
  session and costs a full resume round-trip.
* The standing baseline is now TWO failures, both pre-existing: the
  `verter_tsc` fallthrough test and the Block-6 wall-clock flake. Anything
  else is new.

---

## Step 4.2 implementation handoff — code complete and green, commit blocked by sandbox

The implementation is present in the worktree but is **not committed**. The
linked-worktree Git metadata is read-only in this execution environment:
`git commit -m "feat(core): arm request content population"` failed while
creating `<parent-repo>/.git/worktrees/rc-cutover/index.lock`
with `Operation not permitted`. No escalation path is available. Step 5 was
therefore not started; doing so would violate the required committed-green step
boundary.

### What 4.2 implements

* `CanonicalCompletionOverlay` owns a process-unique `OverlayId` and one
  `BracketedGeneration` revision. A logical promotion across whole hash, file
  facts and route hash runs inside one bracket. Equal replacement preserves the
  revision; changed effective shadowing advances it; an open writer projects to
  `CompletionOverlayState::InFlight`.
* Empty means exactly `whole_hashes ∪ derived_hashes ∪ file_facts` is empty.
  `overlay_bundle_memo` is excluded because it never participates in fact
  validation. The memo-only acceptance pins this choice.
* `HostStoreView::view_population` is `pub(crate)`. `RequestStoreView` refines
  that durable Base/Session parent once through
  `ViewPopulation::refined_by_completion`.
* The validate side landed before the mint side. Direct fact validation samples
  and rechecks one completion state. `validate_fact_signature` leases one state
  for the entire signature, preserves strict self-root routing, and refuses a
  population straddle.
* `AggregateBasisSeed::Vouched` carries the population and an explicit
  `ViewAggregateDomains` participation set. `AggregateGenerations::from_seed`
  now forwards the population. Content is the only view-derived domain enabled
  at this boundary; `SourceEnv`, `SemanticImports`, and `RouteSurface` remain
  precise. This deliberately chooses the errata's "arm one domain at a time"
  answer. It avoids turning every resolved-import publication into a
  `MutationUnstable` cold compute before that domain's observable clock is
  repaired. No self-exemption was added.
* The two stale arguments in `request_store_view.rs` were answered in place.
  Empty is a durable parent projection only for the leased stable revision; a
  shadowing revision gets a distinct population. Validator acceptance and seed
  population now move as the matched pair the old prose required.
* Completion overlay identity intentionally does **not** enter
  `StoreViewValidationToken::overlay_identity`. That token remains the frozen
  Base/Session compatibility lane; completion state can advance within the
  request and lives on the fact-signature population rail. The formerly false
  `OverlayIdentity` completion-overlay claim was corrected.
* The FIFO gap remains open and is stated in `request_store_view.rs`: generic
  candidate eviction sees only the signature payload and cannot prefer durable
  Base/Session candidates over request-completion candidates.

### Acceptance evidence

| Acceptance | Discriminating test |
|---|---|
| Q1 / RC-4 — empty inherits parent; shadowing forks | `a_request_store_view_refines_content_validation_only_after_it_shadows`; `completion_overlay_state_tracks_effective_shadowing_identity` |
| Q1 — in-flight names no population | `completion_overlay_state_is_unavailable_during_a_writer` |
| Q1 — one population lease per signature | `request_signature_validation_refuses_a_population_straddle` |
| 1A-iii mint side — seed carries exact population | `request_basis_arms_content_in_the_exact_completion_population`; `a_seeded_basis_supplies_its_view_population` |
| MU-1 hazard decision — Content only | `movement_outside_the_scopes_domain_participation_leaves_it_admissible`; `request_basis_arms_content_in_the_exact_completion_population` |
| memo exclusion | `memo_only_population_remains_empty` |
| completion-map publication protocol | `completion_writers_publish_presence_under_the_map_lock_before_insertion` |
| anti-poisoning remains independent of cardinality | `non_cacheable_materialization_is_not_memoized` |

Verification on the restored tree:

* `cargo test -p verter_workspace --tests`: 747 lib + 5 integration passed.
* `cargo test -p verter_session --tests`: 4830 lib passed / 533 ignored;
  7 auxiliary passed; 2397 main passed / 24 ignored; zero failures.
* `cargo clippy -p verter_workspace -p verter_session --all-targets -- -D warnings`:
  passed.
* `cargo fmt --all --check` and `git diff --check`: passed.

### Executed §1a mutation recipes

Every row below was planted against the completed tree, counted with Ruby over
the whole file as one exact substring (not line-oriented grep), driven RED,
restored from a `/tmp` backup, recounted to prove zero residue, and rerun GREEN.
The unrelated control was run while the plant remained present.

| Invariant | Exact plant and final location | Count before → planted → restored | RED test | Planted control |
|---|---|---|---|---|
| presence flag is published under the map lock | `self.whole_hashes_nonempty.store(true, Ordering::Release);` → `self.whole_hashes_nonempty.store(false, Ordering::Release);` at `request_store_view.rs:328` | old/new `1/0 → 0/1 → 1/0` | `completion_writers_publish_presence_under_the_map_lock_before_insertion` | `request_store_view_returns_overlay_derived_hash_when_base_absent` GREEN |
| revision advances only on effective change | `whole.insert(canonical.to_owned(), whole_hash) != Some(whole_hash)` → `... == Some(whole_hash)` at `request_store_view.rs:337` | `1/0 → 0/1 → 1/0` | `completion_overlay_state_tracks_effective_shadowing_identity` | `completion_overlay_state_is_unavailable_during_a_writer` GREEN |
| one population for the whole signature | `if self.overlay.completion_state() != state {` → `if false {` at `request_store_view.rs:1127` | `1/0 → 0/1 → 1/0` | `request_signature_validation_refuses_a_population_straddle` | `a_request_store_view_refines_content_validation_only_after_it_shadows` GREEN |
| shadowing never delegates as parent | `ViewPopulation::refined_by_completion(parent, state)` → `Some(parent.into())` at `request_store_view.rs:944` | `1/0 → 0/1 → 1/0` | `a_request_store_view_refines_content_validation_only_after_it_shadows` | `a_content_aggregate_validates_only_at_the_views_captured_generation` GREEN |
| mint side forwards captured population | `view_population: *view_population,` → `view_population: None, // mutation-control: disarm captured population` at `fact_cache.rs:611` | `1/0 → 0/1 → 1/0` | `request_basis_arms_content_in_the_exact_completion_population` | `an_unvouched_seed_names_no_domain_even_with_readable_live_counters` GREEN |
| only Content participates at this boundary | `view_domains: verter_workspace::ViewAggregateDomains::CONTENT,` → `...::ALL, // mutation-control: arm every view domain` at `resolver_store.rs:3201` | `1/0 → 0/1 → 1/0` | `request_basis_arms_content_in_the_exact_completion_population` | `a_seeded_basis_supplies_its_view_population` GREEN |
| memo does not define shadowing | `|| self.file_facts_nonempty.load(Ordering::Acquire);` → same expression OR `overlay_bundle_memo_nonempty` at `request_store_view.rs:286` | `1/0 → 0/1 → 1/0` | `memo_only_population_remains_empty` | `completion_overlay_state_tracks_effective_shadowing_identity` GREEN |

### Contradiction resolved during 4.2

The inherited prose and its source-scanning guard claimed that inserting into a
map and then setting `_nonempty = true` while still holding the write lock made
`false` imply "not inserted". That is false: the fast reader skips the lock and
can run between insert and store. The safe order is acquire write lock, publish
`true`, then insert; a reader seeing `true` blocks until insertion completes.
All three validation-visible maps now use that order. The name-keyed source
scanner was deleted and replaced by a runtime structural verifier covering the
flag state and held write lock for each map.

### Next handoff

1. Obtain a writable linked-worktree Git index and commit the ten source/test
   files plus this errata update; do not start step 5 before that commit is
   green.
2. Start step 5(a) with a dedicated monotonic `StrictSelfRootWorld` authority
   generation owned no lower than `PublishedRoot`-lifetime inputs. Do not scope
   it to the reused `WorkspaceSnapshot` Arc.
3. Separately implement step 5(b)'s post-merge cap for the semantic-graph and
   ref-cycle carriers. Materialisation still does not consume a finalised read
   set and must not be treated as a `DomainGeneration` carrier.

---

## Step 5 implementation handoff — complete at the committed boundary

Step 5 is complete. Steps 6 and 7 were deliberately not started: this was the
next safe green boundary, and the inherited instruction explicitly prefers a
committed Step 5 over a rushed partial Step 6/7.

### What landed

* `StrictSelfRootWorld` is a distinct terminal fact, not a generic Content
  aggregate. Its exact identity is `(authority_id, authority_generation,
  source_epoch, artifact_epoch, population)`. `authority_id` is
  process-unique per workspace engine, so equal local counters from two
  workspace instances cannot alias after `set_workspace`. The population is
  the exact base/session/request-completion population, including completion
  overlay id and revision.
* The dedicated authority uses an active-writer count plus a monotonic
  generation. Begin advances before mutation; end advances before releasing
  the active writer. Minting samples the identity before and after strictly
  validating **every** root and refuses while any writer is active. This closes
  overlapping writers and intermediate-state aliasing.
* Authority movement is attached to actual strict-answer inputs: PublishedRoot
  replacement/rebuild (including reuse of the same `WorkspaceSnapshot` Arc),
  workspace content/overlay/subtree mutations, and derived-state membership
  insertion/removal. Generic resolution-world publication and coarse
  store-view/load epochs do not move it; doing so would invalidate strict
  witnesses during resolution evidence refresh and dependency-edge recording.
* Scheduler and artifact epochs remain explicit identity dimensions. An
  artifact-only root is witnessable only when its workspace reports a complete
  event bridge. The native filesystem's raw `file_exists` fallback therefore
  fails closed; a root already present in the immutable scheduler/session
  roots remains witnessable.
* `RequestStoreView` mints and validates against one leased completion state.
  Same-population followers coalesce; an equal root set in a different
  completion overlay forks. `ReadSetSignature::has_view_discriminating_self_root`
  recognizes the strict witness; a Content `DomainGeneration` does not.
* `bound_completed_structural_carrier` is the single terminal bounding helper.
  It runs after self-root/fence/prelude/traced merging, deduplicates, strictly
  validates and collapses an oversized self-root tail, clears the precise root
  list, and refuses any remainder still above `FACT_SIGNATURE_CAP` with typed
  `SignatureOverflow`.
* All three completed-carrier publishers use that helper:
  `SemanticGraphStore`, `RefCycleResultDb`, and `MaterializeStructureDb`.
  Direct helper boundary tests and tests at each actual publication seam pin
  both exact-cap admission and above-cap refusal.

### Acceptance map

| Acceptance | Discriminating tests |
|---|---|
| RC5-A — every root is checked before mint | `oversized_self_root_compaction_strictly_checks_every_root`; bounded-carrier control `oversized_self_root_set_builds_a_bounded_carrier` |
| RC5-B — terminal witness is distinct and follower-discriminating | `strict_world_witness_discriminates_without_a_precise_root_list`; `generic_content_aggregate_is_not_a_strict_self_root_witness` |
| RC5-C — collision-free authority and transition fencing | `distinct_workspace_authorities_never_alias_strict_self_root_worlds`; `strict_self_root_world_is_unavailable_inside_an_authority_transition`; `derived_state_membership_advances_the_strict_self_root_world`; `occupied_derived_state_lookup_preserves_the_strict_self_root_world` |
| RC5-D — PublishedRoot scope, not snapshot scope | `published_root_replacement_advances_strict_authority_when_snapshot_arc_is_reused` |
| RC5-E — exact completion population | `strict_self_root_witness_reuses_only_the_exact_completion_population` |
| RC5-F — no resolution-only churn | `resolution_only_world_publication_preserves_strict_self_root_authority` |
| RC5-G — uncovered live filesystem fails closed | `uncovered_filesystem_presence_is_not_compacted_into_a_strict_world` |
| RC5-H — post-finalisation semantic bound | `post_finalise_self_root_merge_cannot_publish_above_the_cap`; `semantic_publication_refuses_a_post_finalise_over_cap_carrier`; `completed_structural_carrier_at_the_exact_cap_is_admitted` |
| RC5-I — post-finalisation materialisation bound | `materialize_completed_carrier_enforces_the_terminal_cap`; `materialize_publication_refuses_a_post_finalise_over_cap_carrier` |
| RC5-J — post-finalisation ref-cycle bound | `ref_cycle_completed_carrier_enforces_the_terminal_cap`; `ref_cycle_publication_refuses_a_post_finalise_over_cap_carrier` |

The first red phase was observed before implementation:
`oversized_self_root_set_builds_a_bounded_carrier` returned 1,025 facts, and
`post_finalise_self_root_merge_cannot_publish_above_the_cap` admitted the
completed over-cap carrier.

### Executed §1a mutation recipes

Every effective row below was counted as one whole-file exact substring (not
line-oriented grep), planted only after proving `old/new = 1/0`, recounted as
`0/1`, run RED with the plant present, restored from a `/tmp` backup, recounted
as `1/0`, and rerun GREEN. Each named control ran GREEN while the plant was
present.

| Invariant | Exact plant and final location | Count | RED test(s) | Planted control |
|---|---|---|---|---|
| all roots, not one root | `if !roots.iter().all(|(canonical, hash)| { ... }) {` → `if !roots.last().is_some_and(|(canonical, hash)| { ... }) {` at `resolver_core/mod.rs:311` | `1/0 → 0/1 → 1/0` | `oversized_self_root_compaction_strictly_checks_every_root` | `oversized_self_root_set_builds_a_bounded_carrier` |
| active writer refuses identity | exact line `            || workspace.strict_self_root_transition_active()` → omission at `store_view_roots.rs:616` | `1/0 → 0/1 → 1/0` | `strict_self_root_world_is_unavailable_inside_an_authority_transition` | `strict_self_root_witness_reuses_only_the_exact_completion_population` |
| completion population participates | `        world.population = self.population_for(state)?;` → `        world.population = verter_workspace::ViewPopulation::Base;` at `request_store_view.rs:1031` | `1/0 → 0/1 → 1/0` | `strict_self_root_witness_reuses_only_the_exact_completion_population` | `request_signature_validation_refuses_a_population_straddle` |
| strict witness discriminates followers | the unique `StrictSelfRootWorld` branch's `            return true;` → `            return false;` at `fact_signature_helpers.rs:1443` | `1/0 → 0/1 → 1/0` | `strict_world_witness_discriminates_without_a_precise_root_list` | `generic_content_aggregate_is_not_a_strict_self_root_witness` |
| collapse removes the precise root tail | `        self_root_canonicals.clear();` → `        self_root_canonicals.truncate(1);` at `fact_signature_helpers.rs:1300` | `1/0 → 0/1 → 1/0` | `oversized_self_root_set_builds_a_bounded_carrier` | `completed_structural_carrier_at_the_exact_cap_is_admitted` |
| final cap is load-bearing | `    if facts.len() > FACT_SIGNATURE_CAP {` → `    if false && facts.len() > FACT_SIGNATURE_CAP {` at `fact_signature_helpers.rs:1303` | `1/0 → 0/1 → 1/0` | semantic, materialisation, and ref-cycle completed-carrier cap tests | `completed_structural_carrier_at_the_exact_cap_is_admitted` |
| transition generation has no intermediate alias | exact begin/end block containing `self.bump_strict_self_root_generation();` in both methods → the same block with both bump lines omitted at `engine.rs:615-622` | `1/0 → 0/1 → 1/0` | PublishedRoot reuse and in-flight authority tests | `occupied_derived_state_lookup_preserves_the_strict_self_root_world` |
| vacant derived membership moves authority | exact `Entry::Vacant(entry) => { ... workspace.begin_strict_self_root_transition(); ... entry.insert(...) }` block → `Entry::Vacant(entry) => entry.insert(...)` at `host_construction.rs:1055` | `1/0 → 0/1 → 1/0` | `derived_state_membership_advances_the_strict_self_root_world` | `occupied_derived_state_lookup_preserves_the_strict_self_root_world` |
| uncovered filesystem is not witnessable | `            .is_some_and(|workspace| workspace.resolution_event_bridge_complete())` → `            .is_some()` at `store_view_roots.rs:655` | `1/0 → 0/1 → 1/0` | `uncovered_filesystem_presence_is_not_compacted_into_a_strict_world` | exact-completion-population test |
| semantic actual publisher uses terminal builder | exact `with_effective_store_view(ctx, |view| semantic_graph_read_set_signature(...))` block → typed `Ok((merged_facts, Arc::from([])))` bypass at `project_semantic_dispatch/mod.rs:2629` | `1/0 → 0/1 → 1/0` | `semantic_publication_refuses_a_post_finalise_over_cap_carrier` | direct semantic completed-carrier test |
| materialisation actual publisher uses terminal builder | exact `with_effective_store_view(ctx, |view| merge_traced_facts_into_materialize_carrier(...))` block → typed `Ok` of the original entry at `component_meta_materialize.rs:1673` | `1/0 → 0/1 → 1/0` | `materialize_publication_refuses_a_post_finalise_over_cap_carrier` | direct materialisation completed-carrier test |
| ref-cycle actual publisher uses terminal builder | exact `with_effective_store_view(ctx, |view| ref_cycle_read_set(...))` block → typed `Ok` of the unbounded roots/facts at `component_meta_caches.rs:3442` | `1/0 → 0/1 → 1/0` | `ref_cycle_publication_refuses_a_post_finalise_over_cap_carrier` | direct ref-cycle completed-carrier test |
| resolution-only publication does not churn authority | exact `let _write = self.resolution_world_write.lock();\n        self.mutate_resolution_world_locked(mutation)` → the same substring with `let _strict_transition = self.strict_self_root_transition();` inserted at `engine.rs:774` | `1/0 → 0/1 → 1/0` | `resolution_only_world_publication_preserves_strict_self_root_authority` | PublishedRoot reuse test |
| workspace authorities never alias | `            authority_id: workspace.strict_self_root_authority_id()?,` → `            authority_id: std::num::NonZeroU64::new(1).unwrap().get(),` at `store_view_roots.rs:622` | `1/0 → 0/1 → 1/0` | `distinct_workspace_authorities_never_alias_strict_self_root_worlds` | exact-completion-population test |

One attempted recipe was rejected rather than counted: the first derived-state
fixture called `set_import_dependencies`, whose resolution-side mutation also
moved the observed identity, so removing the vacant-entry bracket stayed
GREEN. The fixture was rewritten to call the vacant membership chokepoint
directly; the same plant then went RED and the occupied-entry control stayed
GREEN. This is the concrete false-green that the mutation protocol caught.

### Contradictions and STOP-worthy findings resolved

* The inherited brief and the previous handoff said materialisation did not
  participate in post-finalisation merging. Source contradicts that claim:
  `trace_materialize_compute` finalises a tracer and merges it into the entry at
  `component_meta_materialize.rs:1673`. Materialisation therefore received the
  same terminal cap as the other two carriers.
* A generation plus two local root epochs was not collision-free across two
  workspace instances. The pre-fix two-workspace acceptance produced identical
  tuples. The process-unique authority id closes that workspace-swap alias.
* Bracketing generic `mutate_resolution_world` made read-time resolution
  baseline/healing publications advance strict authority. The bracket was
  narrowed to actual strict-input mutations, with a positive PublishedRoot
  test and a resolution-only stability test.
* The first full session integration run found one pre-existing committed
  machine-specific path in this errata's old sandbox-failure handoff. It was
  introduced after the previous reported gate and contradicted the claim that
  that committed tip was green. The path is now repo-relative; the exact guard
  was observed RED then rerun GREEN.

### Step 5 verification

Verification on the restored, formatted tree:

* `cargo test -p verter_workspace --tests`: 749 library tests plus 5
  integration tests passed; zero failures.
* `cargo test -p verter_session --tests`: 4,847 library tests passed / 533
  ignored; 7 auxiliary tests passed; after the stale-path correction, the
  main integration binary passed 2,397 / ignored 24 with zero failures.
* The later representation-only boxing of `ParkDecision`'s rare
  `RefusedSelfAwait` payload was checked by
  `reentrant_base_view_on_the_claim_holding_thread_refuses_to_park`: 1 passed.
* `cargo clippy -p verter_workspace -p verter_session --all-targets -- -D warnings`,
  `cargo fmt --all --check`, and `git diff --check`: passed.
* The once-only whole-repository gate ran in the managed sandbox. Surface 1
  ran 23,047 tests: 22,902 passed, 145 failed, and 574 were skipped. All 145
  failures were bind/sandbox dependent: Unix-domain socket creation, HTTP
  readiness binds, or nested macOS sandbox initialization was denied. The
  standing fallthrough failure was among them but failed before its assertion
  because its TSGO socket could not bind; the standing slot-binding timing
  flake passed. Surface 2's three shared-process `verter_session` suites were
  clean. This is a gate FAIL and cannot be compared as the expected two-test
  code baseline in this restricted environment.
* `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`, and `cargo check --workspace --release`: passed.
* The pre-commit hook could not bootstrap `lint-staged`: the repository has a
  lint-staged configuration but no installed executable, and network access
  was unavailable. No JavaScript file was staged; the hook's applicable Rust
  action was run directly as `cargo fmt --all --check` before committing.

### Next handoff: steps 6 and 7

Step 6 has not been attempted. Start by arming the remaining domains one at a
time, then prove Rail A **per signature**. In particular, add the required
Resolution+Content mixed-signature refusal on `RequestStoreView`; do not infer
reusability from a domain in isolation. Verify both existing defensive fixes
against the actual property “the witness cannot be re-observed”, not
`aggregates_domain(Resolution)`: the Decision DAG integration can be
un-enumerable without carrying a `DomainGeneration`.

Step 7 has not been attempted. Migrate all six raw-insert/re-sign seams before
deleting any size refusal, using `owner_import_surface`'s re-finalisation shape.
Only then perform Legacy Deletions #1-#9 and disposition the six forward-DAG
fixtures as ratified above. No legacy refusal symbols or fixtures were deleted
in Step 5.

---

## Step 6 — stable domain arming and per-signature Rail A

Step 6 is landed. `HostStoreView::aggregate_basis_seed` now arms `Content` and
`SourceEnv`. `Resolution` was already armed independently by
`ResolutionTransaction::new`; the transaction tests below establish that the
live path compacts, admits, and does not regrow. `SemanticImports` remains
precise for the previously recorded self-supersession reason.

`RouteSurface` also remains precise. This was not an assumption: arming all
four view-derived domains made the clean arm of
`external_module_augmentation_broken_lease_contributor_folds_cache_suppress`
set `cache_suppress`. Isolating the participation bits made
`Content + SourceEnv` GREEN and `Content + RouteSurface` RED. The RouteSurface
clock therefore advances inside this cold fold, so arming it at the global
view-seed boundary would reject an otherwise clean compute. No self-exemption
was added.

### Rail A is per signature

The request view still accepts a Content-only aggregate in the exact leased
completion population; the older shorthand that it blanket-refuses Content is
stale after step 4. The new whole-signature rule is narrower and load-bearing:
a signature containing a `DomainGeneration(Resolution)` and any view-derived
aggregate (`Content`, `SourceEnv`, `SemanticImports`, or `RouteSurface`) is
refused before per-fact validation. The Resolution-only and Content-only
controls both validate; their union does not.

The revised 0B table deliberately says its first cache rows become acceptance
"after 1A-iii plus the relevant 1B gate migration". At this step boundary the
facts established are:

* `resolution_over_cap_lifts_its_own_domain_and_admits_no_foreign_one` proves
  the production transaction basis mints one bounded Resolution aggregate;
* `a_lifted_resolution_domain_does_not_regrow` proves a reused compacted
  transaction stays bounded;
* `resolution_currency_declaration_companion_positive_reuses_its_warm_candidate`
  proves the underlying resolution candidate does warm-hit;
* `request_signature_refuses_a_mixed_resolution_and_content_aggregate` has a
  Resolution-only control that proves the compacted signature is eligible on
  Rail A, and the mixed control proves eligibility is not inferred from one
  domain in isolation.

The named prepared-bundle and owner-surface cache-hit rows are not claimed at
this boundary: their raw/re-sign seams are exactly the 1B migration that must
re-finalise the whole candidate before those rows can invert without vacuity.

### Defensive evidence healing

Both inherited defenses were aggregate-keyed and were insufficient.
`ReadSetSignature::resolution_evidence_is_unenumerable` now states the property
the consumers require: the signature carries Resolution evidence but exposes
no path observation a live source can re-read, or it carries the terminal
Resolution aggregate. `Engine::refresh_resolution_evidence` uses that property
for its whole-pending-ledger fallback, and
`Engine::witness_evidence_is_unenumerable` uses the same property for the
`Uncovered` refusal. Both discriminating tests obtain a real `Decision`
witness by resolving through the Engine, assert that it is non-aggregate and
has zero re-observable path canonicals, then exercise their respective
pending-ledger fallback and uncovered-backend refusal.

### Acceptance map

| Acceptance | Discriminating tests |
|---|---|
| RC6-A — SourceEnv is armed in the exact population | `request_basis_arms_the_stable_view_domains_in_the_exact_completion_population` |
| RC6-B — self-superseding domains stay precise | the same basis test; `external_module_augmentation_broken_lease_contributor_folds_cache_suppress` |
| RC6-C — Rail A is per signature | `request_signature_refuses_a_mixed_resolution_and_content_aggregate` |
| RC6-D — un-enumerable is not aggregate-only | `a_nonaggregate_unenumerable_witness_still_reobserves_the_pending_ledger`; `an_uncovered_backend_refuses_a_compacted_witness_it_cannot_reobserve` |
| RC6-E — Resolution already compacts and reuses | `resolution_over_cap_lifts_its_own_domain_and_admits_no_foreign_one`; `a_lifted_resolution_domain_does_not_regrow`; `resolution_currency_declaration_companion_positive_reuses_its_warm_candidate` |
| RC6-F — basis rechecks remain O(1) | the nine named O(1) tests plus `a_seeded_scope_reads_no_store_view_at_install_or_at_any_admission_boundary` |

All nine required O(1) tests passed after the final participation set was live:
`output_batch_equals_scalar_and_is_o1_store_view_reads_when_warm`,
`warm_public_api_batch_from_host_calls_are_o1_not_per_item`,
`view_bound_cold_compute_seeds_from_executor_snapshot_not_a_second_read`, the
four `*_failed_revalidation_does_not_leak_live_counter` tests,
`declaration_lookup_straddling_compute_is_not_served_to_the_winner`, and
`external_module_augmentation_broken_lease_contributor_folds_cache_suppress`.
The dedicated eight-boundary oracle also passed with a store-view-read delta of
zero.

### Step 6 verification

Verification on the restored participation set:

* `cargo test -p verter_workspace --tests`: 750 library tests plus 5
  integration tests passed; zero failures (one new library acceptance over the
  step-5 baseline).
* `cargo test -p verter_session --lib`: 4,848 passed / 533 ignored; zero
  failures (one new library acceptance over the step-5 baseline).
* `cargo test -p verter_session --tests`: the same library result, 7 auxiliary
  tests passed, and the main integration binary passed 2,397 / ignored 24;
  zero failures.
* `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`, `cargo check --workspace --release`, and
  `git diff --check`: passed.

### Executed step-6 §1a mutation recipes

Every row used a whole-file exact-substring count, was planted only after
`old/new = 1/0`, recounted as `0/1`, run RED with the plant present, restored
from a `/tmp` backup, recounted as `1/0`, and rerun GREEN. The named control ran
GREEN while the plant was present.

| Invariant | Exact plant and final location | Count | RED test(s) | Planted control |
|---|---|---|---|---|
| SourceEnv participates | exact line `            view_domains: verter_workspace::ViewAggregateDomains::CONTENT_SOURCE_ENV,` → `            view_domains: verter_workspace::ViewAggregateDomains::CONTENT,` at `resolver_store.rs:3223` | `1/0 → 0/1 → 1/0` | `request_basis_arms_the_stable_view_domains_in_the_exact_completion_population` | population-straddle test |
| self-superseding domains remain precise | the same exact line → `            view_domains: verter_workspace::ViewAggregateDomains::ALL,` at `resolver_store.rs:3223` | `1/0 → 0/1 → 1/0` | `external_module_augmentation_broken_lease_contributor_folds_cache_suppress` | mixed-signature refusal test |
| mixed Rail-A signature refuses | exact line `        if carries_resolution_aggregate {` → `        if false && carries_resolution_aggregate {` at `request_store_view.rs:1132` | `1/0 → 0/1 → 1/0` | mixed-signature refusal test | population-straddle test |
| un-enumerable is the property, not aggregation | exact line `        carries_resolution_evidence && !carries_reobservable_path` → `        self.aggregates_domain(CompactionDomain::Resolution)` at `fact_cache.rs:1202` | `1/0 → 0/1 → 1/0` | both non-aggregate evidence tests | both compacted-witness controls |
| live recheck performs no store-view read | exact `live_basis` opening block gains `        let _ = self.host.resolver_store_view_read();` at `fact_signature_helpers.rs:248` | `1/0 → 0/1 → 1/0` | `a_seeded_scope_reads_no_store_view_at_install_or_at_any_admission_boundary` (observed delta `9`, expected `0`) | stable-domain basis test |

### Step 7 handoff

No 1B gate or legacy overflow symbol was changed in this slice. Migrate the six
raw/re-sign seams before deleting any length refusal. The revised 0B named
cache-hit rows remain the post-migration acceptance; do not reinterpret the
step-6 validator proof above as those cache-store proofs.

### Step 7 preflight — stop at the step-6 boundary

A Block 1B TDD probe was executed after the step-6 commit and then restored in
full. Inverting the two `resolution_signature_growth_tests` produced the
required RED (`owner_import_route_witness_for_tests` was still `None`). A
scratch implementation re-finalised that owner-wide union against one captured
resolution world, after first validating every supplied fact against that exact
world; both inverted growth tests then passed, including the two-fresh-empty-
`RequestStoreView` prepared-bundle reuse row. None of that probe is landed:
`git diff` was empty again before this record was written.

The probe exposed a sequencing conflict that makes the rest of 1B unsafe at
this boundary. `SemanticImports` and `RouteSurface` remain deliberately
precise. The former self-supersedes essentially every cold compute; the latter
was independently isolated in step 6 and makes
`external_module_augmentation_broken_lease_contributor_folds_cache_suppress`
RED when armed. Consequently an over-threshold bucket in either domain still
reaches `FactReadSet::finalise` as an over-cap precise set. Migrating the seven
named raw/re-sign seams does not change that fact for the other traced
signature consumers. Deleting `FactReadSetFinalise::Overflow` and the global
length gates now would therefore admit an unbounded, uncompacted signature —
exactly Block 1B's named vacuity and the binding rule that an unarmed domain
must retain its legacy bound.

The remaining design obligation is one of these, with discriminating coverage:

* make each self-superseding domain armable without treating its own
  publication as unexplained movement; or
* add a terminal, exact-view revalidation/rebase that proves every precise
  fact against the post-compute world before minting that world's aggregate;
  or
* prove and structurally enforce a sub-cap bound for every producer of both
  precise domains.

No such authority exists in the landed finalisation carrier today, and a
cardinality-triggered `NonCacheable` fallback would merely rename the refusal
that `SIG-1` requires deleting. Step 7 therefore was not partially committed.
Resume from the clean step-6 boundary only after settling this obligation;
then reapply the proven resolution-world validation shape, migrate every raw
seam, and only afterwards delete Legacy Deletions #1–#9.

The six forward-Decision-DAG fixtures remain unchanged because their Step 7
disposition was not reached. The `resolution_currency_spec_tests.rs` local
seven-family mirror was likewise not touched; it remains non-structural and
must be replaced rather than updated to another local enum mirror when the
nine-family DAG lands.
