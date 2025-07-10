import { LanguageService, Stylesheet } from "vscode-css-languageservice";
import { VueDocument } from "../../index.js";
import { SubDocumentProcessContext, VueSubDocument } from "../sub.js";
import { ProcessedBlock } from "../../utils.js";

export class VueStyleDocument extends VueSubDocument {
  static create(
    uri: string,
    parent: VueDocument,
    languageId: string,
    languageService: LanguageService,
    version: number,
    block: ProcessedBlock
  ) {
    return new VueStyleDocument(
      uri,
      parent,
      languageId,
      languageService,
      version,
      block
    );
  }

  private _stylesheet: Stylesheet | null = null;
  get stylesheet() {
    return (
      this._stylesheet ??
      (this._stylesheet = this._languageService.parseStylesheet(this))
    );
  }

  get languageService() {
    return this._languageService;
  }

  protected constructor(
    uri: string,
    parent: VueDocument,
    languageId: string,
    private _languageService: LanguageService,
    version: number,
    public block: ProcessedBlock
  ) {
    super(uri, parent, languageId, version);
  }

  update(content: string, version?: number) {
    this._stylesheet = null;
    super.update(content, version);
    return this;
  }

  protected process(context: SubDocumentProcessContext): void {
    function replace(start: number, end: number) {
      // context.s.remove(start, end);

      // take just that substring
      const segment = context.s.original.slice(start, end);

      // blank out all non-newline chars in it
      const blanked = segment.replace(/[^\n]/g, " ");

      // overwrite exactly that region
      context.s.update(start, end, blanked, { overwrite: true });
    }

    // remove everyting before block

    for (const block of context.blocks) {
      // if (block.block.tag.type !== "style") {
      if (this.block.blocks.indexOf(block) === -1) {
        // // DELETE the whole block
        // context.s.remove(
        //   start,
        //   block.block.tag.pos.close.end
        // );
        // context.s.update(
        //   block.block.tag.pos.open.start,
        //   block.block.tag.pos.close.end,
        //   Array.from({
        //     length:
        //       block.block.tag.pos.close.end - block.block.tag.pos.open.start,
        //   })
        //     .map(() => "")
        //     .join("")
        // );

        replace(block.block.tag.pos.open.start, block.block.tag.pos.close.end);
      } else {
        // context.s.remove(
        //   block.block.tag.pos.open.start,
        //   block.block.tag.pos.open.end
        // );
        // context.s.remove(
        //   block.block.tag.pos.close.start,
        //   block.block.tag.pos.close.end
        // );

        replace(block.block.tag.pos.open.start, block.block.tag.pos.open.end);
        replace(block.block.tag.pos.close.start, block.block.tag.pos.close.end);

        // context.s.update(
        //   block.block.tag.pos.open.start,
        //   block.block.tag.pos.open.end,
        //   Array.from({
        //     length:
        //       block.block.tag.pos.open.end - block.block.tag.pos.open.start,
        //   })
        //     .map(() => "")
        //     .join("")
        // );

        // context.s.update(
        //   block.block.tag.pos.close.start,
        //   block.block.tag.pos.close.end,
        //   Array.from({
        //     length:
        //       block.block.tag.pos.close.end - block.block.tag.pos.close.start,
        //   })
        //     .map(() => "")
        //     .join("")
        // );
      }
    }
  }
}
