import {
  Position,
  Range,
  TextDocument,
} from "vscode-languageserver-textdocument";
import { VerterDocument } from "../verter";
import { createContext, ParseContext } from "@verter/core";
import { uriToPath } from "../../utils";

export class VueDocument extends VerterDocument {
  static create(uri: string, version: number, content: string) {
    return new VueDocument(uri, version, content);
  }

  /**
   * trigger document updates
   */
  private _dirty = false;

  private _context: ParseContext | null = null;
  get context() {
    return (
      this._context ??
      (this._context = createContext(this.getText(), uriToPath(this.uri)))
    );
  }

  protected constructor(uri: string, version: number, content: string) {
    super(uri, "vue", version, content);
  }

  update(content: string, version?: number): void {
    this._dirty = true;
    this._context = null;
    super.update(content, version);
  }
}
