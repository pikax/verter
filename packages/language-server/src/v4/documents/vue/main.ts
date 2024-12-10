import { SourceMapConsumer } from "source-map-js";
import {
  Position,
  Range,
  TextDocument,
  TextDocumentContentChangeEvent,
} from "vscode-languageserver-textdocument";

import { createContext, ParseContext, VerterSFCBlock } from "@verter/core";
import { ContextProcessor } from "../../processor/types";
import { processRender } from "../../processor/render/render";
import { processOptions } from "../../processor/options";
import {
  generatedPositionFor,
  generatedRangeFor,
  originalPositionFor,
  originalRangeFor,
} from "../../utils";
import {
  isVerterVirtual,
  pathToUri,
  pathToVerterVirtual,
  uriToPath,
  uriToVerterVirtual,
} from "../utils";
import { processBundle } from "../../processor/bundle/bundle";
import { isVueSubDocument } from "../../processor/utils";

import { Bundle as MagicStringBundle } from "magic-string";
import { MagicString } from "vue/compiler-sfc";

type BlockId = "bundle" | "template" | "script" | "style";

const processors = {
  bundle: {
    uri: (parent) => parent + ".bundle.ts",
    // uri: (parent) => parent + ".tsx",
    process: processBundle,
  },
  script: {
    uri: (parent) => parent + ".options.ts",
    process: processOptions,
  },
  template: {
    uri: (parent) => parent + ".render.tsx",
    process: processRender,
  },
  style: {
    uri: (parent) => parent + ".style.css",
    process: (context) => {
      const s = context.s.clone();

      const block = context.blocks.find((x) => x.tag.type === "style")?.block;

      if (block) {

        s.remove(block.loc.end.offset, s.original.length);
        s.remove(0, block.loc.start.offset);
      }
      // throw new Error("Not implemented");
      return {
        s,
        languageId: "css",
        // todo this is incorrect, a SFC can have multiple syles
        content: s.toString() || "/* TEMP */",
        // content: `/* TODO FIX THIS FILE! */\nexport default {}`,
        filename: context.filename + ".wip.style.css",
      } as any;
    },
  },
} satisfies Record<BlockId, ContextProcessor>;

function getBlockId(block: VerterSFCBlock): BlockId | string {
  switch (block.tag.type) {
    case "template":
      return "template";
    case "script":
      return "script";
    case "style":
      return "style";
    default:
      return block.tag.type;
  }
}

function createDocumentFromBlock(block: VerterSFCBlock, doc: VueDocument) {
  const blockId = getBlockId(block);
  const processor = processors[blockId];
  return new VueSubDocument(doc, processor, blockId);
}

export class VueDocument implements TextDocument {
  static fromFilepath(
    filepath: string,
    content: string | (() => string),
    shouldParse = false
  ) {
    return VueDocument.fromUri(pathToUri(filepath), content, shouldParse);
  }
  static fromUri(
    uri: string,
    content: string | (() => string),
    shouldParse = false
  ) {
    const isString = typeof content === "string";
    const doc = TextDocument.create(
      uri,
      "vue",
      -2,
      isString ? content : "/* VERTER CONTENT NOT LOADED */\n"
    );
    return VueDocument.fromTextDocument(
      doc,
      shouldParse,
      isString ? undefined : content
    );
  }

  static fromTextDocument(
    doc: TextDocument,
    shouldParse = false,
    loadDocumentContent?: () => string
  ) {
    const vuedoc = new VueDocument(doc, loadDocumentContent);
    if (shouldParse) {
      vuedoc.syncVersion();
    }
    return vuedoc;
  }

  get bundleDoc() {
    return this.subDocuments.bundle;
  }
  private constructor(
    private _doc: TextDocument,
    private _loadDocumentContent?: () => string
  ) {
    this.subDocuments.bundle = new VueSubDocument(
      this,
      processors.bundle,
      "bundle"
    );
    this.subDocuments.script = new VueSubDocument(
      this,
      processors.script,
      "script"
    );
    this.subDocuments.template = new VueSubDocument(
      this,
      processors.template,
      "template"
    );
  }

  overrideDoc(doc: TextDocument) {
    this._doc = doc;
    this._lastVersion = -2;
    if (this._context || this._doc.version >= 0) {
      this.syncVersion(true);
    }
  }

  override(text: string, version?: number) {
    return this.update(
      [
        {
          text,
          range: {
            start: this.positionAt(0),
            end: this.positionAt(Number.MAX_SAFE_INTEGER),
          },
        },
      ],
      version ?? this.version + 1
    );
  }

  update(changes: TextDocumentContentChangeEvent[], version: number) {
    // todo maybe update subdocuments as well and only sync them when needed
    TextDocument.update(this._doc, changes, version);

    const changesForBlock: Record<
      BlockId | string,
      TextDocumentContentChangeEvent[]
    > = {};

    for (const change of changes) {
      const blocks = this.getBlocksForChange(change);
      for (const block of blocks) {
        const id = getBlockId(block);
        if (!changesForBlock[id]) {
          changesForBlock[id] = [];
        }
        changesForBlock[id].push(change);
      }
    }
    for (const [id, changes] of Object.entries(changesForBlock)) {
      const subDoc = this.subDocuments[id];
      if (subDoc) {
        subDoc.update(changes, version);
      }
    }
    return this._doc;
  }

