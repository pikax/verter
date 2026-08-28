# B4 — landing record

Base `664cab091`. Candidate `84c676689`. Dispatch context:
[`context-packet.md`](context-packet.md).

## What shipped

- `crates/verter_compiler/src/assembly/` — typed `Fragment`/
  `ValidatedFragment`/`SourceUnit`/`SourceSpace`/`ProductPlan`/`compose`/
  `publish`. A fragment must parse under its own declared syntactic
  contract before a `ValidatedFragment` exists; the sequential composer
  used by the live Vue Main path accepts only validated fragment
  references (a real, structural, sealed gate — no bypass exists); atomic
  publication requires exact planned product cardinality, requires source
  projection maps exactly where the plan requires them, and re-parses the
  fully composed artifact under its own declared dialect (not a fixed
  permissive default) before it can be published.
- Vue main-module assembly (`assemble_vue_main_module`) now builds every
  scaffold/content piece as a real validated fragment and publishes them
  through this engine, replacing host-side text concatenation and its own
  generated-text scan. `MapComposer`/`ModuleWriter`/`FragmentWrite` are
  deleted.
- Two real production bugs found and fixed: a render/ssrRender export
  binding decided by scanning generated text for a landmark string
  (silently mis-binds if a template's body merely mentions the other
  binding's landmark string); a virtual-file import injection that left
  its accompanying source map silently pointing at the wrong generated
  position. Two further bugs found incidentally while wiring the fix and
  fixed alongside: a source-map decoder inconsistency between two
  independent JSON readers over dual-spelling ignore-list maps; a splice
  with no separator between two composed pieces that could produce
  invalid JavaScript.
- Disposition ledger (`emitter-mapping-dispositions.tsv`) rows EM-001/
  002/003/021/035/038/040 resolved from evidence, each with a one-line
  rationale; EM-021/035 corrected from the ratified draft's `Delete` to
  `Preserve` after verifying (via full call-graph grep) that
  `eval_source` blanking has zero production callers on the compile/
  publish path — its only live use is the pre-existing typeinfo/shallow/
  prepared-decl script view, out of scope under the program's types-waived
  rule.
- Three items recorded as tracked, dispositioned debt rather than
  attempted here, each with a concrete resolution gate:
  `debt-FC-B4-001-generated-chunk-callsite-migration.md` (three
  `vue_bridge.rs` call sites of an older splice utility whose spliced
  content's grammar guarantee isn't yet established),
  `debt-FC-B4-002-vue-main-module-composer-cutover.md` (RESOLVED — records
  the full cutover this candidate completed), and
  `debt-FC-B4-003-scaffold-text-import-fact-drift.md` (a narrow gap where
  the live undeclared-import check is structurally tautological at one
  call site). `finding-frozen-w13-superseded-by-render-export-fact.md`
  escalates a frozen conformance-vector expectation that pre-dates the
  binding fix — the fix is correct and the vector's own frozen document is
  outside this candidate's amendment authority to change; the affected
  vector is narrowly excluded from one assertion (still loaded and
  exercised everywhere else) pending that amendment.

## Review arc

Three rounds of all-three-seat review (codex conformance, grok
architecture, Claude-subagent adversarial-with-plant-prove-RED-GREEN in
its own worktree), two codex scope-boundary rulings, and one final
targeted-delta fix round — see context-packet.md for the arc. Every
adversarial claim across all three rounds was independently verified via
genuine plant → RED → revert → GREEN cycles, including finding a real,
demonstrable regression class (a marker pointer-identity resolution bug)
via an existing production test, not a constructed one.

## Discriminated as pre-existing / environmental, not regressions

- `resolver_store::store_view_o1_build_tests::store_view_build_wall_cost_is_flat_across_host_sizes`
  — zero diff in this candidate against `resolver_store`/`store_view` (no
  file in that path touched); passes cleanly in isolation
  (`cargo test -p verter_session --lib` filtered to this test); a
  wall-clock-ratio assertion, consistent with resource-contention flake
  under the gate's 8-thread parallel load, not a functional regression.
- `native_content_handoff::external_template_ide_compile_contains_selected_bytes`
  — independently reproduced identically against the base commit
  (`664cab091`) in an isolated worktree before attributing it here. Also
  the same failure B3's own landing record discriminates as pre-existing/
  load-dependent.
- `resilient::resilient_tests::failed_respawn_retries_within_budget_and_recovers`
  — documented program-wide known-flaky baseline (fails ~4/5 on base).

## Environment issue found and fixed as a standalone commit, not part of this candidate

`pnpm-lock.yaml`'s importer entry for `packages/vue-vscode` was missing
the `@verter/binary-launcher` workspace dependency already present in its
`package.json` — present at the base commit itself (reproduced identically
in a separate, untouched worktree at `664cab091`), blocking
`pnpm install --frozen-lockfile` and therefore the canonical gate's
build-prerequisite preflight repo-wide, for every train, not just this
one. Fixed via `pnpm install --no-frozen-lockfile` (3-line diff, exactly
the missing importer entry) and landed as standalone commit `05779b05f`
immediately before this candidate — kept separate rather than folded into
the B4 implementation commit since it is not B4-chartered work.

## Verification

- Canonical Rust gate (`node scripts/gate.mjs --test-threads 8
  --memory-limit 18GiB`) ran once, at landing readiness, after the
  environment fix above unblocked its build-prerequisite preflight.
  Terminal three-surface summary: Surface 1 (nextest, process isolation)
  24623 run / 24620 passed / 3 failed; Surface 2 (direct in-process
  `verter_session` libtests) 2 suites clean, 1 with the same
  already-discriminated failure; Surface 3 (shipped `no-debug-assertions`
  cfg, `verter_session`+`verter_scheduler`) 8678 run / 8677 passed / 1
  failed. All non-tolerated failures are the three discriminated above.
  The `typeinfo_proto_ts_freshness` byte-pin ran genuinely (tooling
  present, tolerance disabled) and passed cleanly — no freshness
  regression.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -D
  warnings` (touched crates), `cargo clippy --target wasm32-unknown-unknown
  -p verter_wasm -D warnings`, `cargo check --workspace --release` — all
  clean. The release-only `private_interfaces` warning found during this
  pass (two leaked marker-type fields) was fixed and independently
  re-verified as 0 occurrences.
- No TypeScript/JavaScript source changed in this candidate; `pnpm test`
  not required per the program's own end-of-change rule for a change
  confined to Rust crates.
