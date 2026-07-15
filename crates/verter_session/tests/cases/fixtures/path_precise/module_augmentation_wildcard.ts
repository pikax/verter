// Archetype: wildcard ambient module — `declare module "*.css" { ... }`.
//
// Per R29 the augmentation target is
// `AugmentationTargetKind::WildcardAmbient(InternedGlobPattern("*.css"))`.
//
// Stage 0 baseline characterisation: today CSS module declarations
// land on a generic "ambient declarations" path with no typed
// dispatch.
//
// Stage 6d post-change discriminator: a consumer importing
// `./styles.css` resolves through the wildcard match; the
// `AugmentationTargetKind` is preserved in the resolved key so
// removing this declaration invalidates only consumers of
// wildcard-matched specifiers, not consumers of explicit ones.

declare module "*.css" {
  const stylesheet: { readonly [className: string]: string };
  export default stylesheet;
}

export {};
