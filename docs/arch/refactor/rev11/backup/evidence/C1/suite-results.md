# C1 Step-6 suite-result references

Reviewed candidate: `c46c60c52f33784356a9f1d7fade31627486e874`, tree
`031c84419aaa1bc851c24e31add987c9ad678ba8`. Production subject:
`2820cf2eb790caffdb69f59bc20402d7d0a6647b`, tree
`ef8efbec06c8e87d1d6d72d9ea8e69fa624f515b`. The evidence/authority descendants change no
production, test, harness, corpus, toolchain, or performance configuration bytes.

The assembler ran no test, canonical-gate, or performance command. Current references are:

- final targeted and affected-closure runs before the last retention-lifetime correction:
  `final-review-round5-kernel-retention-fix.md`;
- the last production/test delta: commit `2820cf2e…`;
- exact-production enabled correctness/counter/digest and disabled wall/RSS evidence:
  `a6/final-round6-performance.md`;
- exact-candidate review receipts:
  `reviews/c46c60c52f33784356a9f1d7fade31627486e874/`.

The tables below retain the earlier exact-subject runs as historical evidence. They are not
restamped as executions on `2820cf2e…` or `c46c60c5…`. Step-8 verification and the canonical gate
remain owed.

Production/evidence subject: `6fd3356e3d1ec7d21e4f03850a224283ef43371e`, tree
`e94f502da626c9062fff54c442d51d90d6e097e2`. The immutable review tip is the later evidence-only
commit recorded in `freeze-report.md` and the ignored final receipt. The canonical gate is reserved
for landing-ruling Step 9 and was not run here.

## Final post-rebase runs

| Check | Result |
|---|---|
| Final workspace resolver/replay filter | PASS 38/38, run `2d499436-112f-4bab-9719-dc9843428e18`. Contains all 24 historical conversions, outcome-ledger totality/uniqueness, registration, seven retry/cache-fence cases, and two canonical replay controls. |
| Final semantic resolver/path/provenance filter | PASS 263/263, run `274a3b45-1e38-4810-a27b-438ec47cd27e`. |
| Final session lifecycle/AC6 filter | PASS 11/11, run `67db2ccc-8148-46a2-865a-e6bb13acf14d`. |
| Compile-fail rails | PASS 3/3, run `ad5b6c6a-939e-4636-bd19-fac8816e8710`: foreign observation implementor rejected, private helpers inaccessible, legacy resolver surface absent. |
| Four exact `ResolutionOverlaySnapshot::get` controls | PASS 4/4, run `904b7f6c-7e0a-4493-8283-4238e1e3b6be`: empty, nonempty upsert, tombstone, unknown. |
| Production dependency closure | PASS 1/1, run `2f3ca012-1729-47fa-b36e-32aac7178f03`. |
| Exact machine-marker selector | PASS 1/1, run `3b38b5b3-f990-47da-89fb-ebbfd0ceab10`. |
| Complete `verter_source_policy_gate` package | PASS 187/187, zero skipped, run `4de9f87e-438d-4488-bd24-2ef6bc64312e`; includes every content-addressed evidence-admission mutation. |
| `cargo fmt --all -- --check` | PASS. |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS, whole workspace, 1m41s. |
| `node scripts/validate-performance-gates.mjs --gates performance-gates.toml` | PASS: 4 cells, 56 metrics, no placeholders. |
| Live program validator against three-field external identity overlay | PASS, 69 blocks; overlay proof and digest in `rebase-proof.md`. |

The exact filters are:

```text
cargo nextest run -p verter_workspace -E 'test(resolution_conversion) | test(resolution_driver_tests) | test(canonical_replay_matches_raw_ingress_and_trusts_its_provenance) | test(canonical_manifest_replay_appends_without_recanonicalizing) | test(resolution_overlay_snapshot)' --no-fail-fast
cargo nextest run -p verter_semantic -E 'test(resolver_core) | test(path_utils) | test(canonical_provenance_tests)' --no-fail-fast
cargo nextest run -p verter_session -E 'test(block_6c_view_hoist_tests) | test(lifecycle_answer_equivalence_tests) | test(request_bound_lifecycles_share_one_resolver_context_implementation)' --no-fail-fast
cargo nextest run -p verter_semantic -p verter_workspace -E 'test(/compile_fail/)' --no-fail-fast
cargo nextest run -p verter_source_policy_gate --no-fail-fast
```

