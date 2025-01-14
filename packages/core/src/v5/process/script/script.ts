import { MagicString } from "@vue/compiler-sfc";
import {
  ScriptItem,
  ScriptItemByType,
  ScriptTypes,
} from "../../parser/script/types";
import { ProcessContext } from "../types";
import { ScriptPlugin } from "./types";

export function buildBundle() {}
export function buildOptions() {}

// Production does not need all the fancy stuff
// this only needs to export the correct type
// some magic still needs to happen
export function buildProduction() {}

export interface ScriptContext extends ProcessContext {}

export function processScript(
  items: ScriptItem[],
  plugins: ScriptPlugin[],
  _context: Partial<ScriptContext> &
    Pick<ProcessContext, "filename" | "s" | "blocks">
) {
  const context: ScriptContext = {
    generic: undefined,
    isAsync: false,
    // camelWhitelistAttributes: (name: string) => {
    //   return name.startsWith("data-") || name.startsWith("aria-");
    // },
    // isCustomElement: (_: string) => {
    //   return false;
    // },
    // prefix: defaultPrefix,

    // retrieveAccessor: (name: TemplateAccessors) => {
    //   return defaultPrefix(name);
    // },

    ..._context,
  };

  const s = context.s.clone();

  const pluginsByType = {
    [ScriptTypes.Async]: [],
    [ScriptTypes.Binding]: [],
    [ScriptTypes.Declaration]: [],
    [ScriptTypes.Export]: [],
    [ScriptTypes.FunctionCall]: [],
    [ScriptTypes.Import]: [],
    [ScriptTypes.Error]: [],
    [ScriptTypes.Warning]: [],
  } as {
    [K in ScriptTypes]: Array<
      (
        item: ScriptItemByType[K],
        s: MagicString,
        context: ProcessContext
      ) => void
    >;
  };
  const PLUGIN_TYPES = Object.keys(pluginsByType) as readonly ScriptTypes[];

  const prePlugins = [] as Array<
    (s: MagicString, context: ProcessContext) => void
  >;
  const postPlugins = [] as Array<
    (s: MagicString, context: ProcessContext) => void
  >;

  [...plugins]
    .sort((a, b) => {
      if (a.enforce === "pre" && b.enforce === "post") {
        return -1;
      }
      if (a.enforce === "post" && b.enforce === "pre") {
        return 1;
      }
      return 0;
    })
    .forEach((x) => {
      for (const [key, value] of Object.entries(x)) {
        if (typeof value !== "function") continue;
        switch (key) {
          case "pre": {
            prePlugins.push(value.bind(x) as any);
            break;
          }
          case "post": {
            postPlugins.push(value.bind(x) as any);
            break;
          }

          case "transform": {
            PLUGIN_TYPES.forEach((type) => {
              pluginsByType[type].push(value.bind(x) as any);
            });
            break;
          }
          default: {
            if (key.startsWith("transform")) {
              const type = key.slice(9) as ScriptTypes;
              pluginsByType[type].push(value.bind(x) as any);
            }
          }
        }
      }
    });

  for (const plugin of prePlugins) {
    plugin(s, context);
  }

  for (const item of items) {
    for (const plugin of pluginsByType[item.type]) {
      plugin(item as any, s, context);
    }
  }

  for (const plugin of postPlugins) {
    plugin(s, context);
  }

  return {
    context,
    s: s,
    result: s.toString(),
  };
}
