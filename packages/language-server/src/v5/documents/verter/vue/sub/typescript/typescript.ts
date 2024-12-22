import ts from "typescript";
import { VueSubDocument } from "../sub.js";
import { VueDocument } from "../../vue.js";

export type LanguageTypescript = "ts" | "tsx" | "js" | "jsx";

export abstract class VueTypescriptDocument extends VueSubDocument {
  private _snapshot: ts.IScriptSnapshot | null;

  protected constructor(
    uri: string,
    parent: VueDocument,
    languageId: LanguageTypescript,
    version: number
  ) {
    super(uri, parent, languageId, version);
  }

  get snapshot() {
    if (this.parent.version !== this.version) {
      this._snapshot = null;
    }

    return (
      this._snapshot ??
      (this._snapshot = ts.ScriptSnapshot.fromString(this.getText()))
    );
  }

  update(content: string, version?: number): void {
    this._snapshot = null;
    super.update(content, version);
  }
}
