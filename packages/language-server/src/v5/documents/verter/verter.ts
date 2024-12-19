import { TextEdit } from "vscode-languageserver-protocol";
import {
  Position,
  Range,
  TextDocument,
} from "vscode-languageserver-textdocument";

export class VerterDocument implements TextDocument {
  static create(
    uri: string,
    languageId: string,
    content: string,
    version?: number
  ) {
    return new VerterDocument(uri, languageId, version ?? -1, content);
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

  protected doc: TextDocument;

  protected constructor(
    _uri: string,
    _languageId: string,
    _version: number,
    _content: string
  ) {
    this.doc = TextDocument.create(_uri, _languageId, _version, _content);
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

  update(content: string, version?: number) {
    const d = this.doc;

    this.doc = TextDocument.update(
      d,
      [
        {
          text: content,
          range: {
            start: d.positionAt(0),
            end: d.positionAt(Number.MAX_SAFE_INTEGER),
          },
        },
      ],
      version ?? this.version + 1
    );
  }

  applyEdits(edits: TextEdit[]) {
    return TextDocument.applyEdits(this.doc, edits);
  }
}
