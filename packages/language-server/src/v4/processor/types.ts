import { ParseScriptContext } from "@verter/core";
import { MagicString } from "vue/compiler-sfc";

export type ContextProcessor = {
  uri: (parentUri: string) => string;

  process(context: ParseScriptContext): {
    languageId: string;
    filename: string;

    loc: {
      source: string;
    };

    s: MagicString;
    content: string;
  };
};
