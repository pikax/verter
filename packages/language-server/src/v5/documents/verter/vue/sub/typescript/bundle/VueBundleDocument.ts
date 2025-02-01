import { buildBundle, ScriptItem } from "@verter/core";
import { SubDocumentProcessContext } from "../../sub.js";
import { LanguageTypescript, VueTypescriptDocument } from "../typescript.js";
import { ParsedBlockScript } from "@verter/core/dist/v5/parser/types.js";
import { uriToPath } from "../../../../../utils.js";
import { VueDocument } from "../../../vue.js";

export class VueBundleDocument extends VueTypescriptDocument {
  static create(
    uri: string,
    parent: VueDocument,
    languageId: LanguageTypescript,
    version: number
  ) {
    return new VueBundleDocument(uri, parent, languageId, version);
  }

  protected buildResult: ReturnType<typeof buildBundle> | null = null;

  protected process(context: SubDocumentProcessContext) {
    const block = context.blocks.find(
      (block) => block.type === "script" && (block as ParsedBlockScript).isMain
    );
    if (!block) return;

    this.buildResult = buildBundle(
      (block.result?.items as ScriptItem[]) ?? [],
      {
        ...context,
        block,
        filename: uriToPath(this.uri),
        override: true,
      }
    );
  }
}
