import { MagicString } from "@vue/compiler-sfc";
import {
  TemplateCondition,
  TemplateItem,
  TemplateItemByType,
  TemplateTypes,
} from "../../parser/template/types";
import { defaultPrefix } from "../utils";
import { ProcessContext, ProcessPlugin } from "./../types";

export type TemplatePlugin = ProcessPlugin<TemplateItem, TemplateContext> & {
  [K in `transform${TemplateTypes}`]?: (
    item: K extends `transform${infer C extends TemplateTypes}`
      ? TemplateItemByType[C]
      : TemplateItem,
    s: MagicString,
    context: TemplateContext
  ) => void;
};

// type TransformPlugin = {
//     [K in `transform${TemplateTypes}`]?: (
//     item: K extends `transform${infer C extends TemplateTypes}`
//       ? TemplateItemByType[C]
//       : TemplateItem,
//     s: MagicString,
//     context: TemplateContext
//   ) => void;
// }

// export interface TemplatePlugin extends ProcessPlugin<TemplateItem>,  TransformPlugin{

// }

export type TemplateAccessors =
  | "renderList"
  | "normalizeClass"
  | "normalizeStyle"
  | "COMPONENT_CTX"
  | "ctx"
  // aka $slots
  | "$slot"
  // slotToComponent, const slotComponent = ctx.$slots.default;
  | "slotComponent"
  // instance from v-slot=
  | "slotInstance"
  // slotRender, slotRender(ctx.$slots.default, (slotProps)=> [...])
  | "slotRender"
  // callback for events
  | "eventCb"
  // (...eventArgs) in the event callback
  | 'eventArgs';
export type TemplateContext = ProcessContext & {
  prefix: (str: string) => string;
  isCustomElement: (tag: string) => boolean;

  camelWhitelistAttributes: (name: string) => boolean;

  retrieveAccessor: (name: TemplateAccessors) => string;

  narrow?:
    | boolean
    | {
        functions: boolean;
      };

  toNarrow?: Array<{
    index: number;
    inBlock: boolean;
    conditions: TemplateItemByType[TemplateTypes.Condition][];

    type?: "prepend" | "append";
    direction?: "left" | "right";

    condition?: TemplateCondition | null;
  }>;
};

export function declareTemplatePlugin<T extends TemplatePlugin>(plugin: T) {
  return plugin;
}

export function processTemplate(
  items: TemplateItem[],
  plugins: TemplatePlugin[],
  _context: Partial<TemplateContext> &
    Pick<ProcessContext, "filename" | "s" | "blocks">
) {
  const context: TemplateContext = {
    generic: undefined,
    isAsync: false,
    camelWhitelistAttributes: (name: string) => {
      return name.startsWith("data-") || name.startsWith("aria-");
    },
    isCustomElement: (_: string) => {
      return false;
    },
    prefix: defaultPrefix,

    retrieveAccessor: (name: TemplateAccessors) => {
      return defaultPrefix(name);
    },

    ..._context,
  };

  const s = context.s.clone();

  const pluginsByType = {
    [TemplateTypes.SlotDeclaration]: [],
    [TemplateTypes.Loop]: [],
    [TemplateTypes.Element]: [],
    [TemplateTypes.Prop]: [],
    [TemplateTypes.Binding]: [],
    [TemplateTypes.Interpolation]: [],
    [TemplateTypes.SlotRender]: [],
    [TemplateTypes.Comment]: [],
    [TemplateTypes.Text]: [],
    [TemplateTypes.Directive]: [],
    [TemplateTypes.Function]: [],
    [TemplateTypes.Condition]: [],
  } as {
    [K in TemplateTypes]: Array<
      (
        item: TemplateItemByType[K],
        s: MagicString,
        context: ProcessContext
      ) => void
    >;
  };
  const PLUGIN_TYPES = Object.keys(pluginsByType) as readonly TemplateTypes[];

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
              const type = key.slice(9) as TemplateTypes;
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
