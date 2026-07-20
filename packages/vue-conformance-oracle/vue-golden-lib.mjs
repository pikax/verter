/**
 * Shared pin authority for the Vue conformance-goldens oracle.
 *
 * `VUE_ORACLE_VERSION` is THE single authority for the official Vue RC
 * toolchain version the vendored goldens under
 * `crates/verter_vue_conformance/corpus/goldens/` were generated with. The
 * dev-dependency pins in this package's `package.json`, the generator's
 * resolved-version assertion, and the hermetic Rust freshness guard
 * (`verter_vue_conformance` `tests/cases/generator_smoke.rs`) all read or are
 * checked against this constant. Bumping the RC requires:
 *
 *   1. update `VUE_ORACLE_VERSION` here,
 *   2. update the four pins in `package.json` (exact, no ranges/dist-tags),
 *   3. `pnpm install` (lockfile refresh),
 *   4. `pnpm gen:vue-goldens` (regenerate every golden + metadata),
 *   5. commit everything together.
 */

export const VUE_ORACLE_VERSION = "3.6.0-rc.1";

/** The four packages that must ALL resolve to exactly VUE_ORACLE_VERSION. */
export const ORACLE_PACKAGES = [
  "vue",
  "@vue/compiler-dom",
  "@vue/compiler-sfc",
  "@vue/compiler-vapor",
];

/**
 * Exact esbuild version used for the ONE sanctioned post-process: stripping
 * TypeScript types from `lang="ts"` script-setup cells (the official
 * SFC-loader pipeline strips types the same way after `compileScript`).
 * Pinned exactly in `package.json`; recorded in every stripped cell's
 * metadata. Type-stripping keeps `/*@__PURE__*\/` annotations and the
 * official export shape (`{ loader: "ts" }`, no format conversion), and the
 * compiler's source map is chained through the strip so the vendored map
 * still anchors the golden back to the SFC.
 */
export const ESBUILD_VERSION = "0.28.0";

/** Bump when the generator's emission/metadata shape changes. */
export const GENERATOR_VERSION = 3;

/** Metadata schema version stamped into every `.meta.json`. */
export const META_SCHEMA_VERSION = 2;

/** Manifest schema version stamped into `corpus/manifest.json`. */
export const MANIFEST_SCHEMA_VERSION = 2;
