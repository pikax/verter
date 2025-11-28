import { MagicString } from "@vue/compiler-sfc";
import {
  TemplateCondition,
  TemplateElement,
  TemplateItem,
  TemplateItemByType,
  TemplateTypes,
} from "../../parser/template/types";
import { defaultPrefix } from "../utils";
import { ProcessContext, ProcessPlugin } from "./../types";
import { VerterASTNode } from "../../parser/ast";
import { Node } from "@vue/compiler-core";

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
  | "StrictRenderSlot"
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
  | "eventArgs"
  // to retrieve the instance on a component or element Template
  | "instancePropertySymbol"
  // retrieve the directive function for an element, eg: const dir = {instanceToDirectiveFn}(new HTMLInputElement());
  | "instanceToDirectiveFn"
  // var for directive, eg: const {instanceToDirectiveVar} = instanceToDirectiveFn(new HTMLInputElement())
  | "instanceToDirectiveVar"
  // directive name, eg: const {directiveName} = instanceToDirectiveVar(directive);
  | "directiveName"
  // accessor for directives, in setup is equivalent to ctx, but in object API should filter only declared in directives
  | "directiveAccessor";
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

  doNarrow?: (
    o: {
      parent?: VerterASTNode | Node | TemplateElement | undefined;
      index: number;
      inBlock: boolean;
      conditions: TemplateItemByType[TemplateTypes.Condition][];

      type?: "prepend" | "append";
      direction?: "left" | "right";

      condition?: TemplateCondition | null;
    },
    s: MagicString
  ) => void;
};

export function declareTemplatePlugin<T extends TemplatePlugin>(plugin: T) {
  return plugin;
}

export function processTemplate(
  items: TemplateItem[],
  plugins: TemplatePlugin[],
  _context: Partial<TemplateContext> &
    Pick<
      ProcessContext,
      "filename" | "s" | "blocks" | "block" | "blockNameResolver"
    >
) {
  const context: TemplateContext = {
    generic: null,
    isAsync: false,
    isTS: false,
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

    items: [],

    ..._context,
  };

  const s = context.override ? context.s : context.s.clone();

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
    [TemplateTypes.Literal]: [],
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
      if (a.enforce === "pre") {
        return -1;
      }
      if (a.enforce === "post") {
        return 1;
      }
      if (b.enforce === "pre") {
        return 1;
      }
      if (b.enforce === "post") {
        return -1;
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
