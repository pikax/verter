import { MagicString } from "@vue/compiler-sfc";
import { ParseContext } from "../parser";

export interface GeneratorContext {
  filename: string;
  s: MagicString;

  parsed: ParseContext;
  sharedBag: Record<string, unknown>;
}
