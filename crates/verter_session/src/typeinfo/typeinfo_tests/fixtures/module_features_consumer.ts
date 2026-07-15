// @ai-generated - Synthetic consumer that ties together
//   * `typeof import("./module_features_leaf")` named + default exports,
//   * `declare module "./module_features_base"` augmented surface,
//   * `typeof import("./module_features_cjs")` CommonJS-export-= interop.

import type { Plugin } from "./module_features_base";
import "./module_features_patch";

// `typeof import("./mod")` returns the module's runtime VALUE namespace;
// it does NOT include type-only exports. Type-only exports must be
// reached through the dynamic-import-in-type-position form
// `import("./mod").TypeName` (which exposes both type and value slots,
// and which is itself a valid stored type alias). Both forms are used
// below to exercise the value-namespace path AND the type-slot path
// against the same leaf module.
export type LeafModule = typeof import("./module_features_leaf");
export type LeafDefault = LeafModule["default"];
export type LeafNamedShape = import("./module_features_leaf").LeafShape;
export type LeafNamedValue = LeafModule["leafName"];

// The augmented `Plugin` surface — must include base `id`, patch `extra`,
// patch `label?`.
export type AugmentedPlugin = Plugin;

// `typeof import("./module_features_cjs")` against an `export = ` module
// gives the type of the export-= value directly.
export type CjsBinding = typeof import("./module_features_cjs");

// Mixed `import { type X, valueY }` syntax against `module_features_leaf`.
// `LeafShape` is a type-only specifier; `leafName` is a value-only
// specifier. The two slots resolve independently:
//   * `LeafShape` → the declared interface shape
//   * `typeof leafName` → the literal type `"leaf"` (`const`-narrowed)
import { type LeafShape, leafName } from "./module_features_leaf";

export type LeafTypeImported = LeafShape;
export type LeafValueTypeof = typeof leafName;
