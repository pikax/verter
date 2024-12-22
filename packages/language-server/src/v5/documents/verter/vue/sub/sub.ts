import { VueDocument } from "../vue.js";
import { VerterDocument } from "../../verter.js";
import MagicString from "magic-string";
import { ParseContext, VerterASTBlock } from "@verter/core";
import { SourceMapConsumer } from "source-map-js";
import { Position, Range } from "vscode-languageserver-textdocument";
import {
  generatedPositionFor,
  generatedRangeFor,
  originalPositionFor,
  originalRangeFor,
} from "../utils.js";

export interface SubDocumentProcessContext {
  s: MagicString;

  blocks: Array<VerterASTBlock>;
  parsed: ParseContext;
}

export abstract class VueSubDocument extends VerterDocument {
  protected constructor(
    uri: string,
    private _parent: VueDocument,
    languageId: string,
    version: number
  ) {
    super(uri, languageId, version, "");
  }

  get parent() {
    return this._parent;
  }

  private _sourceMapConsumer: SourceMapConsumer | null = null;
  protected get sourceMapConsumer() {
    if (this.version !== this._parent.version) {
      this._sourceMapConsumer = null;
    }
    return (
      this._sourceMapConsumer ?? (this._sourceMapConsumer = this.sync(true))
    );
    // if (this.version === this._parent.version && this._sourceMapConsumer) {
    //   return this._sourceMapConsumer;
    // }
    // return (this._sourceMapConsumer = this.sync());
  }

  protected _isSynching = false;
  protected sync(force?: boolean): SourceMapConsumer | null {
    const parent = this._parent;

    if (!force && this.sourceMapConsumer) {
      return this.sourceMapConsumer;
    }

    try {
      if (this._isSynching) return;
      this._isSynching = true;
      const context = parent.context;
      const block = parent.blocks.find((x) => x.uri === this.uri);
      if (!block) {
        throw new Error("Block not found!");
      }

      const s = context.s.clone();
      this.process({
        s,
        parsed: context,
        blocks: block.blocks,
      });

      this.update(s.toString(), this.parent.version);

      const consumer = new SourceMapConsumer(
        s.generateMap({
          hires: true,
          includeContent: true,
        }) as any
      );

      consumer.computeColumnSpans();

      return consumer;
    } finally {
      this._isSynching = false;
    }
  }

  update(content: string, version?: number): void {
    this._sourceMapConsumer = null;
    super.update(content, version);
  }

  toGeneratedPosition(pos: Position): Position {
    const p = generatedPositionFor(this.sourceMapConsumer, pos);

    if (!Number.isFinite(p.character)) {
      p.character = this.getText().length;
    }

    return p;
  }

  toGeneratedRange(range: Range): Range {
    const r = generatedRangeFor(this.sourceMapConsumer, range);
    if (!Number.isFinite(r.start.character)) {
      r.start.character = this.getText().length;
    }
    if (!Number.isFinite(r.end.character)) {
      r.end.character = this.getText().length;
    }

    return r;
  }

  toGeneratedOffsetFromPosition(pos: Position): number {
    const position = this.toGeneratedPosition(pos);
    return this.offsetAt(position);
  }

  toOriginalPosition(pos: Position): Position {
    return originalPositionFor(this.sourceMapConsumer, pos);
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
    return originalRangeFor(this._sourceMapConsumer, range);
  }

  getText(range?: Range): string {
    this.sync();
    return super.getText(range);
  }

  offsetAt(position: Position): number {
    this.sync();
    return super.offsetAt(position);
  }
  positionAt(offset: number): Position {
    this.sync();
    return super.positionAt(offset);
  }

  protected abstract process(context: SubDocumentProcessContext): void;
}
