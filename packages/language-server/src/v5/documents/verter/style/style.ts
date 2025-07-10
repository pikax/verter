import { LanguageService, Stylesheet } from "vscode-css-languageservice";
import { VerterDocument } from "../verter.js";
import { TextDocumentContentChangeEvent } from "vscode-languageserver-textdocument";

export type LanguageStyle = "css" | "scss" | "less";

export class StyleDocument extends VerterDocument {
  static create(
    languageService: LanguageService,
    uri: string,
    languageId: LanguageStyle,
    version: number,
    content: string
  ) {
    return new StyleDocument(
      languageService,
      uri,
      languageId,
      version,
      content
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
    private _languageService: LanguageService,
    uri: string,
    languageId: LanguageStyle,
    version: number,
    content: string
  ) {
    super(uri, languageId, version, content);
  }

  update(content: string | TextDocumentContentChangeEvent[], version?: number) {
    this._stylesheet = null;
    super.update(content, version);
    return this;
  }
}
