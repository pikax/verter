// Archetype: external-specifier module augmentation —
// `declare module 'vue' { interface ComponentOptions { foo: number } }`.
//
// Per R29 the augmentation target resolves to
// `AugmentationTargetKind::ExternalSpecifier("vue")`. A consumer that
// reads `keyof ComponentOptions` (from `'vue'`) sees the augmented
// surface that includes `foo`.
//
// Stage 0 baseline characterisation: today there is no
// `ModuleAugmentationIndex`; the augmenter file participates in the
// consumer's invalidation cascade through workspace-edge tracking,
// not through a typed augmentation fact. Editing this file
// invalidates the consumer.
//
// Stage 6d post-change discriminator: the augmenter contributes via
// `EffectiveExportSet("vue")` consulting
// `FileArtifactStore.augmentation_index[ExternalSpecifier("vue")]`.
// Editing the augmenter file changes
// `ModuleAugmentationIndexShape(ExternalSpecifier("vue")).semantic_hash`
// and invalidates the consumer. Adding an UNRELATED augmenter (e.g.
// `declare module 'pinia'`) does NOT.

declare module "vue" {
  interface ComponentOptions {
    /** Augmenter contribution — visible on `keyof ComponentOptions`. */
    foo: number;
  }
}

// Force this file to be a module.
export {};
