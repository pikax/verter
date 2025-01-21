import { LanguageService, Stylesheet } from "vscode-css-languageservice";
import { VueDocument } from "../../index.js";
import { SubDocumentProcessContext, VueSubDocument } from "../sub.js";
import { StyleDocument } from "../../../style/style.js";

export class VueStyleDocument extends VueSubDocument {
  static create(
    uri: string,
    parent: VueDocument,
    languageId: string,
    languageService: LanguageService,
    version: number
  ) {
    return new VueStyleDocument(
      uri,
      parent,
      languageId,
      languageService,
      version
    );
  }

  private _stylesheet: Stylesheet | null = null;
  get stylesheet() {
    return (
      this._stylesheet ??
      (this._stylesheet = this._languageService.parseStylesheet(this))
    );
  }

  protected constructor(
    uri: string,
    parent: VueDocument,
    languageId: string,
    private _languageService: LanguageService,
    version: number
  ) {
    super(uri, parent, languageId, version);
  }

  update(content: string, version?: number): void {
    this._stylesheet = null;
    super.update(content, version);
  }

  protected process(context: SubDocumentProcessContext): void {
    for (const block of context.blocks) {
      if (block.block.tag.type !== "style") {
        // DELETE the whole block
        context.s.remove(
          block.block.tag.pos.open.start,
          block.block.tag.pos.close.end
        );
      } else {
        context.s.remove(
          block.block.tag.pos.open.start,
          block.block.tag.pos.open.end
        );
        context.s.remove(
          block.block.tag.pos.close.start,
          block.block.tag.pos.close.end
        );
      }
    }
  }
}
