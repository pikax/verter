import {
  Position,
  Range,
  TextDocument,
} from "vscode-languageserver-textdocument";
import { VerterDocument } from "../verter";
import { parser, ParserResult } from "@verter/core";
import { uriToPath } from "../../utils";
import { processBlocks, ProcessedBlock } from "./utils";

export class VueDocument extends VerterDocument {
  static create(uri: string, content: string, version?: number) {
    return new VueDocument(uri, version ?? -1, content);
  }

  private _context: ParserResult | null = null;
  get context() {
    return (
      this._context ??
      (this._context = parser(this.getText(), uriToPath(this.uri)))
    );
  }

  get blocks() {
    return processBlocks(this.uri, this.context.blocks);
  }

  protected constructor(uri: string, version: number, content: string) {
    super(uri, "vue", version, content);
  }

  update(content: string, version?: number): void {
    // this._dirty = true;
    this._context = null;
    super.update(content, version);
  }
}
