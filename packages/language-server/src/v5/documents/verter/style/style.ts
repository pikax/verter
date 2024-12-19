import { LanguageService, Stylesheet } from "vscode-css-languageservice";
import { VerterDocument } from "../verter.js";

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

  update(content: string, version?: number): void {
    this._stylesheet = null;
    super.update(content, version);
  }
}
