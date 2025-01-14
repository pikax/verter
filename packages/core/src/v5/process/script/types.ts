import { MagicString } from "@vue/compiler-sfc";
import {
  ScriptItem,
  ScriptItemByType,
  ScriptTypes,
} from "../../parser/script/types";
import { ProcessContext, ProcessPlugin } from "../types";

export type ScriptPlugin = ProcessPlugin<ScriptItem, ProcessContext> & {
  [K in `transform${ScriptTypes}`]?: (
    item: K extends `transform${infer C extends ScriptTypes}`
      ? ScriptItemByType[C]
      : ScriptItem,
    s: MagicString,
    context: ProcessContext
  ) => void;
};

export function definePlugin<T extends ScriptPlugin>(plugin: T) {
  return plugin;
}


