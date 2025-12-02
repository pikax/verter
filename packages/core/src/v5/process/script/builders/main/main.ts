import { ScriptItem } from "../../../../parser/script/types";
import { ProcessContext } from "../../../types";
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
  ComponentInstancePlugin,
  DefineOptionsPlugin,
  InferFunctionPlugin,
  TemplateRefPlugin,
} from "../../plugins/";

import { TemplateTypes } from "../../../../parser/template/types";
import { ComponentTypePlugin } from "../../plugins/component-type";

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
      ComponentInstancePlugin,
      DefineOptionsPlugin,
      ComponentTypePlugin,
      TemplateRefPlugin,
      InferFunctionPlugin,
    ],
    {
      ...context,
      templateBindings: template?.result?.items
        ? template.result?.items.filter((x) => x.type === TemplateTypes.Binding)
        : [],
    }
  );
}
