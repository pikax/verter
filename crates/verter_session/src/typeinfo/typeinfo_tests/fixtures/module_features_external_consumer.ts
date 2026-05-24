// @ai-generated - Synthetic consumer that imports `Config` from the
// string-literal ambient module `"external-spec"`. After both
// `module_features_external.d.ts` and `module_features_external_patch.ts`
// are loaded, the merged `Config` interface must surface both
// `base: string` and `extra: number`.

import type { Config } from "external-spec";
import "./module_features_external_patch";

export type ExternalConfig = Config;