  protected getBlocksForChange(change: TextDocumentContentChangeEvent) {
    if (!this.context) return [];
    const range = "range" in change ? change.range : undefined;
    // if there's no range, we need to update all blocks
    if (!range || !("rangeLength" in change)) {
      return this.context.blocks;
    }
    const blocks = this.context.blocks.filter((block) => {
      const startOffset = this.offsetAt(range.start);
      const endOffset = this.offsetAt(range.end);

      const blockStart = block.tag.pos.open.start;
      const blockEnd = block.tag.pos.close.end;

      return startOffset < blockEnd && endOffset > blockStart;
    });
    return blocks;
  }

  protected getBlockForPosition(pos: Position) {
    const offset = this.offsetAt(pos);
    return this.context.blocks.find((block) => {
      const blockStart = block.tag.pos.open.start;
      const blockEnd = block.tag.pos.close.end;

      return offset >= blockStart && offset <= blockEnd;
    });
  }

  // TODO this should return many blocks
  // the check should be done if the block has the position when getting with the map
  getDocumentForPosition(pos: Position) {
    const block = this.getBlockForPosition(pos);

    if (!block) {
      return null;
    }

    const id = getBlockId(block);
    return this.subDocuments[id];
  }

  languageId = "vue";

  get uri() {
    return this._doc.uri;
  }
  get version() {
    return this._doc.version;
  }
  get lineCount() {
    return this._doc.lineCount;
  }
  private _lastVersion = -2;

  subDocuments: Record<BlockId | string, VueSubDocument> = {};

  get subDocumentPaths() {
    return Object.values(this.subDocuments).map((x) => x.uri);
  }

  preview() {
    const bundle = new MagicStringBundle();
    for (const doc of Object.values(this.subDocuments)) {
      bundle.append("// start: " + doc.uri + "\n\n");
      bundle.addSource({
        filename: doc.uri,
        content: new MagicString(doc.getText()),
      });
      bundle.append("// end: " + doc.uri + "\n\n");
    }

    return bundle;
  }

  getTextFromFile(uri: string, range?: Range) {
    if (isVueSubDocument(uri)) {
      let subDoc = this.getDocument(uri);

      return subDoc.getText(range);
    } else {
      return this.bundleDoc.getText(range);
      // return this._doc.getText(range);
    }
  }

  getDocument(uri: string): VueSubDocument | undefined {
    if (!isVerterVirtual(uri)) {
      uri = uriToVerterVirtual(uri);
    }

    if (uri.endsWith(".vue")) {
      return this.bundleDoc;
    }

    // TODO probably check if the URI is part of this parent
    if (uri.endsWith(".options.ts") || uri.endsWith(".options.js")) {
      return this.subDocuments.script;
    }
    for (const doc of Object.values(this.subDocuments)) {
      if (doc.uri === uri) {
        return doc;
      }
    }

    return undefined;
  }

  getText(range?: Range): string {
    return this._doc.getText(range);
  }
  positionAt(offset: number): Position {
    return this._doc.positionAt(offset);
  }
  offsetAt(position: Position): number {
    return this._doc.offsetAt(position);
  }

  private _context: ParseContext;
  get context(): ParseContext {
    this.syncVersion();
    return this._context;
  }

  protected syncVersion(syncAll = false) {
    if (this._lastVersion === this.version) {
      return;
    }
    this._lastVersion = this.version;

    if (this._loadDocumentContent) {
      const text = this._loadDocumentContent();
      this.update([{ text }], 1);
      this._loadDocumentContent = undefined;
    }

    const context = createContext(this.getText(), uriToPath(this.uri), {});
    this._context = context;
    console.log("created context ", context.filename);

    const blockIds: Array<BlockId | string> = [];
    for (const block of context.blocks) {
      const id = getBlockId(block);
      blockIds.push(id);
      let subDoc = this.subDocuments[id];
      if (!subDoc) {
        subDoc = createDocumentFromBlock(block, this);
        this.subDocuments[id] = subDoc;
      }

      if (syncAll) {
        subDoc.syncVersion();
      }
    }

    // remove unknown blocks
    const toRemove = Object.keys(this.subDocuments).filter(
      (id) => blockIds.indexOf(id) === -1
    );
    for (const id of toRemove) {
      if (id === "bundle") continue;
      delete this.subDocuments[id];
    }
  }
}

