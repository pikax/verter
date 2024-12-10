export * from "./builder.js";

export type {
  ParseScriptContext,
  LocationByType,
  TypeLocationImport,
  ImportItem,
} from "./plugins/index.js";
export { LocationType } from "./plugins/index.js";
export { TemplateBuilder, getAccessors } from "./plugins/index.js";

export {
  DEFAULT_PREFIX,
  PrefixSTR,
} from "./plugins/template/v2/transpile/transpile.js";

export type { VerterSFCBlock } from "./utils/sfc/index.js";

export { mergeFull } from "./mergers/full.js";

export * from "./parser/index.js";