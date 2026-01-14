import { ProcessContext } from "../types";
import { processScript } from "../script";

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
} from "../script/plugins/";

import {
  TemplateItem,
  TemplateTypes,
} from "../script/../../parser/template/types";
import { ComponentTypePlugin } from "../script/plugins/component-type";
import { ParsedBlockScript, ScriptItem } from "../../parser";
import { DefaultPlugins, processTemplate } from "../template";
import { ScriptContext } from "../script/types";

export function buildSingle(
  context: Omit<
    ProcessContext,
    "block" | "blockNameResolver" | "isSingleFile" | "items"
  >
) {
  const template = context.blocks.find((x) => x.type === "template");
  const block = context.blocks.find(
    (block) => block.type === "script" && (block as ParsedBlockScript).isMain
  );

  const s = context.override ? context.s : context.s.clone();
  //   ImportsPlugin,

  const scriptContext = {
    ...context,
    s,
    override: true,
    block: block!,
    templateBindings: template?.result?.items
      ? template.result?.items.filter((x) => x.type === TemplateTypes.Binding)
      : [],

    isSingleFile: true,
  } as ScriptContext;

  const scriptResult = block
    ? processScript(
        (block?.result?.items as ScriptItem[]) ?? [],
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
          ComponentInstancePlugin,
          DefineOptionsPlugin,
          ComponentTypePlugin,
          TemplateRefPlugin,
          InferFunctionPlugin,
        ],
        scriptContext,
        false
      )
    : undefined;
  const templateResult = template
    ? processTemplate(
        (template.result?.items as TemplateItem[]) ?? [],
        DefaultPlugins.filter(
          (x) => x.name !== "VerterSFCCleaner" //&& x.name !== "VerterImports"
        ),
        {
          ...context,
          blockNameResolver: () => context.filename,
          s,
          override: true,
          block: template!,
          isSingleFile: true,
        },
        false
      )
    : undefined;

  scriptResult?.pre();
  templateResult?.pre();

  scriptResult?.main();
  templateResult?.main();

  scriptResult?.post();
  templateResult?.post();

  return {
    s,
    template: templateResult,
    script: scriptResult,
  };
}
