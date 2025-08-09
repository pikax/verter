import { MagicString } from "@vue/compiler-sfc";
import {
  ScriptItem,
  ScriptItemByType,
  ScriptTypes,
} from "../../parser/script/types";
import { ProcessContext, ProcessPlugin } from "../types";
import { TemplateBinding } from "../../parser/template/types";

export interface ScriptContext extends ProcessContext {
  prefix(name: string): string;

  isSetup: boolean;

  templateBindings: TemplateBinding[];
  handledAttributes?: Set<string>;

  processedItems: ScriptItem[];
}

export type ScriptPlugin = ProcessPlugin<ScriptItem, ScriptContext> & {
  [K in `transform${ScriptTypes}`]?: (
    item: K extends `transform${infer C extends ScriptTypes}`
      ? ScriptItemByType[C]
      : ScriptItem,
    s: MagicString,
    context: ScriptContext
  ) => void;
};

export function definePlugin<T extends ScriptPlugin>(plugin: T) {
  return plugin;
}
