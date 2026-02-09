import { buildSingle } from "@verter/core";
import { SubDocumentProcessContext } from "../../sub";
import { LanguageTypescript, VueTypescriptDocument } from "../typescript.js";
import { uriToPath } from "../../../../../utils.js";
import type { VueDocument } from "../../../vue.js";

export class VueSingleDocument extends VueTypescriptDocument {
  static create(uri: string, parent: VueDocument, languageId: LanguageTypescript, version: number) {
    return new VueSingleDocument(uri, parent, languageId, version);
  }

  protected buildResult: ReturnType<typeof buildSingle> | null = null;

  protected process(context: SubDocumentProcessContext) {
    this.buildResult = buildSingle({
      ...context,
      override: true,
      filename: uriToPath(this.uri),
    });
  }
}
