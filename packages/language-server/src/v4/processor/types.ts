import { ParseContext } from "@verter/core";
import { MagicString } from "vue/compiler-sfc";

export type ContextProcessor = {
  uri: (parentUri: string) => string;

  process(context: ParseContext): {
    languageId: string;
    filename: string;

    loc: {
      source: string;
    };

    s: MagicString;
    content: string;
  };
};
