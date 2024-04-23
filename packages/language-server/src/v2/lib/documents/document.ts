import { SourceMapConsumer } from "source-map-js";
import {
  Position,
  Range,
  TextDocument,
  TextEdit,
} from "vscode-languageserver-textdocument";
import { MagicString } from "vue/compiler-sfc";
import { importVueCompiler } from "../../../importPackages";
import { createBuilder, mergeFull } from "@verter/core";

import { basename } from "path";
import { uriToVerterVirtual } from "./utils";
import { URI } from "vscode-uri";

export interface VueDocumentTemplate {
  content: string;
  map?: ReturnType<MagicString["generateMap"]>;
  mapConsumer: SourceMapConsumer;
}

export class VueDocument implements TextDocument {
  static fromTextDocument(doc: TextDocument, shouldParse = false) {
    const vuedoc = new VueDocument(
      doc,
      TextDocument.create(uriToVerterVirtual(doc.uri), "tsx", doc.version, "")
    );
    if (shouldParse) {
      vuedoc.parse();
    }
    return vuedoc;
  }

  static create(uri: string, content: string, shouldParse = false) {
    const doc = TextDocument.create(uri, "vue", -1, content);
    return VueDocument.fromTextDocument(doc, shouldParse);
  }
  languageId = "vue";

  protected _name: string;
  get version() {
    return this._doc.version;
  }
  get lineCount() {
    return this._doc.lineCount;
  }
  get uri() {
    return this._doc.uri;
  }
  get name() {
    return this._name ?? (this._name = basename(this.uri));
  }
  protected _template?: VueDocumentTemplate;
  get template() {
    return this.parse();
  }

  _virtualUri: string;
  get virtualUri() {
    return this._virtualUri;
  }

  protected _lastVersion = -1;

  private constructor(
    private _doc: TextDocument,
    private _compiledDoc: TextDocument
  ) {
    this.overrideDoc(this._doc);
  }

  // replaces document with the new one
  overrideDoc(doc: TextDocument) {
    this._doc = doc;

    this._virtualUri = uriToVerterVirtual(doc.uri);

    // if there was an template assigned override it and
    // parse again
    if (this._template) {
      this._template = undefined;
      this.parse();
    }
  }

  getText(range?: Range | undefined): string {
    return this._doc.getText(range);
  }
  positionAt(offset: number): Position {
    return this._doc.positionAt(offset);
  }
  offsetAt(position: Position): number {
    return this._doc.offsetAt(position);
  }

  getParsedText(range?: Range | undefined): string {
    try {
      this.parse()
    } catch (e) {
      console.error('ff', e)
      debugger
      this.parse()
    }
    return this._compiledDoc.getText(range);
  }

  toParsedPosition(position: Position): Position {
    const { column, line } = this.template.mapConsumer.generatedPositionFor({
      line: position.line + 1,
      column: position.character,
      source: ".",
    });

    const s = this.template.mapConsumer.generatedPositionFor({
      line: position.line,
      column: position.character,
      source: ".",
    });
    return {
      character: column,
      line: line - 1,
    };
  }

  toParsedOffset(offset: number) {
    const parsedPosition = this.toParsedPosition(this.positionAt(offset));
    return this._compiledDoc.offsetAt(parsedPosition);
  }

  toParsedRange(range: Range): Range {
    const start = this.toParsedPosition(range.start);
    const end = this.toParsedPosition(range.end);
    return {
      start,
      end,
    };
  }

  parsedOffsetFromPosition(position: Position): number {
    const pos = this.toParsedPosition(position);
    return this._compiledDoc.offsetAt(pos);
  }

  originalPosition(parsedOffset: number) {
    const pos = this._compiledDoc.positionAt(parsedOffset);
    const originalPos = this.template.mapConsumer.originalPositionFor({
      column: pos.character,
      line: pos.line + 1,
    });
    return {
      character: originalPos.column,
      line: originalPos.line - 1,
    };
  }

  originalOffset(parsedOffset: number) {
    const pos = this.originalPosition(parsedOffset);
    return this.offsetAt(pos);
  }

  applyEditsToCompiled(edits: TextEdit[]): string {
    const processedEdits = edits.map((x) => ({
      newText: x.newText,
      range: this.toParsedRange(x.range),
    }));

    return TextDocument.applyEdits(this._compiledDoc, processedEdits);
  }

  protected parse(): VueDocumentTemplate {
    // if version hasn't changed
    if (this._lastVersion === this._doc.version && this._template) {
      return this._template;
    }

    const compiler = importVueCompiler(this.uri)!;
    const builder = createBuilder({});

    const content = this.getText();
    const parsed = compiler.parse(content, {
      filename: this.name,
      sourceMap: true,
      ignoreEmpty: false,
      templateParseOptions: {
        parseMode: "sfc",
      },
    });

    const { locations, context } = builder.fromCompiled(parsed);
    const result = mergeFull(locations, context);

    TextDocument.update(
      this._compiledDoc,
      [
        {
          range: {
            start: this._compiledDoc.positionAt(0),
            end: this._compiledDoc.positionAt(Number.MAX_SAFE_INTEGER),
          },
          text: result.content,
        },
      ],
      this._doc.version
    );

    this._lastVersion = this._doc.version;

    return (this._template = {
      content: result.content,
      map: result.map,
      mapConsumer: new SourceMapConsumer(result.map!),
    });
  }
}
