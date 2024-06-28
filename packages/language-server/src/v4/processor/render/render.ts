import { MagicString, SFCTemplateBlock } from "vue/compiler-sfc";
import {
  ParseContext,
  type ParseScriptContext,
  PrefixSTR,
  TemplateBuilder,
  VerterASTBlock,
  VerterSFCBlock,
  getAccessors,
} from "@verter/core";
import { relative } from "path/posix";

import { BindingContextExportName, DefaultOptions } from "../options/index.js";
import { getBlockFilename } from "../utils.js";
import {
  transpileTemplate,
  WalkResult,
} from "@verter/core/dist/plugins/index.js";

export const FunctionExportName = PrefixSTR("Render");

export function processRender(context: ParseContext) {
  const accessors = getAccessors();
  const filename = getBlockFilename("render", context);
  const optionsFilename = getBlockFilename("options", context, true);
  const genericInfo = context.generic;

  let scriptBlock: VerterASTBlock | null = null;

  //   const componentAccessor = PrefixSTR("Component");
  //   const defineComponent = PrefixSTR("defineComponent");

  const s = context.s.clone();

  const isAsync = context.isAsync;

  // remove unknown blocks
  const SUPPORTED_BLOCKS = new Set(["template"]);
  const blocks: VerterASTBlock[] = [];

  for (const block of context.blocks) {
    if (SUPPORTED_BLOCKS.has(block.block.type)) {
      blocks.push(block);
    } else {
      // keep generic block
      if (genericInfo && block.block.attrs.generic === genericInfo.source) {
        scriptBlock = block;
      } else {
        s.remove(block.tag.pos.open.start, block.tag.pos.close.end);
      }
    }
  }

  const mainBlock = blocks[0] as
    | (VerterASTBlock & { block: SFCTemplateBlock })
    | undefined;

  if (!mainBlock || !mainBlock.block.ast) {
    s.append(`export function ${FunctionExportName}() { return <></> }`);
  } else {
    // const generic = context.generic ? `<${context.generic}>` : "";

    const declarations: WalkResult[] = [];

    transpileTemplate(context.sfc.template.ast, s, {
      declarations,
    });

    const variables = {
      defineComponent: PrefixSTR("defineComponent"),
      ShallowUnwrapRef: PrefixSTR("ShallowUnwrapRef"),
      component: PrefixSTR("Component"),

      ComponentOptions: PrefixSTR("Options"),

      isConstructor: PrefixSTR("isConstructor"),
      UnionToIntersection: PrefixSTR("UnionToIntersection"),
      ExtractInstance: PrefixSTR("ExtractInstance"),
    };

    const vueImports: Array<[string, string, boolean?]> = [
      ["renderList", accessors.renderList],
      ["normalizeClass", accessors.normalizeClass],
      ["normalizeStyle", accessors.normalizeStyle],
      ["defineComponent", variables.defineComponent],
      ["ShallowUnwrapRef", variables.ShallowUnwrapRef, true],
    ];

    const imports = context.isSetup
      ? `import { ${vueImports.map(
          ([i, n, t]) => `${t ? "type " : ""}${i} as ${n}`
        )} } from "vue";
import { ${BindingContextExportName}, ${DefaultOptions} } from "${optionsFilename}"
`
      : `
import { defineComponent as ${variables.defineComponent}, ShallowUnwrapRef as ${variables.ShallowUnwrapRef} } from "vue";
import { ${DefaultOptions} } from "${optionsFilename}";
`;

    const helpers = context.isSetup
      ? `
      
// Helper to retrieve slots definitions
declare function ___VERTER_extract_Slots<CompSlots>(comp: { new(): { $slots: CompSlots } }, slots?: undefined): CompSlots;
declare function ___VERTER_extract_Slots<CompSlots, Slots extends Record<string, any> = {}>(comp: { new(): { $slots: CompSlots } }, slots: Slots): Slots;

declare function ___VERTER___SLOT_CALLBACK<T>(slot: (...args: T[]) => any): (cb: ((...args: T[]) => any))=>void;
declare function ___VERTER___eventCb<TArgs extends Array<any>, R extends ($event: TArgs[0],) => any>(event: TArgs, cb: R): R;`
      : `
declare function ${variables.isConstructor}<T extends { new(): Record<string, any> }>(o: T | unknown): true;
type ${variables.UnionToIntersection}<U> =
  (U extends any ? (x: U) => void : never) extends ((x: infer I) => void) ? I : never;
declare function ${variables.ExtractInstance}<T>(o: T): T extends { new(): infer P } ? P : never;

// Helper to retrieve slots definitions
declare function ___VERTER_extract_Slots<CompSlots>(comp: { new(): { $slots: CompSlots } }, slots?: undefined): CompSlots;
declare function ___VERTER_extract_Slots<CompSlots, Slots extends Record<string, any> = {}>(comp: { new(): { $slots: CompSlots } }, slots: Slots): Slots;

declare function ___VERTER___SLOT_CALLBACK<T>(slot: (...args: T[]) => any): (cb: ((...args: T[]) => any))=>void;
declare function ___VERTER___eventCb<TArgs extends Array<any>, R extends ($event: TArgs[0],) => any>(event: TArgs, cb: R): R;
`;

    const contextContent = context.isSetup
      ? `
const ${variables.component} = new ${DefaultOptions}()
const ${BindingContextExportName}CTX = ${
          isAsync ? "await " : ""
        }${BindingContextExportName}${
          genericInfo ? `<${genericInfo.names.join(",")}>` : ""
        }()
const ${accessors.ctx} = {
    ...${variables.component},
    ...({} as ${
      variables.ShallowUnwrapRef
    }<typeof ${BindingContextExportName}CTX>),
};


const ${accessors.comp} =  {
  //...({} as ExtractRenderComponents<typeof ___VERTER___ctx>),
  ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } }),
  ...___VERTER___ctx
}
`
      : `
const ${variables.ComponentOptions} = ${DefaultOptions};
const ${variables.component} = ${variables.ExtractInstance}(${variables.ComponentOptions})

const ${accessors.ctx} = {
    ...${variables.component},
}

const ${accessors.comp} =  {
  //...({} as ExtractRenderComponents<typeof ___VERTER___ctx>),
  ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } }),
  ...___VERTER___ctx
}
`;

    // we might not need to map it tbh
    if (genericInfo) {
      const preGeneric = `\nexport ${
        isAsync ? "async " : ""
      }function ${FunctionExportName}`;
      const postGeneric = `() {\n`;

      const generic = genericInfo?.source;
      // generic information will be kept intact for the source-map
      if (typeof generic === "string") {
        // get <script ... >
        const tagContent = s.original.slice(
          scriptBlock.tag.pos.open.start,
          scriptBlock.tag.pos.open.end
        );

        const genericIndex =
          tagContent.indexOf(generic) + scriptBlock.tag.pos.open.start;

        // replace before generic with `preGeneric`
        s.overwrite(
          scriptBlock.tag.pos.open.start,
          genericIndex,
          preGeneric + "<"
        );

        // replace after generic with `postGeneric`
        s.overwrite(
          genericIndex + generic.length,
          scriptBlock.block.loc.start.offset,
          ">" + postGeneric
        );
      } else {
        s.overwrite(
          scriptBlock.tag.pos.open.start,
          scriptBlock.tag.pos.open.end,
          preGeneric + postGeneric
        );
      }

      s.remove(
        scriptBlock.block.loc.start.offset,
        scriptBlock.tag.pos.close.end
      );

      s.move(
        scriptBlock.tag.pos.open.start,
        scriptBlock.tag.pos.open.end,
        mainBlock.tag.pos.open.start
      );

      s.overwrite(mainBlock.tag.pos.open.start, mainBlock.tag.pos.open.end, "");
    } else {
      s.overwrite(
        mainBlock.tag.pos.open.start,
        mainBlock.tag.pos.open.end,
        `export function ${isAsync ? "async " : ""}${FunctionExportName}() {\n`
      );
    }

    s.overwrite(
      mainBlock.tag.pos.close.start,
      mainBlock.tag.pos.close.end,
      "\n</>\n}"
    );

    s.appendLeft(mainBlock.block.loc.start.offset, contextContent);
    s.appendLeft(mainBlock.block.loc.start.offset, "\nreturn <>\n");

    s.prepend(helpers + "\n");
    s.prepend(imports + "\n");
  }

  return {
    languageId: "tsx",

    filename,

    loc: {
      source: s.original,
    },

    s,
    content: s.toString(),
  };
}
