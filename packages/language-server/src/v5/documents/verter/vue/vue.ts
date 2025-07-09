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
import { VueBundleDocument, VueStyleDocument } from "./sub";

export class VueDocument extends VerterDocument {
  static create(uri: string, content: string, version?: number) {
    return new VueDocument(uri, version ?? -1, content);
  }

  private _docs_dirty = false;

  private _context: ParserResult | null = null;
  get context() {
    if (!this._context) {
      this._context = parser(this.getText(), uriToPath(this.uri));
    }
    return this._context;
  }

  private _blocks: ProcessedBlock[] | null = null;
  get blocks() {
    if (!this._blocks) {
      this._blocks = processBlocks(this.uri, this.context.blocks);
    }
    return this._blocks;
  }

  private _docs: VueSubDocument[] | null = null;
  get docs() {
    if (this._docs_dirty || !this._docs) {
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
        } else if (s instanceof VueStyleDocument) {
          s.block = block;
        }
        processed.add(s);
      }

      this._docs = docs;
      this._docs_dirty = false;
    }

    return this._docs;
  }

  protected constructor(uri: string, version: number, content: string) {
    super(uri, "vue", version, content);
  }

  update(content: string, version?: number) {
    this._docs_dirty = true;
    this._context = null;
    this._blocks = null;

    super.update(content, version);
    return this;
  }

  getSubDoc(filename: string) {
    const uri = pathToUri(filename);
    return this.docs.find((x) => x.uri === uri);
  }

  docsForPos(position: Position, all = false) {
    const offset = this.doc.offsetAt(position);
    const main = this.blocks.find((x) => {
      // bundle should never be used as a main
      if (x.type === "bundle") {
        return false;
      }
      return x.blocks.some((b) => {
        const pos = b.block.tag.pos;
        return pos.open.start <= offset && pos.close.end >= offset;
      });
    });
    return this.docs
      .map((x) => {
        const o = x.toGeneratedOffsetFromPosition(position);
        if (o > 0 && o < x.getText().length) {
          return {
            doc: x,
            offset: o,
            main: x.uri === main?.uri,
            isBundler: x instanceof VueBundleDocument,
          };
        }

        return undefined;
      })
      .filter((x) => all || x?.main)
      .filter((x) => !!x);
  }
}
