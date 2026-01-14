import { getStyleLanguageService } from "../../../../services/style";
import { ProcessedBlock } from "../utils";
import type { VueDocument } from "../vue";
import { VueStyleDocument } from "./style";
import {
  VueOptionsDocument,
  VueBundleDocument,
  VueRenderDocument,
  VueSingleDocument,
} from "./typescript";

export function createSubDocument(parent: VueDocument, block: ProcessedBlock) {
  switch (block.type) {
    case "single": {
      return VueSingleDocument.create(
        block.uri,
        parent,
        block.languageId as any,
        parent.version
      );
    }
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
    case "template": {
      return VueRenderDocument.create(
        block.uri,
        parent,
        block.languageId as any,
        parent.version
      );
    }

    case "style": {
      return VueStyleDocument.create(
        block.uri,
        parent,
        block.languageId,
        getStyleLanguageService(block.uri, block.languageId as any),
        parent.version,
        block
      );
    }
  }
}