## Discrimination and mutation evidence

| Boundary | RED / mutation | GREEN / restore |
|---|---|---|
| Historical S2-F4 conversion | `s2f4/plants.md`: 21 independent semantic plants, 19 physically applied and reverted; every one of the 24 historical cases is discriminated by at least one plant. | `s2f4/correspondence.md` and final 38-test workspace run preserve the exact 24-case ledger. |
| Retained request frame | One-shot retry control renormalizes; deleting retained-frame reuse restores that count while result, waves, ordered load sets, and consumed observations stay equal. | `one_resolution_frame_reuses_normalized_geometry_across_input_waves` PASS in the 263-test semantic run. |
| Canonical provenance | RED 4/5 before registration/replay; one independent-frame isolation control stayed green. | GREEN 5/5; two-frame and basis-clear controls each pay first use. |
| Canonical workspace replay | Compile-RED before the private replay/append functions existed. | Both replay controls PASS in the final workspace run; values, order, and facts equal raw ingress. |
| Empty overlay fast path | RED empty case 0/1 before the private early return; nonempty upsert/tombstone/unknown controls stayed 3/3. Reverting only the return restores that RED. | Final exact controls PASS 4/4. |
| Shared lifecycle adapter (C1-AC-6) | Exact `#[cfg(any())] impl ResolverContext for HostResolverContext` plant turned the structural guard RED, run `b0d33163-8af4-4e59-bb39-08470ee6f592`. | Plant removed byte-exact; GREEN run `b00c720a-5906-40a6-9d2a-bcf7de5515e4`, and final combined lifecycle run PASS 11/11. See `ac6-structural-plant.md`. |
| Machine-path evidence admission | Wrong bytes, missing pin, out-of-root path, wildcard/duplicate path, dead row, unlisted marker, malformed digest, and manifest-digest mutations each produce the named RED. | Complete source-policy package PASS 187/187 with all nine admitted SHA-256 values intact. See `source-policy-mutation-proof.md`. |
| Cache fences and retry ownership | Depth/unique-key/byte/depth/churn limit and no-progress mutations surface `Terminal`; transient load failure retries; changed manifest fingerprint invalidates replay; basis change restarts. | Seven final driver cases PASS inside the 38-test run. |
| Compile-time ownership | Foreign `ResolverObservation` impl, external private-helper access, and legacy resolver use each fail to compile. | Trybuild expectations PASS 3/3 on the final lineage. |

## Historical A6 subject

Enabled attribution arm PASS: normalization `1,981 <= 11,313`; dispatch `4,216`; cold builds and
cache admissions `1,063`; carrier/script/eval parse `40/40/42`; source copy `124,410`; fact
observations `16,917`; indexed builds `8,032`; source-map builds `40`; all three CSS counters zero;
component-meta digest `7161214711717846280`.

Accepted disabled protocol4: controls `86.10 -> 86.90 ms` (`+0.929152%`); base/candidate medians of
four invocation medians `86.880 -> 96.385 ms`. Relative wall is literal **FAIL** at `+10.940378%`,
covered only by `C1-A6-WALL-REL-001`. Absolute wall PASS (`96.385 ms <= 100 ms`); candidate peak RSS
`75,890,688 B`, PASS absolute and `+1.389953%` relative to the frozen reference. See
`a6/final-waiver-application.md`.

## Historical attempts retained, not used

- Wall protocol1 controls drifted `105.69 -> 83.85 ms`; whole session void.
- Wall protocol2 controls drifted `83.48 -> 87.46 ms`; whole session void.
- Protocol3 controls passed and produced the same direction, but a RustDesk application update—not
  merely the explicitly waived steady-state presence—appeared in its condition receipts. It is
  retained diagnostic-only. Protocol4 began only after the updater exited.

No sample from a void or diagnostic-only session enters the final statistic.

## Historical disposition

At the historical subject, no C1-owned implementation, correctness,
performance-retained-gate, mutation, formatting, clippy, or evidence finding
was open. That statement is superseded for the current production-changed
candidate. The registered C2 successors, exact-candidate reviews, canonical
gate, squash, landing, and trunk-side accepted identity are not claimed here.
