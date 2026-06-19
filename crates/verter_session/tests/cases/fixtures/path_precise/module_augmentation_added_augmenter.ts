// Archetype: dynamic augmenter set — initial state has ONE augmenter.
// During the test, a sibling fixture
// `module_augmentation_added_augmenter_secondary.ts` is loaded and
// adds a second augmenter to the same module.
//
// Per R29 + G1: this exercises
// `ModuleAugmentationIndexShape(target).semantic_hash` invariance:
// before secondary load, fingerprint = `hash([primary])`; after
// secondary load, fingerprint = `hash([primary, secondary])` sorted.
// Existing `EffectiveExportSet(specifier="vue")` cache entries
// MUST invalidate when the augmenter set changes.
//
// Stage 0 baseline characterisation: today there is no typed
// augmenter-set fingerprint; the secondary's load forces a
// workspace cascade.
//
// Stage 6d post-change discriminator: cold-start with ONLY
// primary; secondary is added; consumer recomputes and sees BOTH
// augmenters' contributions.

declare module "vue" {
  interface ComponentOptions {
    /** Primary augmenter contribution. */
    primaryField: string;
  }
}

export {};
