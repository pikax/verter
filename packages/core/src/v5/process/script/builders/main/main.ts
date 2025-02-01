import { camelize, capitalize } from "vue";
import { ScriptItem, ScriptTypes } from "../../../../parser/script/types";
import { BundlerHelper } from "../../../template/helpers/bundler";
import { ProcessContext } from "../../../types";
import { generateImport } from "../../../utils";
import { processScript } from "../../script";
import { ScriptContext } from "../../types";

import {
  ImportsPlugin,
  ScriptBlockPlugin,
  AttributesPlugin,
  BindingPlugin,
  FullContextPlugin,
  MacrosPlugin,
  TemplateBindingPlugin,
} from "../../plugins/";

import { relative } from "node:path/posix";
import { TemplateTypes } from "../../../../parser/template/types";

export function ResolveOptionsFilename(filename: string) {
  return `${filename}.bundle`;
}

export function buildOptions(
  items: ScriptItem[],
  context: Partial<ScriptContext> &
    Pick<ProcessContext, "filename" | "s" | "blocks" | "block" | "override">
) {
  const template = context.blocks.find((x) => x.type === "template");

  return processScript(
    items,
    [
      ImportsPlugin,
      ScriptBlockPlugin,
      AttributesPlugin,
      BindingPlugin,
      FullContextPlugin,
      MacrosPlugin,
      TemplateBindingPlugin,
    ],
    {
      ...context,
      templateBindings: template?.result?.items
        ? template.result?.items.filter((x) => x.type === TemplateTypes.Binding)
        : [],
    }
  );
}
