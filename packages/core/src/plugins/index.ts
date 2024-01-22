import { ParseScriptContext, PluginOption, LocationType } from "./types.js";

export * from "./types.js";

import EmitsPlugin from "./emits/index.js";
import PropsPlugin from "./props/index.js";
import SlotsPlugin from "./slots/index.js";
import ModelsPlugin from "./models/index.js";
import ExposePlugin from "./expose/index.js";
import OptionsPlugin from "./options/index.js";
import DeclarationPlugin from "./declaration/index.js";
import GenericPlugin from "./generic/index.js";
import TemplatePlugin from "./template/index.js";
// import ExportPlugin from "./export/index.js";

export const defaultPlugins = [
  EmitsPlugin,
  PropsPlugin,
  SlotsPlugin,
  ModelsPlugin,
  ExposePlugin,
  OptionsPlugin,
  DeclarationPlugin,
  GenericPlugin,
  TemplatePlugin,
  //   ImportPlugin,
  // ExportPlugin,
] as PluginOption[];
