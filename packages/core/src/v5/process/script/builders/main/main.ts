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
  DeclarePlugin,
  FullContextPlugin,
  MacrosPlugin,
  TemplateBindingPlugin,
  SFCCleanerPlugin,
  ScriptDefaultPlugin,
  ScriptResolversPlugin,
  ComponentInstancePlugin,
  DefineOptionsPlugin,
  InferFunctionPlugin,
} from "../../plugins/";

import { TemplateTypes } from "../../../../parser/template/types";

export function ResolveOptionsFilename(
  ctx: Required<Pick<ProcessContext, "blockNameResolver">>
) {
  return ctx.blockNameResolver(`options`);
}

export function buildOptions(
  items: ScriptItem[],
  context: Partial<ScriptContext> &
    Pick<
      ProcessContext,
      "filename" | "s" | "blocks" | "block" | "override" | "blockNameResolver"
    >
) {
  const template = context.blocks.find((x) => x.type === "template");

  return processScript(
    items,
    [
      ImportsPlugin,
      ScriptBlockPlugin,
      AttributesPlugin,
      DeclarePlugin,
      BindingPlugin,
      FullContextPlugin,
      MacrosPlugin,
      TemplateBindingPlugin,
      ScriptDefaultPlugin,
      SFCCleanerPlugin,
      ScriptResolversPlugin,
      ComponentInstancePlugin,
      DefineOptionsPlugin,
      InferFunctionPlugin
    ],
    {
      ...context,
      templateBindings: template?.result?.items
        ? template.result?.items.filter((x) => x.type === TemplateTypes.Binding)
        : [],
    }
  );
}
