import {
  Position,
  Range,
  TextDocument,
} from "vscode-languageserver-textdocument";
import { VerterDocument } from "../verter";
import { parser, ParserResult } from "@verter/core";
import { createSubDocumentUri, pathToUri, uriToPath } from "../../utils";
import { processBlocks, ProcessedBlock } from "./utils";
import { VueSubDocument } from "./sub/sub";
import { createSubDocument } from "./sub/utils";
import { pathToUrl } from "../../../../utils";
import { VueBundleDocument } from "./sub";

export class VueDocument extends VerterDocument {
  static create(uri: string, content: string, version?: number) {
    return new VueDocument(uri, version ?? -1, content);
  }

  private _dirty = false;

  private _context: ParserResult | null = null;
  get context() {
    if (this._dirty || !this._context) {
      this._context = parser(this.getText(), uriToPath(this.uri));
    }
    return this._context;
  }

  private _blocks: ProcessedBlock[] | null = null;
  get blocks() {
    if (!this._blocks || this._dirty) {
      this._blocks = processBlocks(this.uri, this.context.blocks);
    }
    return this._blocks;
  }

  private _docs: VueSubDocument[] | null = null;
  get docs() {
    if (this._dirty || !this._docs) {
      const blocks = this.blocks;
      const docs = this._docs ?? [];
      const processed = new Set<any>();

      for (let i = 0; i < docs.length; i++) {
        const d = docs[i];
        if (blocks.some((x) => x.uri === d.uri)) {
          continue;
        }
        // remove not found docs
        blocks.splice(i, 1);
      }

      for (const block of blocks) {
        const uri = block.uri;
        let s = docs?.find((x) => x.uri === uri);
        if (!s) {
          s = createSubDocument(this, block);
          if (!s) {
            console.error("couldn't create document for block", block);
            continue;
          }
          docs.push(s);
        }
        processed.add(s);
      }

      this._docs = docs;
    }

    return this._docs;
  }

  protected constructor(uri: string, version: number, content: string) {
    super(uri, "vue", version, content);
  }

  update(content: string, version?: number) {
    this._dirty = true;
    // this._context = null;
    // this._blocks  = null

    super.update(content, version);
    return this;
  }

  getSubDoc(filename: string) {
    const uri = pathToUri(filename);
    return this.docs.find((x) => x.uri === uri);
  }

  docsForPos(position: Position) {
    return this.docs
      .map((x) => {
        const o = x.toGeneratedOffsetFromPosition(position);
        if (o > 0 && o < x.getText().length) {
          return {
            doc: x,
            offset: o,
          };
        }

        return undefined;
      })
      .filter((x) => !!x);
  }
}
