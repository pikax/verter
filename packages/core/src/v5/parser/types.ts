import { SFCBlock, SFCScriptBlock, SFCTemplateBlock } from "@vue/compiler-sfc";
import { VerterSFCBlock } from "../utils";
import { ParsedTemplateResult } from "./template";
import { ParsedScriptResult } from "./script/index.js";

export interface VerterParseContext {
  filename: string;
  source: string;
}

export interface ParsedBlockTemplate {
  type: "template";
  lang: "vue";

  block: VerterSFCBlock<SFCTemplateBlock>;
  result: ParsedTemplateResult;
}

export interface ParsedBlockScript {
  type: "script";
  lang: "js" | "jsx" | "ts" | "tsx";

  isMain: boolean;
  block: VerterSFCBlock<SFCScriptBlock>;
  result: ParsedScriptResult;
}

export interface ParsedBlockUnknown {
  type: Omit<string, "template" | "script">;
  lang: string;
  block: VerterSFCBlock<SFCBlock>;
  result: null;
}

export type ParsedBlock =
  | ParsedBlockTemplate
  | ParsedBlockScript
  | ParsedBlockUnknown;
