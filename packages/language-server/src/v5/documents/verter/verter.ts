import { TextEdit } from "vscode-languageserver-protocol";
import {
  Position,
  Range,
  TextDocument,
  TextDocumentContentChangeEvent,
} from "vscode-languageserver-textdocument";

export class VerterDocument implements TextDocument {
  static createDoc(
    uri: string,
    languageId: string,
    version: number,
    content: string
  ) {
    return new VerterDocument(uri, languageId, version, content);
  }
  get uri() {
    return this.doc.uri;
  }
  get languageId() {
    return this.doc.languageId;
  }
  get version() {
    return this.doc.version;
  }
  get lineCount() {
    return this.doc.lineCount;
  }

  private _doc: TextDocument;
  protected get doc() {
    return this._doc;
  }

  protected constructor(
    _uri: string,
    _languageId: string,
    _version: number,
    _content: string
  ) {
    this._doc = TextDocument.create(_uri, _languageId, _version, _content);
  }

  getText(range?: Range): string {
    return this.doc.getText(range);
  }
  positionAt(offset: number): Position {
    return this.doc.positionAt(offset);
  }
  offsetAt(position: Position): number {
    return this.doc.offsetAt(position);
  }

  update(content: string | TextDocumentContentChangeEvent[], version?: number) {
    const d = this.doc;
    TextDocument.update(
      d,
      typeof content === "string"
        ? [
            {
              text: content,
              range: {
                start: d.positionAt(0),
                end: d.positionAt(Number.MAX_SAFE_INTEGER),
              },
            },
          ]
        : content,
      version ?? d.version + 1
    );
    return this;
  }

  applyEdits(edits: TextEdit[]) {
    return TextDocument.applyEdits(this.doc, edits);
  }
}
