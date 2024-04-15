import type { RootNode } from "@vue/compiler-core";
import { MagicString } from "@vue/compiler-sfc";
import { walk } from "../walk";
import Transpilers from "./transpilers";
import type { TranspileContext } from "./types";

import deepmerge from "deepmerge";

const DEFAULT_PREFIX = `___VERTER___`;

function PrefixSTR(s: string, prefix = DEFAULT_PREFIX) {
  return [prefix, s].join("");
}

export type TranspileOptions = Partial<
  TranspileContext & {
    plugins: typeof Transpilers;
    prefix: string;
  }
>;

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
      accessors: {
        ctx: PrefixSTR("ctx", prefix),
        comp: PrefixSTR("comp", prefix),
        slot: PrefixSTR("slot", prefix),
        template: PrefixSTR("template", prefix),
        slotCallback: PrefixSTR("SLOT_CALLBACK", prefix),
        normalizeClass: PrefixSTR("normalizeClass", prefix),
        normalizeStyle: PrefixSTR("normalizeStyle", prefix),
        renderList: PrefixSTR("renderList", prefix),
        eventCb: PrefixSTR("eventCb", prefix),
      },
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
