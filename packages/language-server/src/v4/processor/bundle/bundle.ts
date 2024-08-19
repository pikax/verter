import { ParseContext, PrefixSTR } from "@verter/core";
import { MagicString } from "vue/compiler-sfc";

import {
  ResolveProps,
  ComponentExport,
  DefaultOptions,
  ResolveSlots,
  ResolveExtraProps,
} from "./../options/index.js";
import { FunctionExportName } from "./../render/index.js";
import { getBlockFilename } from "../utils";

export function processBundle(context: ParseContext) {
  const filename = context.filename + ".bundle.ts";

  const optionsFile = getBlockFilename("options", context, true);
  const renderFile = getBlockFilename("render", context, true);
  const s = new MagicString("");

  const ctx = {
    DefineComponent: PrefixSTR("DefineComponent"),
  };
  const generic = context.generic;
  const isAsync = context.isAsync;

  s.append(
    `import { DefineComponent as ${
      ctx.DefineComponent
    }, DefineProps } from "vue";
import { ${DefaultOptions}, ${ResolveProps}, ${ResolveSlots}, ${ResolveExtraProps} } from "${optionsFile}";
// import { ${FunctionExportName} } from "${renderFile}";

type PartialUndefined<T> = {
  [P in keyof T]: undefined extends T[P] ? P : never;
}[keyof T] extends infer U extends keyof T
  ? Omit<T, U> & Partial<Pick<T, U>>
  : T;

type ProcessProps<T> = T extends DefineProps<infer U, infer BKeys> ? PartialUndefined<U> : T;


declare const Comp : typeof ${DefaultOptions} & { new${
      generic ? `<${generic.declaration}>` : ""
    }(): { 
  $props: (ProcessProps<ReturnType<typeof ${ResolveProps}${
      generic ? `<${generic.sanitisedNames.join(",")}>` : ""
    }>${
      isAsync ? " extends Promise<infer P> ? P : {}" : ""
    }>) & ReturnType<typeof ${ResolveExtraProps}${
      generic ? `<${generic.sanitisedNames.join(",")}>` : ""
    }> ;
  $slots: ReturnType<typeof ${ResolveSlots}${
      generic ? `<${generic.sanitisedNames.join(",")}>` : ""
    }> extends ${isAsync ? "Promise<" : ""}infer P${
      isAsync ? ">" : ""
    } ? P extends P & 1 ? {} : P : never;
} };


export default Comp;
`
  );

  return {
    languageId: "typescript",
    filename,

    loc: {
      source: s.original,
    },

    s,
    content: s.toString(),
  };
}
