import { TextDocument } from "vscode-languageserver-textdocument";
import { VueDocument } from "../vue.js";

export abstract class VueSubDocument {
  private _doc: TextDocument;
  private constructor(
    private _uri: string,
    private _parent: VueDocument,
    private _languageId: string
  ) {
    this._doc = TextDocument.create(_uri, _languageId, -1, "");
  }

  get doc() {
    return this._doc;
  }

  get languageId() {
    return this._languageId;
  }

  get uri() {
    return this._uri;
  }

  get parent() {
    return this._parent;
  }
}
