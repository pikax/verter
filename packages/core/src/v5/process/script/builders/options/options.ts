import { camelize, capitalize } from "vue";
import { ScriptItem } from "../../../../parser/script/types";
import { BundlerHelper } from "../../../template/helpers/bundler";
import { ProcessContext } from "../../../types";
import { generateImport } from "../../../utils";
import { processScript } from "../../script";
import { ScriptContext } from "../../types";

import { ImportsPlugin, ScriptBlockPlugin } from "../../plugins/";

import { relative } from "node:path/posix";

export function ResolveOptionsFilename(filename: string) {
  return `${filename}.bundle.ts`;
}

export function buildOptions(
  items: ScriptItem[],
  context: Partial<ScriptContext> &
    Pick<ProcessContext, "filename" | "s" | "blocks" | "block">
) {
  const template = context.blocks.find((x) => x.type === "template");

  return processScript(items, [ImportsPlugin, ScriptBlockPlugin], context);
}
