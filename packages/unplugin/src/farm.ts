import type { JsPlugin } from "@farmfe/core";
import type { VerterPluginOptions } from "./core/types";
import unplugin from "./index";

export default unplugin.farm as (options?: VerterPluginOptions) => JsPlugin;
