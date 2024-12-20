import { TextDocument } from "vscode-languageserver-textdocument";
import { VueDocument } from "../vue.js";
import { VerterDocument } from "../../verter.js";

export abstract class VueSubDocument extends VerterDocument {
  private constructor(
    uri: string,
    private _parent: VueDocument,
    languageId: string,
    version: number
  ) {
    super(uri, languageId, version, "");
    // this._doc = TextDocument.create(_uri, _languageId, -1, "");
  }
  get parent() {
    return this._parent;
  }
}
