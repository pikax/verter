import {
  Position,
  Range,
  TextDocument,
} from "vscode-languageserver-textdocument";
import { VerterDocument } from "../verter";
import { createContext, ParseContext } from "@verter/core";
import { uriToPath } from "../../utils";
import { processBlocks, ProcessedBlock } from "./utils";

export class VueDocument extends VerterDocument {
  static create(uri: string, content: string, version?: number) {
    return new VueDocument(uri, version ?? -1, content);
  }

  // /**
  //  * trigger document updates
  //  */
  // private _dirty = false;

  private _context: ParseContext | null = null;
  get context() {
    return (
      this._context ??
      (this._context = createContext(this.getText(), uriToPath(this.uri)))
    );
  }

  private _blocks: null | ProcessedBlock[] = null;

  get blocks() {
    return (
      this._blocks ??
      (this._blocks = processBlocks(this.uri, this.context.blocks))
    );
  }

  // private _documents =

  protected constructor(uri: string, version: number, content: string) {
    super(uri, "vue", version, content);
  }

  update(content: string, version?: number): void {
    // this._dirty = true;
    this._context = null;
    this._blocks = null;
    super.update(content, version);
  }
}
