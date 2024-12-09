import {
  extractIdentifiers,
  MagicString,
  SFCTemplateBlock,
  walk,
  walkIdentifiers,
} from "vue/compiler-sfc";
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

import {
  BindingContextExportName,
  FullContextExportName,
  DefaultOptions,
  SlotsPropertyName,
  ResolveSlots,
  PropsPropertyName,
  ResolveProps,
  VueSetupMacros,
  ResolveRenderProps,
  ResolveEmits,
} from "../options/index.js";
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

  const importIdentifiers = new Set<string>();

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
        // s.update(block.tag.pos.open.start, block.tag.pos.close.end, "", {
        //   overwrite: true,
        // });
        if (block.ast) {
          for (const it of block.ast.body) {
            switch (it.type as any) {
              case "ImportDeclaration": {
                // case "TsExportAssignment": // case "TsNamespaceExportDeclaration": // case "ExportNamedDeclaration": // case "ExportDeclaration": // case "ExportAllDeclaration":
                const offset = block.block.loc.start.offset;
                const startIndex = it.start + offset;
                const endIndex = it.end + offset;

                walkIdentifiers(
                  // @ts-expect-error
                  it,
                  (n) => {
                    if (n) {
                      importIdentifiers.add(n.name);
                    }
                  },
                  true
                );

                try {
                  s.move(startIndex, endIndex, 0);
                } catch (e) {
                  console.error(e);
                  debugger;
                }
                break;
              }
              default: {
                const offset = block.block.loc.start.offset;
                const startIndex = it.start + offset;
                const endIndex = it.end + offset;
                s.remove(startIndex, endIndex);
              }
            }
          }
          s.remove(block.tag.pos.open.start, block.tag.pos.open.end);
          s.remove(block.tag.pos.close.start, block.tag.pos.close.end);
        } else {
          s.remove(block.block.loc.start.offset, block.block.loc.end.offset);
        }
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

    transpileTemplate(context.sfc.template.ast, s, {
      declarations: context.locations,
    });

    const variables = {
      defineComponent: PrefixSTR("defineComponent"),
      ShallowUnwrapRef: PrefixSTR("ShallowUnwrapRef"),
      component: PrefixSTR("Component"),

      ComponentOptions: PrefixSTR("Options"),

      isConstructor: PrefixSTR("isConstructor"),
      UnionToIntersection: PrefixSTR("UnionToIntersection"),
      ExtractInstance: PrefixSTR("ExtractInstance"),

      GlobalComponents: PrefixSTR("GlobalComponents"),
      ComponentPublicInstance: PrefixSTR("ComponentPublicInstance"),
      IntrinsicElementAttributes: PrefixSTR("IntrinsicElementAttributes"),
      VueComponent: PrefixSTR("VueComponent"),
      Ref: PrefixSTR("Ref"),

      ExtractRenderComponents: PrefixSTR("ExtractRenderComponents"),
      ExtractComponent: PrefixSTR("ExtractComponent"),
    };

    const vueImports: Array<[string, string, boolean?]> = [
      ["renderList", accessors.renderList],
      ["normalizeClass", accessors.normalizeClass],
      ["normalizeStyle", accessors.normalizeStyle],
      ["defineComponent", variables.defineComponent],
      ["ShallowUnwrapRef", variables.ShallowUnwrapRef, true],
      ["GlobalComponents", variables.GlobalComponents, true],
      ["ComponentPublicInstance", variables.ComponentPublicInstance, true],
      ["Ref", variables.Ref, true],
      [
        "IntrinsicElementAttributes",
        variables.IntrinsicElementAttributes,
        true,
      ],
      ["Component", variables.VueComponent, true],
    ];

    /*
     ${
      // @ts-expect-error
      context.macroVariableName?.has("defineProps") ||
      // @ts-expect-error
      context.macroVariableName?.has("withDefaults")
        ? // use the original props, ShallowUnwrap them will break generics

          `${
            // @ts-expect-error
            context.macroVariableName.get("defineProps") ||
            // @ts-expect-error
            context.macroVariableName.get("withDefaults")
          } : {} as ReturnType<typeof ${
            isAsync ? "await " : ""
          }${ResolveProps}${
            genericInfo ? `<${genericInfo.names.join(",")}>` : ""
          }>,`
        : ""
    }
    */

    const macroVariableName: Map<VueSetupMacros, string> | undefined =
      // @ts-expect-error
      context.macroVariableName;

    const macrosVariables = macroVariableName
      ? ([
          macroVariableName.get("defineProps") ||
            macroVariableName.get("withDefaults"),
          macroVariableName.get("defineEmits"),
          macroVariableName.get("defineSlots"),
        ].filter(Boolean) as Array<string>)
      : [];

    // variables exposed by verter
    const verterVariables = [
      PrefixSTR("props"),
      PrefixSTR("withDefaultsProps"),
      PrefixSTR("emits"),
      PrefixSTR("slots"),
      PrefixSTR("models"),
    ];

    const ignoredExposed = [...verterVariables, ...macrosVariables];

    const globalPatch = getGlobalComponentsStr(variables);
    const imports = context.isSetup
      ? `
import 'vue/jsx';
import { ${vueImports.map(
          ([i, n, t]) => `${t ? "type " : ""}${i} as ${n}`
        )} } from "vue";
import { ${BindingContextExportName}, ${FullContextExportName}, ${DefaultOptions}, ${ResolveSlots}, ${ResolveRenderProps}, ${ResolveEmits}} from "${optionsFilename}";

${globalPatch}
`
      : `import 'vue/jsx';
// import { defineComponent as ${
          variables.defineComponent
        }, ShallowUnwrapRef as ${variables.ShallowUnwrapRef} } from "vue";
import { ${vueImports.map(
          ([i, n, t]) => `${t ? "type " : ""}${i} as ${n}`
        )} } from "vue";
import { ${DefaultOptions} } from "${optionsFilename}";

${globalPatch}
`;

    const helpers = context.isSetup
      ? `
      
// Helper to retrieve slots definitions
declare function ___VERTER_extract_Slots<CompSlots>(comp: { new(): { $slots: CompSlots } }, slots?: undefined): CompSlots;
declare function ___VERTER_extract_Slots<CompSlots, Slots extends Record<string, any> = {}>(comp: { new(): { $slots: CompSlots } }, slots: Slots): Slots;

declare function ___VERTER___SLOT_CALLBACK<T>(slot?: (...args: T[]) => any): (cb: ((...args: T[]) => any))=>void;
declare function ___VERTER___eventCb<TArgs extends Array<any>, R extends ($event: TArgs[0],) => any>(event: TArgs, cb: R): R;
declare function ___VERTER___AssertAny<T>(o: T): T extends T & 0 ? never : T;
declare function ___VERTER___SLOT_TO_COMPONENT<T>(o: T): T extends T & 0 ? never : [T] extends [{ [K in string]: {} }] ? (arg: T) => JSX.Element : [T] extends [(...args: infer A) => any] ? (scope: A[0]) => JSX.Element : T;
`
      : `
declare function ${variables.isConstructor}<T extends { new(): Record<string, any> }>(o: T | unknown): true;
type ${variables.UnionToIntersection}<U> =
  (U extends any ? (x: U) => void : never) extends ((x: infer I) => void) ? I : never;
declare function ${variables.ExtractInstance}<T>(o: T): T extends { new(): infer P } ? P : never;

// Helper to retrieve slots definitions
declare function ___VERTER_extract_Slots<CompSlots>(comp: { new(): { $slots: CompSlots } }, slots?: undefined): CompSlots;
declare function ___VERTER_extract_Slots<CompSlots, Slots extends Record<string, any> = {}>(comp: { new(): { $slots: CompSlots } }, slots: Slots): Slots;

declare function ___VERTER___SLOT_CALLBACK<T>(slot?: (...args: T[]) => any): (cb: ((...args: T[]) => any))=>void;
declare function ___VERTER___eventCb<TArgs extends Array<any>, R extends ($event: TArgs[0],) => any>(event: TArgs, cb: R): R;
declare function ___VERTER___AssertAny<T>(o: T): T extends T & 0 ? never : T;
declare function ___VERTER___SLOT_TO_COMPONENT<T>(o: T): T extends T & 0 ? never : [T] extends [{ [K in string]: {} }] ? (arg: T) => JSX.Element : [T] extends [(...args: infer A) => any] ? (scope: A[0]) => JSX.Element : T;
`;

    const contextContent = context.isSetup
      ? `
const ${variables.component} = new ${DefaultOptions}()
const ${BindingContextExportName}CTX = ${
          isAsync ? "await " : ""
        }${BindingContextExportName}${
          genericInfo ? `<${genericInfo.names.join(",")}>` : ""
        }();
const ${FullContextExportName}CTX = ${
          isAsync ? "await " : ""
        }${FullContextExportName}${
          genericInfo ? `<${genericInfo.names.join(",")}>` : ""
        }();

const ${ResolveRenderProps}CTX = ${
          isAsync ? "await " : ""
        }${ResolveRenderProps}${
          genericInfo ? `<${genericInfo.names.join(",")}>` : ""
        }();

const ${ResolveEmits}CTX = ${isAsync ? "await " : ""}${ResolveEmits}${
          genericInfo ? `<${genericInfo.names.join(",")}>` : ""
        }();

const ${accessors.ctx} = {
    ...${variables.component},
    ${
      importIdentifiers.size > 0
        ? Array.from(importIdentifiers.values()).join(", ") + ","
        : ""
    }


    // expose props access directly
    ...({} as Omit<typeof ${ResolveRenderProps}CTX, keyof typeof ${BindingContextExportName}CTX>),


   ...({} as ${
     variables.ShallowUnwrapRef
   }<Omit<typeof ${FullContextExportName}CTX, ${
          ignoredExposed.length
            ? ignoredExposed.map((x) => `"${x}"`).join(" | ")
            : ""
        }>>),
    ...({} as ${
      variables.ShallowUnwrapRef
    }<Omit<typeof ${BindingContextExportName}CTX, ${
          ignoredExposed.length
            ? ignoredExposed.map((x) => `"${x}"`).join(" | ")
            : ""
        }>>),

        ${
          macrosVariables.length > 0
            ? `// Do not shallowUnwrap macros
    ...({} as Pick<typeof ${BindingContextExportName}CTX, ${
                macrosVariables.length
                  ? macrosVariables.map((x) => `"${x}"`).join(" | ")
                  : ""
              }>),`
            : ""
        }
   

    $props: { ...${variables.component}.$props, ...${ResolveRenderProps}CTX },
    $emit: ${ResolveEmits}CTX,
    $slots: ${ResolveSlots}${
          genericInfo ? `<${genericInfo.names.join(",")}>` : ""
        }(),
};



const ${accessors.comp} =  {
  // ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } }),
  ...({} as ${variables.ExtractRenderComponents}<typeof ___VERTER___ctx>),
  // ...({} as ${variables.GlobalComponents}),
  // ...___VERTER___ctx
}
`
      : `
const ${variables.ComponentOptions} = ${DefaultOptions};
const ${variables.component} = ${variables.ExtractInstance}(${variables.ComponentOptions})

const ${accessors.ctx} = {
    ...${variables.component},
    // TODO handle components
}

const ${accessors.slot} = ${variables.component}.$slots;

const ${accessors.comp} =  {
  // ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } }),
  ...({} as ${variables.ExtractRenderComponents}<typeof ___VERTER___ctx>),
  // ...({} as ${variables.GlobalComponents}),
  // ...___VERTER___ctx
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
        `export ${isAsync ? "async " : ""}function ${FunctionExportName}() {\n`
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

    locations: context.locations,

    s,
    content: s.toString(),
  };
}

function getGlobalComponentsStr(
  variables: Record<
    | "Ref"
    | "ExtractComponent"
    | "VueComponent"
    | "IntrinsicElementAttributes"
    | "ExtractRenderComponents"
    | "ComponentPublicInstance"
    | "GlobalComponents",
    string
  >
) {
  const toImport = [
    "Suspense",
    "KeepAlive",
    "Transition",
    "TransitionGroup",
    "Teleport",
  ].map((x) => [x, PrefixSTR(x)]);
  //   return toImport
  //     .map(([i, n]) => `import { ${i} as ${n} } from "vue";`)
  //     .join("\n");
  const importStr = `import type {${toImport.map(
    ([name, prefix]) => `${name} as ${prefix}`
  )} } from "vue";`;

  const globalDeclaration = `declare module 'vue' {
  interface GlobalComponents {
${toImport.map(([name, prefix]) => `${name}: typeof ${prefix};`).join("\n")}
  }

  interface HTMLAttributes {
    'v-slot'?: {
      default: () => JSX.Element
    }
  }
}

// patching elements
declare global {
  namespace JSX {
    export interface IntrinsicClassAttributes<T> {
      'v-slot'?: (T extends { $slots: infer S } ? S : undefined) | ((c: T) => T extends { $slots: infer S } ? S : undefined)
    }
  }
}
  
declare function ___VERTER_renderSlot<T extends Array<any>>(slot: (...args: T) => any, cb: (...cb: T) => any): void;
declare function ___VERTER___template(): JSX.Element;

type ___VERTER___OmitNever<T> = {
    [K in keyof T as T[K] extends never ? never : K]: T[K];
};

type ${variables.ExtractComponent}<T> = T extends ${variables.Ref}<infer V>
  ? ${variables.ExtractComponent}<V>
  : T extends ${variables.VueComponent}<infer Props>
  ? T & { new(): { $props: Props & import('vue').VNode['props'] } }
  : T extends keyof ${variables.IntrinsicElementAttributes}
  ? { new (): { $props: ${variables.IntrinsicElementAttributes}[T] } }
  : never;

type ${variables.ExtractRenderComponents}<T> = ___VERTER___OmitNever<{
  [K in keyof Omit<T, keyof ${variables.ComponentPublicInstance}>]: ${
    variables.ExtractComponent
  }<T[K]>;
}> & ${variables.GlobalComponents} & {};
`;

  // type ExtractComponent<T> = T extends Ref<infer V>
  // ? ExtractComponent<V>
  // : T extends Component
  // ? T
  // : T extends keyof IntrinsicElementAttributes
  // ? { new (): { $props: IntrinsicElementAttributes[T] } }
  // : never;

  // type ExtractRenderComponents<T> = OmitNever<{
  //   [K in keyof Omit<T, keyof ComponentPublicInstance>]: ExtractComponent<T[K]>;
  // }> &
  //   GlobalComponents & {};

  return `${importStr}\n${globalDeclaration}`;
}