export class VueSubDocument implements TextDocument {
  private _doc: TextDocument;
  constructor(
    private _parent: VueDocument,
    private _processor: ContextProcessor,
    private _blockId: string,
    initialContent?: string
  ) {
    const processorFilename = _processor.uri(_parent.uri);

    const virtualUri = uriToVerterVirtual(processorFilename);

    if (_blockId === "style") {
      // TODO improve this
      this._doc = TextDocument.create(
        virtualUri,
        "css",
        -1,
        // "// PLACEHOLDER TO BE POPULATED BY VUE DOCUMENT\n"
        initialContent || "// PLACEHOLDER TO BE POPULATED BY VUE DOCUMENT\n"
      );
    } else {
      this._doc = TextDocument.create(
        virtualUri,
        processorFilename.endsWith(".tsx") ? "tsx" : "typescript",
        -1,
        initialContent || "// PLACEHOLDER TO BE POPULATED BY VUE DOCUMENT\n"
      );
    }
  }

  get blockId() {
    return this._blockId;
    // return getBlockId(this._block);
  }

  get extension() {
    switch (this.languageId) {
      case "javascript":
        return ".js";
      case "jsx":
        return ".jsx";
      case "typescript":
        return ".ts";
      case "tsx":
        return ".tsx";
      case "css":
        return ".css";
    }

    return ".verter.unknown";
  }
  get languageId() {
    return this._doc.languageId;
  }
  get uri() {
    // todo this should be the virtual uri
    return this._doc.uri;
  }
  get version() {
    return this._doc.version;
  }
  get lineCount() {
    return this._doc.lineCount;
  }

  private _sourceMapConsumer: SourceMapConsumer | undefined;

  private _lastProcessedResult:
    | ReturnType<ContextProcessor["process"]>
    | undefined;

  isInsideBindingReturn(offset: number) {
    if ("bindingReturn" in this._lastProcessedResult) {
      return (
        // @ts-expect-error TODO type
        this._lastProcessedResult.bindingReturn.start <= offset &&
        // @ts-expect-error TODO type
        this._lastProcessedResult.bindingReturn.end >= offset
      );
    }
    return false;
  }

  getText(range?: Range): string {
    this.syncVersion();
    return this._doc.getText(range);
  }

  getOriginalText(): string {
    return this._lastProcessedResult?.s.original;
  }

  positionAt(offset: number): Position {
    return this._doc.positionAt(offset);
  }
  offsetAt(position: Position): number {
    return this._doc.offsetAt(position);
  }

  toGeneratedPosition(pos: Position): Position {
    return generatedPositionFor(this._sourceMapConsumer!, pos);
  }
  toGeneratedRange(range: Range): Range {
    return generatedRangeFor(this._sourceMapConsumer!, range);
  }

  toGeneratedOffsetFromPosition(pos: Position): number {
    const position = this.toGeneratedPosition(pos);
    return this.offsetAt(position);
  }

  toOriginalPosition(pos: Position): Position {
    return originalPositionFor(this._sourceMapConsumer!, pos);
  }
  toOriginalOffset(offset: number): number {
    const originalPosition = this.toOriginalPositionFromOffset(offset);
    return this._parent.offsetAt(originalPosition);
  }

  toOriginalPositionFromOffset(offset: number): Position {
    const position = this.positionAt(offset);
    return this.toOriginalPosition(position);
  }

  toOriginalRange(range: Range): Range {
    return originalRangeFor(this._sourceMapConsumer!, range);
  }

  update(changes: TextDocumentContentChangeEvent[], version: number) {
    if (!this._sourceMapConsumer) {
      this.syncVersion();
      return this._doc;
    }

    this.process();

    // let shouldProcess = false;
    // for (const change of changes) {
    //   // partialy update
    //   if ("range" in change) {
    //     const generatedRange = this.toGeneratedRange(change.range);

    //     console.log("to update", generatedRange, this._doc.getText());
    //     TextDocument.update(
    //       this._doc,
    //       [
    //         {
    //           range: generatedRange,
    //           text: change.text,
    //         },
    //       ],
    //       version
    //     );

    //     // todo there's must be a way to trigger another process
    //     // because there might be a big change in the source that needs it
    //     console.log(this._doc.getText());
    //     shouldProcess = true;
    //   } else {
    //     shouldProcess = true;
    //   }
    // }
    // if (shouldProcess) {
    //   this.process();
    // }

    return this._doc;
  }

  syncVersion() {
    if (this.version === this._parent.version) {
      if (
        this._lastProcessedResult.s.original !== this._parent.context.s.original
      ) {
        debugger;
      }

      return;
    }
    this.process();
  }

  process() {
    try {
      const { s, filename, languageId, content } = (this._lastProcessedResult =
        this._processor.process(this._parent.context));

      const map = s.generateMap({ hires: true, includeContent: true });
      this._sourceMapConsumer = new SourceMapConsumer(map as any);

      const uri = pathToVerterVirtual(filename);

      // if the language changes we need to update
      if (languageId !== this._doc.languageId) {
        this._doc = TextDocument.create(
          uri,
          languageId,
          this._doc.version + 1,
          content
        );
      } else {
        TextDocument.update(
          this._doc,
          [
            {
              range: {
                start: this._doc.positionAt(0),
                end: this._doc.positionAt(Number.MAX_SAFE_INTEGER),
              },
              text: this._lastProcessedResult.content,
            },
          ],
          this._doc.version
        );
      }
    } catch (e) {
      console.error(e);
      debugger;
    }
  }
}
