import { buildOptions, ScriptItem } from "@verter/core";
import { SubDocumentProcessContext } from "../../sub.js";
import { VueTypescriptDocument } from "../typescript.js";
import { ParsedBlockScript } from "@verter/core/dist/v5/parser/types.js";
import { uriToPath } from "../../../../../utils.js";

export class VueOptionsDocument extends VueTypescriptDocument {
  protected buildResult: ReturnType<typeof buildOptions> | null = null;

  protected process(context: SubDocumentProcessContext): void {
    const block = context.blocks.find(
      (block) => block.type === "script" && (block as ParsedBlockScript).isMain
    );
    if (!block) return;

    this.buildResult = buildOptions(
      (block.result?.items as ScriptItem[]) ?? [],
      {
        ...context,
        block,
        filename: uriToPath(this.uri),
      }
    );
  }
}
