# STP15 evidence index

Engine pins: `tests/sfc-projection/STP1/products/engine-matrix.json`. Vue pin: `vue@3.6.0-rc.5`.

Selected case IDs: `STP15-read-ref`, `STP15-readonly-write`, `STP15-setter-domain`, `STP15-unused`, `STP15-mutation`.

Commands (run on the candidate; this file is not an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP15 --engine all --require-all --json`
- `cargo test -p verter_compiler --lib ide::vue_projection::binding_views`
- `cargo test -p verter_compiler --test main binding_views_reads_admitted_carrier_blocks`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

Products: `TemplateReadView`, `TemplateWriteTarget`, `BindingUsageSet`.

Implementation home: `crates/verter_compiler/src/ide/vue_projection/binding_views.rs`. Dormant consumer: `VueProjectionBackend::binding_views`. Vue IDE `project_ide` and the Svelte route are unchanged.

Changed symbols: `BindingKind` (unchanged variants; `toRefs` whole-result now classifies `Plain`); `WriteRejection::ConstBinding` (new) with `TemplateWriteTarget::immutable` rows and the `write_target` refusal; `push_binding`/`classify_declarator` take declaration mutability (`const` plains refuse writes, all other kinds unchanged); `setter_domain` asserts its span instead of clamping; `BindingUsageSet::from_region_text` (new) collecting template mentions, script identifier references, and style `v-bind()` names over `from_authored_references`; `FACTORY_EXPORTS` membership documented against the parsed `from 'vue'` imports.

Deletion population this node: empty. Retirement of universal unwrap/write aliases and unconditional binding-use scaffolds happens at atomic activation (STP58); no such alias or scaffold exists in this product.

## Why the negative cases discriminate

Each `reject` row is exercised twice: as an applied source mutation in `protocol.mjs` (`assertDirtyTwinsRejected` writes the patch into the owned product, requires the discriminator run to fail, then restores clean), and as an executable Rust case that fails when the rule is removed. Negative controls run on the candidate confirmed that applying any one of the following makes exactly its owning discriminator fail and leaves the clean run green:

- universal mutable alias (getter-only `computed` pushed to writable rows) — `stp15_readonly_write_rejects_getter_computed_and_readonly_prop`
- `const`-plain write refusal removed — `stp15_const_plain_bindings_refuse_template_writes`
- synthetic void reads (`if true` in the accounting core) — `stp15_unused_reports_only_authored_references`
- immutable snapshot (`snapshot_kind` returning a snapshot variant) — `stp15_mutation_uses_live_views_without_snapshots`
- read-type write domain (setter domain forced empty) — `stp15_setter_domain_uses_declared_setter_type`

The whole-`toRefs` and region-usage controls (`stp15_whole_torefs_result_reads_directly`, `stp15_usage_collects_authored_region_references`) pin the accept rows with clean/control twins. The probe pair additionally proves through both pinned engines that the read/write views keep the authored member type: the negative probe still raises the customer diagnostic rather than collapsing into a permissive shape, and the positive probe's clean twin stays diagnostic-free.

## Review findings disposition

Open findings against this candidate and their repair: span-clamp replaced with an assert; `FACTORY_EXPORTS` membership documented against the parsed imports; `const` plains refused via `WriteRejection::ConstBinding` with a mutable-twin control; usage connected to source-backed region reference facts (`from_region_text` plus region fixture); dirty twins applied as source patches with required failure and restore; whole-`toRefs` result classified `Plain` with a destructured-member control twin; completion evidence recorded here. The scheduler fixed-sleep report does not match the owned test (the wait observes the teardown-wake post count with a deadline-bounded yield, no fixed sleep at the cited site). Raw CI logs remain owned by the CI artifact (`CI Required` rollup on the candidate PR); this file records no execution transcript.

Raw outcomes: see the CI artifact owner for the candidate run; local qualification is the charter §14 command above.
