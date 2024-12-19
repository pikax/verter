import ts from "typescript";
import { VerterDocument } from "../verter.js";

export type LanguageTypescript = "ts" | "tsx" | "js" | "jsx";

export class TypescriptDocument extends VerterDocument {
  static create(
    uri: string,
    languageId: LanguageTypescript,
    version: number,
    content: string
  ) {
    return new TypescriptDocument(uri, languageId, version, content);
  }

  private _snapshot: ts.IScriptSnapshot | null;

  protected constructor(
    uri: string,
    languageId: LanguageTypescript,
    version: number,
    content: string
  ) {
    super(uri, languageId, version, content);
  }

  get snapshot() {
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
