// Companion to `module_augmentation_added_augmenter.ts`.
// Added DURING the test to force a change in the augmenter set for
// `'vue'`. The test asserts the consumer's
// `EffectiveExportSet("vue")` cache entry invalidates and the
// recomputed surface includes BOTH `primaryField` and
// `secondaryField`.

declare module "vue" {
  interface ComponentOptions {
    /** Secondary augmenter contribution. */
    secondaryField: number;
  }
}

export {};
