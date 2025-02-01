import { ProcessedBlock } from "../utils";
import { VueDocument } from "../vue";
import { VueOptionsDocument, VueBundleDocument } from "./typescript";

export function createSubDocument(parent: VueDocument, block: ProcessedBlock) {
  switch (block.type) {
    case "bundle": {
      return VueBundleDocument.create(
        block.uri,
        parent,
        block.languageId as any,
        parent.version
      );
    }
    case "script": {
      return VueOptionsDocument.create(
        block.uri,
        parent,
        block.languageId as any,
        parent.version
      );
    }
  }
}
