// Archetype: global augmentation — `declare global { interface Window { ... } }`.
//
// Per R29 the augmentation target is
// `AugmentationTargetKind::GlobalAugmentation`.
//
// Stage 0 baseline characterisation: today global augmentations
// participate via TypeScript's implicit global resolution; there is
// no typed key.
//
// Stage 6d post-change discriminator: any consumer that observes
// `Window` (via `window.<key>`, `typeof window`, etc.) records a
// fact against `ModuleAugmentationIndexShape(GlobalAugmentation)`.
// Removing this declaration invalidates only those consumers.

declare global {
  interface Window {
    /** Global augmenter contribution. */
    __verterDebugSession?: { id: string; epoch: number };
  }
}

export {};
