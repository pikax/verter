import { MagicString } from "vue/compiler-sfc";
import {
  type ParseScriptContext,
  PrefixSTR,
  TemplateBuilder,
  VerterSFCBlock,
  getAccessors,
} from "@verter/core";
import { relative } from "path/posix";

import {
  BindingContextExportName,
  DefaultOptions,
  ComponentExport,
  GenericOptions,
  OptionsExportName,
  genericProcess,
} from "../options/index.js";
import { getBlockFilename } from "../../../v3/processor/utils.js";

export const FunctionExportName = PrefixSTR("Render");

export function processRender(context: ParseScriptContext) {
  const accessors = getAccessors();
  const filename = getBlockFilename("render", context);
  const optionsFilename = getBlockFilename("options", context);

  //   const componentAccessor = PrefixSTR("Component");
  //   const defineComponent = PrefixSTR("defineComponent");

  const relativeScriptPath = relative(context.filename, optionsFilename);
  const s = context.s.clone();

  const isAsync = context.isAsync;

  // remove unknown blocks
  const SUPPORTED_BLOCKS = new Set(["template"]);
  const blocks: VerterSFCBlock[] = [];
  for (const block of context.blocks) {
    if (SUPPORTED_BLOCKS.has(block.tag.type)) {
      blocks.push(block);
    } else {
      s.remove(block.tag.pos.open.start, block.tag.pos.close.end);
    }
  }

  const mainBlock = blocks[0];

  if (context.template === null || !mainBlock) {
    s.append(`export function ${FunctionExportName}() { return <></> }`);
  } else {
    // const generic = context.generic ? `<${context.generic}>` : "";
    const generic = genericProcess(context);

    TemplateBuilder.process({
      ...context,
      s,
    });

    const variables = {
      defineComponent: PrefixSTR("defineComponent"),
      component: PrefixSTR("Component"),

      ComponentOptions: PrefixSTR("Options"),

      isConstructor: PrefixSTR("isConstructor"),
      UnionToIntersection: PrefixSTR("UnionToIntersection"),
      ExtractInstance: PrefixSTR("ExtractInstance"),
    };

    const imports = context.isSetup
      ? ``
      : `
import { defineComponent as ${variables.defineComponent} } from "vue";
import { ${OptionsExportName} } from "./${relativeScriptPath}";
`;

    const helpers = context.isSetup
      ? ``
      : `
declare function ${variables.isConstructor}<T extends { new(): Record<string, any> }>(o: T | unknown): true;
type ${variables.UnionToIntersection}<U> =
  (U extends any ? (x: U) => void : never) extends ((x: infer I) => void) ? I : never;
declare function ${variables.ExtractInstance}<T>(o: T): T extends { new(): infer P } ? P : never;
`;

    const contextContent = context.isSetup
      ? `
const ${variables.component} = new (${
          variables.defineComponent
        }(${DefaultOptions}))
const ${BindingContextExportName}CTX = ${
          isAsync ? "await " : ""
        }${BindingContextExportName}${
          context.generic ? generic.genericNames.join(",") : ""
        }()
const ${accessors.ctx} = {
    ...${variables.component},
    ...${
      isAsync && false
        ? `{} as typeof ${BindingContextExportName}CTX extends Promise<infer T> ? T : never`
        : `${BindingContextExportName}CTX`
    },
}
`
      : `
const ${variables.ComponentOptions} = ${variables.isConstructor}(${OptionsExportName}) ? ${OptionsExportName} : ${variables.defineComponent}(${OptionsExportName});
const ${variables.component} = ${variables.ExtractInstance}(${variables.ComponentOptions})

const ${accessors.ctx} = {
    ...${variables.component},
}
`;

    s.overwrite(
      mainBlock.tag.pos.open.start,
      mainBlock.tag.pos.open.end,
      `export function ${isAsync ? "async " : ""}${FunctionExportName}${
        context.generic ? `<${context.generic}>` : ""
      }() {\n`
    );
    s.overwrite(
      mainBlock.tag.pos.close.start,
      mainBlock.tag.pos.close.end,
      "\n</>\n}"
    );

    s.appendLeft(mainBlock.block.loc.start.offset, contextContent);
    s.appendLeft(mainBlock.block.loc.start.offset, "\nreturn <>\n");

    s.prepend(helpers + "\n");
    s.prepend(imports + "\n");

    // const prependContent = [
    //   `import { ${BindingContextExportName} } from './${relativeScriptPath}'`,
    //   `export function ${FunctionExportName}${
    //     context.generic ? `<${context.generic}>` : ""
    //   }() {`,
    //   `const ${accessors.ctx} = {
    //     ...{} as (${DefaultOptions}),
    //     ...${BindingContextExportName}${
    //     context.generic ? generic.genericNames.join(", ") : ""
    //   }(),`,
    //   "return <>",
    //   "",
    // ];
    // const cc = prependContent.join("\n");

    // console.log("prependContent", prependContent);
  }

  return {
    filename,

    loc: {
      source: s.original,
    },

    s,
    content: s.toString(),
  };
}
