// Archetype: relative-path module augmentation —
// `declare module "./local" { ... }`.
//
// Per R29 the augmentation target resolves to
// `AugmentationTargetKind::ResolvedRelativeCanonical(canonical)`
// where `canonical` is `"./local"` resolved relative to THIS file's
// canonical path.
//
// Stage 0 baseline characterisation: today the augmenter is reached
// through the import graph; no typed AugmentationTargetKey exists.
// Project isolation (per Codex P0) means an augmenter under
// project A does NOT poison project B, but that isolation is
// implicit in workspace ownership today.
//
// Stage 6d post-change discriminator: `AugmentationTargetKey` includes
// `project_identity` + `resolve_env_hash` so the same `./local`
// specifier in two projects resolves to two distinct keys.

declare module "./local" {
  /** Relative-target augmenter contribution. */
  export interface LocalContract {
    extension: string;
  }
}

export {};
