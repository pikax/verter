# Inherited class member read inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

The inherited member read adds no retained store, counter, hook or trace. Its
work is measured in tests through the existing semantic-node intern counter
(`MetaProvenance` snapshot `node_arena_pushes`); it registers nothing new.

## verter_type_engine

These are per-read bookkeeping, not semantic stores, caches or counters; they are
classified so the inventory covers every field this change adds.

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `class_member_source` `followed`: the `(canonical, owner, class)` identities one member read has followed up its `extends` chain | the read's circular-chain stop | REQUIRED | Owned by one `class_member_source` call; dropped when it returns | `crates/verter_type_engine/src/project_semantic_dispatch/build.rs` | always |
| `class_takes_module_augmentation` augmenter-population observation (the existing `ModuleAugmentationIndexShape` / contributor-population facts) | the read's fallback to the whole merged declaration when another module augments a class on the path; the facts root the reading result so an augmenter appearing later misses it | REQUIRED | Observed onto the active fact tracer for the enclosing traced compute | `crates/verter_type_engine/src/project_semantic_dispatch/build.rs` | always |
