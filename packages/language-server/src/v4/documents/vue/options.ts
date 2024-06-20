import {
  Position,
  Range,
  TextDocument,
} from "vscode-languageserver-textdocument";
import { VueDocument } from "./main.js";
import { ParseScriptContext } from "@verter/core";
import { processRender } from "../../processor/render/render.js";

export class VueOptionsDocument implements TextDocument {
  static fromVueDocument(doc: VueDocument) {
    doc.context.filename;
  }

  uri: string;
  languageId: string;
  lineCount: number;

  get version() {
    return this._vueDoc.version;
  }

  private lastProcessedVersion = -2;
  private _doc: TextDocument;

  private constructor(private _vueDoc: VueDocument) {
    this._doc = TextDocument.create(_vueDoc.uri, "vue", _vueDoc.version, "");
  }

  getText(range?: Range): string {
    throw new Error("Method not implemented.");
  }
  positionAt(offset: number): Position {
    throw new Error("Method not implemented.");
  }
  offsetAt(position: Position): number {
    throw new Error("Method not implemented.");
  }

  /**
   * This syncs the original document, if versions are different
   * it will reprocess the document
   */
  private syncVersion() {
    if (this._vueDoc.version === this.lastProcessedVersion) {
      return;
    }

    this.lastProcessedVersion = this._vueDoc.version;
  }
}
