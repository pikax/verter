import { buildOptions, ScriptItem } from "@verter/core";
import { SubDocumentProcessContext } from "../../sub.js";
import { LanguageTypescript, VueTypescriptDocument } from "../typescript.js";
import { ParsedBlockScript } from "@verter/core/dist/v5/parser/types.js";
import { createSubDocumentUri, uriToPath } from "../../../../../utils.js";
import { VueDocument } from "../../../vue.js";

export class VueOptionsDocument extends VueTypescriptDocument {
  static create(
    uri: string,
    parent: VueDocument,
    languageId: LanguageTypescript,
    version: number
  ) {
    return new VueOptionsDocument(uri, parent, languageId, version);
  }

  protected buildResult: ReturnType<typeof buildOptions> | null = null;

  protected process(context: SubDocumentProcessContext) {
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
        override: true,
        blockNameResolver: (name) => {
          const n = this.parent.uri.split("/").pop() ?? "Comp.vue";
          return createSubDocumentUri(n, name);
        },
      }
    );
  }
}
