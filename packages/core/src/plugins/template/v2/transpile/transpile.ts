import type { RootNode } from "@vue/compiler-core";
import { MagicString } from "@vue/compiler-sfc";
import { walk } from "../walk/index.js";
import Transpilers from "./transpilers/index.js";
import type { TranspileContext } from "./types.js";

import deepmerge from "deepmerge";

// TODO possibly move this to another file
export const DEFAULT_PREFIX = `___VERTER___`;
export function PrefixSTR(s: string, prefix = DEFAULT_PREFIX) {
  return [prefix, s].join("");
}

export type TranspileOptions = Partial<
  TranspileContext & {
    plugins: typeof Transpilers;
    prefix: string;
  }
>;

const DefaultAccessors = {
  ctx: PrefixSTR("ctx", DEFAULT_PREFIX),
  comp: PrefixSTR("comp", DEFAULT_PREFIX),
  slot: PrefixSTR("slot", DEFAULT_PREFIX),
  template: PrefixSTR("template", DEFAULT_PREFIX),
  slotCallback: PrefixSTR("SLOT_CALLBACK", DEFAULT_PREFIX),
  normalizeClass: PrefixSTR("normalizeClass", DEFAULT_PREFIX),
  normalizeStyle: PrefixSTR("normalizeStyle", DEFAULT_PREFIX),
  renderList: PrefixSTR("renderList", DEFAULT_PREFIX),
  eventCb: PrefixSTR("eventCb", DEFAULT_PREFIX),
  componentInstance: PrefixSTR("componentInstance", DEFAULT_PREFIX),
};

export function getAccessors(
  prefix?: string
): Readonly<typeof DefaultAccessors> {
  return prefix === DEFAULT_PREFIX || !prefix
    ? DefaultAccessors
    : {
        ctx: PrefixSTR("ctx", prefix),
        comp: PrefixSTR("comp", prefix),
        slot: PrefixSTR("slot", prefix),
        template: PrefixSTR("template", prefix),
        slotCallback: PrefixSTR("SLOT_CALLBACK", prefix),
        normalizeClass: PrefixSTR("normalizeClass", prefix),
        normalizeStyle: PrefixSTR("normalizeStyle", prefix),
        renderList: PrefixSTR("renderList", prefix),
        eventCb: PrefixSTR("eventCb", prefix),
        componentInstance: PrefixSTR("componentInstance", prefix),
      };
}

export function transpile(
  root: RootNode,
  s: MagicString,
  options: TranspileOptions = {
    ...({} as Partial<TranspileContext>),
    plugins: Transpilers,
    prefix: DEFAULT_PREFIX,
  }
) {
  const {
    plugins = Transpilers,
    prefix = DEFAULT_PREFIX,
    ...context
  } = options;

  const finalOptions = deepmerge(
    {
      s,
      accessors: getAccessors(prefix),
      declarations: [],
      conditions: {
        ifs: [],
        elses: [],
      },
      ignoredIdentifiers: [],

      webComponents: [],
      attributes: {
        camelWhitelist: ["data-", "aria-"],
      },
    } as TranspileContext,
    context,
    {
      clone: false,
    }
  );

  walk(
    root,
    {
      enter(node, parent, context, parentContext) {
        const transpiler = plugins[node.type];
        return transpiler?.enter?.(node, parent, context, parentContext);
      },
      leave(node, parent, context, parentContext) {
        const transpiler = plugins[node.type];
        return transpiler?.leave?.(node, parent, context, parentContext);
      },
    },
    finalOptions
  );

  return finalOptions;
}
