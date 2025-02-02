import { buildTemplate, TemplateItem } from "@verter/core";
import { SubDocumentProcessContext } from "../../sub.js";
import { LanguageTypescript, VueTypescriptDocument } from "../typescript.js";
import {
  ParsedBlockScript,
  ParsedBlockTemplate,
} from "@verter/core/dist/v5/parser/types.js";
import { createSubDocumentUri, uriToPath } from "../../../../../utils.js";
import { VueDocument } from "../../../vue.js";

export class VueRenderDocument extends VueTypescriptDocument {
  static create(
    uri: string,
    parent: VueDocument,
    languageId: LanguageTypescript,
    version: number
  ) {
    return new VueRenderDocument(uri, parent, languageId, version);
  }

  protected buildResult: ReturnType<typeof buildTemplate> | null = null;

  protected process(context: SubDocumentProcessContext) {
    const block = context.blocks.find((block) => block.type === "template") as
      | ParsedBlockTemplate
      | undefined;
    if (!block) return;

    this.buildResult = buildTemplate(block.result?.items ?? [], {
      ...context,
      block,
      filename: uriToPath(this.uri),
      override: true,
      blockNameResolver: (name) => {
        const n = this.parent.uri.split("/").pop() ?? "Comp.vue";
        return createSubDocumentUri(n, name);
      },
    });
  }
}
