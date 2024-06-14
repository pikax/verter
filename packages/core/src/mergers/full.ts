import { MagicString } from "@vue/compiler-sfc";
import {
  ImportItem,
  LocationByType,
  LocationType,
  ParseScriptContext,
  TypeLocationDeclaration,
} from "../plugins";
import {
  VerterSFCBlock,
  extractBlocksFromDescriptor,
  retrieveHTMLComments,
} from "../utils/sfc";

const possibleExports = [
  "export default /*#__PURE__*/_",
  "export default /*#__PURE__*/",
  "export default ",
];

export function mergeFull(
  locations: LocationByType,
  context: ParseScriptContext
): {
  locations: LocationByType;
  source: string;
  content: string;
  map: ReturnType<MagicString["generateMap"]>;
  context: ParseScriptContext;
} {
  sanitiseMap(locations);

  const { s, generic, isSetup, sfc } = context;
  const source = s.original;

  const allBlocks = extractBlocksFromDescriptor(sfc.descriptor);
  const blocks: VerterSFCBlock[] = [];
  const SUPPORTED_BLOCKS = new Set(["template", "script"]);

  const genericInfo = genericProcess(context, locations);

  {
    // move non Generated imports to the top
    // this need to be before other moves to respesct the imports to top
    for (const { node, offset } of locations.import.filter(
      (x) => !x.generated
    )) {
      // node AST are offset based on the content not on the file
      s.move(offset + node.loc.start.index, offset + node.loc.end.index, 0);
    }

    // handle generated imports from scriptSetup
    if (sfc.descriptor.scriptSetup) {
      const content =
        context.script?.content ?? context.sfc.descriptor.scriptSetup.content;

      // this is the added imports from vue
      const firstLineEndIndex = content.indexOf("\n");
      const line = content.slice(0, firstLineEndIndex + 1);
      s.prependLeft(0, line);
    }
  }

  {
    const allHTMLComments = retrieveHTMLComments(source);

    const comments = allHTMLComments.filter(
      ({ start, end }) =>
        !allBlocks.some(
          (b) => b.tag.pos.open.start <= end && start <= b.tag.pos.close.end
        )
    );

    comments.forEach(({ start, end }) => s.remove(start, end));
  }

  // remove unknown blocks
  for (const block of allBlocks) {
    if (SUPPORTED_BLOCKS.has(block.tag.type)) {
      blocks.push(block);
    } else {
      s.remove(block.tag.pos.open.start, block.tag.pos.close.end);
    }
  }

  const startScript =
    sfc.descriptor.scriptSetup ??
    sfc.descriptor.script ??
    blocks.find((x) => x.tag.type === "script")?.block;

  const startScriptIndex = startScript?.loc.start.offset;
  const endScriptIndex = startScript?.loc.end.offset;

  // if is setup, there's a change to have 2 script tags
  // we need to bring the non-setup to the top
  if (isSetup) {
    const scriptBlocks = blocks.filter((x) => x.block.type === "script");
    if (scriptBlocks.length > 1) {
      const secondBlock = scriptBlocks[1];
      if (secondBlock.block.attrs.setup !== true) {
        s.move(
          secondBlock.tag.pos.open.start,
          secondBlock.tag.pos.close.end,
          scriptBlocks[0].tag.pos.open.start
        );
      }
    }
  }

  {
    // move template to before </script>
    // this is better because we can use the component funciton generic
    // to fix some of the types
    const templateIndex = blocks.findIndex((x) => x.block.type === "template");
    if (templateIndex >= 0) {
      const templateBlock = blocks[templateIndex];

      s.move(
        templateBlock.tag.pos.open.start,
        templateBlock.tag.pos.close.end,
        endScriptIndex
      );
    }
  }

  for (const block of blocks) {
    processBlock(block, context, locations);
  }

  // handling slots
  {
    locations.declaration.push(
      {
        type: LocationType.Declaration,
        generated: true,
        context: "global",
        declaration: {
          content: `

import type {
  EmitsOptions as ___VERTER__EmitsOptions, SlotsType as ___VERTER__SlotsType, DefineComponent as ___VERTER__DefineComponent,
  ComponentOptions as ___VERTER__ComponentOptions, PropType as ___VERTER__PropType, ObjectEmitsOptions as __VERTER__ObjectEmitsOptions,
} from 'vue'


type EmitsToProps<T extends ___VERTER__EmitsOptions> = T extends (
  event: infer E extends string,
  ...args: infer Args
) => any
  ? {
    [K in \`on\${Capitalize<E>}\`]?: (...args: Args) => any
  }
  : T extends string[]
  ? {
    [K in \`on\${Capitalize<T[number]>}\`]?: (...args: any[]) => any
  }
  : T extends __VERTER__ObjectEmitsOptions
  ? {
    [K in \`on\${Capitalize<string & keyof T>}\`]?: K extends \`on\${infer C}\`
    ? (
      ...args: T[Uncapitalize<C>] extends (...args: infer P) => any
        ? P
        : T[Uncapitalize<C>] extends null
        ? any[]
        : T[Uncapitalize<C>] extends Array<any>
        ? T[Uncapitalize<C>]
        : never
    ) => any
    : never
  }
  : {}

type ObjectToComponentProps<T> = [T] extends [Readonly<Array<string>>]
  ? T
  : [T] extends [Record<string, any>]
  ? {
    [K in keyof T]-?: T[K] extends Function
    ? {
      type: ___VERTER__PropType<T[K]>
      required: undefined extends T[K] ? false : true
    }
    : {
      type: ___VERTER__PropType<Exclude<T[K], undefined>>
      required: undefined extends T[K] ? false : true
    }
  }
  : {}

export type ___VERTER__DeclareComponent<
  Props = {},
  Data extends Record<string, any> = {},
  Emits extends ___VERTER__EmitsOptions = {},
  Slots extends Record<string, any> = {},
  Options = {},
> =
  (___VERTER__DefineComponent<
    ObjectToComponentProps<Props>,
    Data,
    {},
    {},
    {},
    {},
    {},
    {},
    string,
    {},
    Props & EmitsToProps<Emits>,
    {},
    ___VERTER__SlotsType<Slots>
  > & (Options extends infer O extends Record<PropertyKey, {}> ? O : {})) extends infer C ? C & { new(): { $props: { 'v-slot'?: (c: C & { $slots: Slots }) => any } } } : never

  declare function ___VERTER___AssertAny<T>(o: T extends T & 0 ? never : T): T extends T & 0 ? never : T

  declare function ___VERTER___PATCH_TYPE<T>(options: T): T
  declare function ___VERTER___PATCH_TYPE<T, Props, Data extends Record<string, any>, Emits, Slots extends Record<PropertyKey, any>>(options: T, props: Props, data?: Data, emits?: Emits, slots?: Slots)
    : ___VERTER__DeclareComponent<Props, Data, __VERTER__EmitsToObject<Emits>, Slots, T>
  type __VERTER__EmitsToObject<T> = T extends (event: infer E extends PropertyKey, ...args: infer Args) => any ? { [K in E]: (...args: Args) => true } : {}

          
// Helper to retrieve slots definitions
declare function ___VERTER_extract_Slots<CompSlots>(comp: { new(): { $slots: CompSlots } }, slots?: undefined): CompSlots;
declare function ___VERTER_extract_Slots<CompSlots, Slots extends Record<string, any> = {}>(comp: { new(): { $slots: CompSlots } }, slots: Slots): Slots;

declare function ___VERTER___SLOT_CALLBACK<T>(slot: (...args: T[]) => any): (cb: ((...args: T[]) => any))=>void;
declare function ___VERTER___eventCb<TArgs extends Array<any>, R extends ($event: TArgs[0],) => any>(event: TArgs, cb: R): R;`,
        },
      },
      // TODO add defineSlots definition here as ___VERTER_DEFINE_SLOTS___
      // also introduce generating slots

      ...(isSetup
        ? [
            {
              type: LocationType.Declaration,
              generated: true,
              context: "post",

              declaration: {
                name: "___VERTER_DEFINE_SLOTS___",
                type: "const",
                content: `{
              ${locations.slots
                .map(
                  (x) =>
                    x.varName ||
                    s.original.slice(
                      x.expression.start + context.script.loc.start.offset,
                      x.expression.end + context.script.loc.start.offset
                    )
                )
                .map((x) => `...(${x})`)
                .join(",\n")}
            }`,
              },
            } as TypeLocationDeclaration,
          ]
        : []),
      {
        type: LocationType.Declaration,
        generated: true,
        context: "post",
        declaration: {
          name: "___VERTER___slot",
          type: "const",
          content: `___VERTER_extract_Slots(___VERTER_COMP___${
            isSetup ? `, ___VERTER_DEFINE_SLOTS___` : ""
          })`,
        },
      }
    );
  }

  // adding some imports and variable context
  {
    const exposedCtx: string[] = [];
    const rawToExposeCtx: string[] = [];

    const globalUser = locations.declaration.filter(
      (x) => !x.generated && x.context === "global"
    );

    if (globalUser.length) {
      globalUser.forEach((x) => {
        s.move(
          startScriptIndex + x.node.loc.start.index,
          startScriptIndex + x.node.loc.end.index,
          0
        );
      });
    }

    if (context.isSetup) {
      rawToExposeCtx.push(
        ...locations.import
          .filter((x) => !x.generated)
          .flatMap(
            (x) =>
              x.items?.map((i) => i.alias ?? i.name) ??
              // TODO maybe handle alias
              x.node.specifiers.map(
                (x) =>
                  x.local?.name ??
                  ("imported" in x ? (x.imported as any).name : null)
              )
          )
          .filter(Boolean)
      );

      const declarations = locations.declaration.filter((x) => !x.generated);

      rawToExposeCtx.push(
        ...declarations
          .flatMap(
            (x) =>
              x.declaration.name ??
              ("declarations" in x.node
                ? x.node.declarations?.flatMap(
                    (x) =>
                      ("name" in x.id ? x.id.name : null) ??
                      ("properties" in x.id
                        ? x.id.properties.map((p) =>
                            "name" in p
                              ? p.name
                              : "value" in p
                              ? (p.value as any).name
                              : null
                          )
                        : null)
                  )
                : null) ??
              (x.node as any).id?.name
          )
          .filter(Boolean)
      );

      if (rawToExposeCtx.length > 0) {
        locations.import.push({
          type: LocationType.Import,
          generated: true,
          from: "vue",
          node: undefined,
          items: [{ name: "unref", alias: "__VERTER__unref" }],
        });
      }

      // handle props
      // if (locations.props.length) {
      const props = [];
      for (const it of locations.props) {
        if (it.varName) {
          props.push(it.varName);
        } else if (it.expression) {
          s.prependLeft(
            it.expression.start + context.script.loc.start.offset,
            "const ___VERTER_PROPS_DECLARATION___ = "
          );
          props.push("___VERTER_PROPS_DECLARATION___");
        }
      }

      // generate a named prop that we can reference
      locations.declaration.push({
        type: LocationType.Declaration,
        context: "post",
        generated: true,
        declaration: {
          type: "const",
          name: "___VERTER_PROPS___",
          content: `{
              ${props.map((x) => `...(${x})`).join(",\n")}
            }`,
        },
      });
      // const propsType = "typeof ___VERTER_PROPS___";

      // to prevent typescript from merging types, we need to omit
      // the exposed values, props have a lower priority in the context
      exposedCtx.push(
        // `...({} as ${
        //   rawToExposeCtx.length
        //     ? `Omit<${propsType}, ${rawToExposeCtx
        //         .map((x) => JSON.stringify(x))
        //         .join("|")}>`
        //     : propsType
        // })`
        `...___VERTER_PROPS___`
      );
      // }

      // if (locations.emits.length) {
      const emits = [];
      for (const it of locations.emits) {
        if (it.varName) {
          emits.push(it.varName);
        } else if (it.node) {
          s.prependLeft(
            it.node.start + context.script.loc.start.offset,
            "const ___VERTER_EMITS_DECLARATION___ = "
          );
          emits.push("___VERTER_EMITS_DECLARATION___");
        }
      }
      // (x.expression &&
      //   s.original.slice(
      //     x.expression.start + context.script.loc.start.offset,
      //     x.expression.end + context.script.loc.start.offset
      //   ))
      locations.declaration.push({
        type: LocationType.Declaration,
        context: "post",
        generated: true,
        declaration: {
          type: "const",
          name: "___VERTER_EMITS___",
          content: emits[0] ?? "{}",
          // content: `{
          //     ${emits.map((x) => `...(${x})`).join(",\n")}
          //   }`,
        },
      });
      // }

      exposedCtx.push(
        ...rawToExposeCtx.map((x) => `${x}: __VERTER__unref(${x})`)
      );
    }

    const instanceType = `InstanceType<typeof ___VERTER_COMP___>`;

    exposedCtx.unshift(
      `...({} as ${
        rawToExposeCtx.length || locations.props.length
          ? `Omit<${instanceType}, ${rawToExposeCtx
              .map((x) => JSON.stringify(x))
              .concat(
                locations.props.length
                  ? ["keyof typeof ___VERTER_PROPS___"]
                  : []
              )
              .join("|")}>`
          : instanceType
      })`
    );

    locations.declaration.push({
      type: LocationType.Declaration,
      generated: true,
      context: "post",
      declaration: {
        type: "const",
        name: "___VERTER___ctx",
        content: `{ ${exposedCtx.join(",\n")} }`,
      },
    });

    locations.declaration.push({
      type: LocationType.Declaration,
      generated: true,
      context: "post",
      declaration: {
        type: "const",
        name: "___VERTER___comp",
        content: `{ 
            //...({} as ExtractRenderComponents<typeof ___VERTER___ctx>),
            ...({} as { [K in keyof JSX.IntrinsicElements]: { new(): { $props: JSX.IntrinsicElements[K] } } }),
            ...___VERTER___ctx
          }`,
      },
    });

    // locations.import.push({
    //   type: LocationType.Import,
    //   generated: true,
    //   from: "verter:helper",
    //   asType: true,
    //   items: [
    //     {
    //       name: "ExtractRenderComponents",
    //       // TODO prepend __VERTER__ to avoid collisions
    //       alias: "ExtractRenderComponents",
    //     },
    //   ],
    // });

    if (generic) {
      const sanitisedGenericNames = genericInfo.sanitisedNames.join(", ");
      // const genericNames = genericInfo.genericNames.join(", ");

      locations.declaration.push({
        type: LocationType.Declaration,
        generated: true,
        context: "finish",
        declaration: {
          type: "const",
          name: "__VERTER__RESULT",
          content: `___VERTER_COMPONENT__<${genericInfo.genericNames.map(
            () => "any"
          )}>();`,
        },
      });

      // this will remove the instance and constructor from __VERTER_COMPONENT__
      const removeInstanceFromType = `ReturnType<typeof ___VERTER_COMPONENT__<${sanitisedGenericNames}>> extends ${
        context.isAsync ? "Promise<infer Comp>" : "infer Comp"
      } ? Pick<Comp, keyof Comp> : never`;

      const betterInstance = `{ new<${genericInfo.instance}>(): { $props: { 
        /* props info here */

       }} }`;
      locations.declaration.push({
        type: LocationType.Declaration,
        generated: true,
        context: "finish",
        declaration: {
          type: "type",
          name: `__VERTER_RESULT_TYPE__<${genericInfo.component}>`,
          content: `(${removeInstanceFromType}) & ${betterInstance};`,
        },
      });
    } else {
      locations.declaration.push({
        type: LocationType.Declaration,
        generated: true,
        context: "finish",
        declaration: {
          type: "const",
          name: "__VERTER__RESULT",
          content: `___VERTER_COMPONENT__()${
            context.isAsync
              ? "as unknown as ReturnType<typeof ___VERTER_COMPONENT__> extends Promise<infer V> ? V : ReturnType<typeof ___VERTER_COMPONENT__>"
              : ""
          };`,
        },
      });
    }

    // locations.import.push({
    //   type: LocationType.Import,
    //   generated: true,
    //   from: "vue",
    //   asType: true,
    //   items: [
    //     {
    //       name: "IntrinsicElementAttributes",
    //       alias: "___VERTER_IntrinsicElementAttributes",
    //     },
    //   ],
    // });
  }

  // append imports
  {
    const generatedImports = locations.import.filter((x) => !!x.generated);

    const generated = {} as Record<string, ImportItem[]>;
    const generatedTypes = {} as Record<string, ImportItem[]>;

    for (let i = 0; i < generatedImports.length; i++) {
      const element = generatedImports[i];

      const o = element.asType ? generatedTypes : generated;
      const arr = o[element.from] ?? (o[element.from] = []);
      arr.push(...element.items);
    }

    // reduce generated imports
    // const generated = generatedImports
    //   .filter((x) => !!x.generated && !x.asType)
    //   .reduce((prev, cur) => {
    //     const arr = prev[cur.from] ?? (prev[cur.from] = []);
    //     arr.push(...cur.items);
    //     return prev;
    //   }, {} as Record<string, ImportItem[]>);

    // // reduce generated type only imports
    // const generatedTypes = generatedImports
    //   .filter((x) => !!x.generated && x.asType)
    //   .reduce((prev, cur) => {
    //     const arr = prev[cur.from] ?? (prev[cur.from] = []);
    //     arr.push(...cur.items);
    //     return prev;
    //   }, {} as Record<string, ImportItem[]>);

    const importsGenerated = Object.entries(generated).map(
      ([fromKey, items]) =>
        `import { ${items
          .map((x) =>
            (x.type ? "type" : "") + x.alias
              ? `${x.name} as ${x.alias}`
              : x.name
          )
          .join(", ")} } from '${fromKey}';`
    );

    const importsGeneratedTyped = Object.entries(generatedTypes).map(
      ([fromKey, items]) =>
        `import type { ${items
          .map((x) => (x.alias ? `${x.name} as ${x.alias}` : x.name))
          .join(", ")} } from '${fromKey}';`
    );

    const importsStr = [...importsGenerated, ...importsGeneratedTyped].join(
      "\n"
    );
    if (importsStr.trim()) {
      s.appendLeft(0, importsStr + "\n");
    }
  }

  // add the generated declarations
  {
    // "global" | "pre" | "post" | "end"

    function declarationToString(x: TypeLocationDeclaration): string {
      if (!x.declaration.name) {
        return x.declaration.content;
      }
      return [
        x.declaration.type ?? "const",
        x.declaration.name,
        "=",
        x.declaration.content,
      ].join(" ");
    }

    const generatedDeclarations = locations.declaration.filter(
      (x) => !!x.generated
    );

    const global = generatedDeclarations
      .filter((x) => x.context === "global" || !x.context)
      .map(declarationToString);

    const pre = generatedDeclarations
      .filter((x) => x.context === "pre")
      .map(declarationToString);

    const post = generatedDeclarations
      .filter((x) => x.context === "post")
      .map(declarationToString);

    const end = generatedDeclarations
      .filter((x) => x.context === "end")
      .map(declarationToString);

    const finish = generatedDeclarations
      .filter((x) => x.context === "finish")
      .map(declarationToString);

    // TODO add global

    if (global.length) {
      s.appendLeft(0, global.join("\n") + "\n");
    }

    if (pre.length) {
      s.prependLeft(startScriptIndex, pre.join("\n") + "\n");
    }

    const toRenderString = [
      ...post,
      ...end,
      "___VERTER___comp;",
      "___VERTER___ctx;",
      `return ___VERTER___PATCH_TYPE(___VERTER_COMP___, ___VERTER_PROPS___, {}, ___VERTER_EMITS___, ___VERTER___slot);`,
    ];

    // because of the way prepend works the rendering is reversed
    if (startScriptIndex === endScriptIndex) {
      s.prependRight(endScriptIndex, toRenderString.join("\n"));
    } else {
      s.prependRight(endScriptIndex, toRenderString.join("\n"));
    }

    s.prependRight(
      s.original.length,
      [
        ...finish,
        `export default __VERTER__RESULT${
          generic ? " as unknown as __VERTER_RESULT_TYPE__" : ""
        };`,
      ].join("\n")
    );
  }

  // s.prependLeft(0, "/* @jsxImportSource vue */\nimport 'vue/jsx';\n");

  // VoidElementTags = 'area' | 'base' | 'br' | 'col' | 'embed' | 'hr' | 'img' | 'input' | 'link' | 'meta' | 'source' | 'track' | 'wbr'

  const patchSlots = `
// patching elements
declare global {
  namespace JSX {
    export interface IntrinsicClassAttributes<T> {
      // $children: T extends { $slots: infer S } ? { default?: never } & { [K in keyof S]: S[K] extends (...args: infer Args) => any ? (...args: Args) => JSX.Element : () => JSX.Element } : { default?: never }
      // $children?: T extends { $slots: infer S } ? S : undefined
      'v-slot'?: (T extends { $slots: infer S } ? S : undefined) | ((c: T) => T extends { $slots: infer S } ? S : undefined)
    }
  }
}
declare module 'vue' {
  interface HTMLAttributes {
    'v-slot'?: {
      default: () => JSX.Element
    }
  }
}

declare function ___VERTER_renderSlot<T extends Array<any>>(slot: (...args: T) => any, cb: (...cb: T) => any): void;
declare function ___VERTER___template(): JSX.Element;

`;

  s.prependLeft(0, "import 'vue/jsx';\n" + patchSlots);

  // // fix imports
  // s.replaceAll(/\.vue(["'`])/gm, ".vue.tsx$1");

  return {
    locations,
    source: context.s.original,
    content: context.s.toString(),
    map: context.s.generateMap({ hires: true, includeContent: true }),

    context,
  };
}

// make sure all the locations are valid
function sanitiseMap(map: LocationByType) {
  map[LocationType.Props] ?? (map[LocationType.Props] = []);
  map[LocationType.Emits] ?? (map[LocationType.Emits] = []);
  map[LocationType.Slots] ?? (map[LocationType.Slots] = []);
  map[LocationType.Options] ?? (map[LocationType.Options] = []);
  map[LocationType.Model] ?? (map[LocationType.Model] = []);
  map[LocationType.Expose] ?? (map[LocationType.Expose] = []);
  map[LocationType.Template] ?? (map[LocationType.Template] = []);
  map[LocationType.Generic] ?? (map[LocationType.Generic] = []);
  map[LocationType.Declaration] ?? (map[LocationType.Declaration] = []);
  map[LocationType.Import] ?? (map[LocationType.Import] = []);
  map[LocationType.Export] ?? (map[LocationType.Export] = []);
}

function processBlock(
  block: VerterSFCBlock,
  context: ParseScriptContext,
  locations: LocationByType
) {
  const { s } = context;
  switch (block.tag.type) {
    case "script": {
      scriptTagProcess(block, context);
      const declarationStr = "const ____VERTER_COMP_OPTION__ = ";
      if (block.block.attrs.setup) {
        // locations.import.push({
        //   type: LocationType.Import,
        //   from: "vue",
        //   generated: true,
        //   node: null,
        //   items: [
        //     {
        //       name: "defineComponent",
        //       alias: "___VERTER_defineComponent",
        //       // note could be type
        //       type: false,
        //     },
        //   ],
        // });

        const scriptContent =
          context.script?.content ?? context.sfc.descriptor.scriptSetup.content;

        let content = scriptContent.slice(
          scriptContent.indexOf("const ____VERTER_COMP_OPTION__")
        );

        // no defineComponent, we need to append the defineComponent
        if (content.indexOf("_defineComponent(") === -1) {
          content =
            content.replace(
              `const ____VERTER_COMP_OPTION__ = {`,
              `const ____VERTER_COMP_OPTION__ = ___VERTER_defineComponent({`
            ) + ")";

          locations.import.push({
            type: LocationType.Import,
            from: "vue",
            generated: true,
            node: null,
            items: [
              {
                name: "defineComponent",
                alias: "___VERTER_defineComponent",
                // note could be type
                type: false,
              },
            ],
          });
        }

        locations.declaration.push({
          type: LocationType.Declaration,
          generated: true,
          context: "pre",
          declaration: {
            content,
            //   context.script.content.replace(
            //     "____VERTER_COMP_OPTION__ = ",
            //     "____VERTER_COMP_OPTION__ = ___VERTER_defineComponent("
            //   ) + ")",
          },
        });

        // add ___VERTER_COMP___ in declaration
        locations.declaration.push({
          type: LocationType.Declaration,
          generated: true,
          context: "post",
          declaration: {
            type: "const",
            name: "___VERTER_COMP___",
            content: `____VERTER_COMP_OPTION__`,
          },
        });
      } else {
        // if there's a setup and this block is not a setup, we just ignore it
        // there's no further work needed here
        if (context.isSetup && !block.block.attrs.setup) {
          return;
        }
        // options
        {
          // override the export default
          for (const possibleExport of possibleExports) {
            s.replace(possibleExport, declarationStr);
          }

          //   if (s.toString().indexOf(declarationStr) === -1) {
          //     s.appendLeft(block.block.loc.end.offset, `${declarationStr}{};`);
          //   }
          let wrap = false;
          if (s.toString().indexOf(declarationStr) === -1) {
            locations.declaration.push({
              type: LocationType.Declaration,
              generated: true,
              context: "pre",
              declaration: {
                type: "const",
                name: "____VERTER_COMP_OPTION__",
                content: `___VERTER_defineComponent({})`,
              },
            });

            locations.import.push({
              type: LocationType.Import,
              from: "vue",
              generated: true,
              node: null,
              items: [
                {
                  name: "defineComponent",
                  alias: "___VERTER_defineComponent",
                  // note could be type
                  type: false,
                },
              ],
            });
          } else {
            // check if there's defineComponent or is pure object
            const regexComp = /const ____VERTER_COMP_OPTION__ = [^\w]*{]*/gm;

            const rawObjectDeclaration = regexComp.test(s.toString());

            if (rawObjectDeclaration) {
              const defaultExport = context.script.scriptAst.find(
                (x) => x.type === "ExportDefaultDeclaration"
              );

              // if we find the defualt export wrap it with defineComponent
              // to allow intellisense
              if (defaultExport) {
                // Babel Nodes are not offset correctly
                const offset = block.tag.pos.open.end;
                const startOffset = defaultExport.loc.start.index + offset;
                const endOffset = defaultExport.loc.end.index + offset;

                const exportContent = s.original.slice(startOffset, endOffset);

                // find first {
                const openingIndex = exportContent.indexOf("{");

                // find last }
                const closeIndex = exportContent.lastIndexOf("}");

                s.prependRight(
                  openingIndex + startOffset,
                  "___VERTER_defineComponent("
                );

                s.prependRight(closeIndex + startOffset + 1, ")");

                locations.import.push({
                  type: LocationType.Import,
                  from: "vue",
                  generated: true,
                  node: null,
                  items: [
                    {
                      name: "defineComponent",
                      alias: "___VERTER_defineComponent",
                      // note could be type
                      type: false,
                    },
                  ],
                });
              } else {
                wrap = true;

                locations.import.push({
                  type: LocationType.Import,
                  from: "vue",
                  generated: true,
                  node: null,
                  items: [
                    {
                      name: "defineComponent",
                      alias: "___VERTER_defineComponent",
                      // note could be type
                      type: false,
                    },
                  ],
                });
              }
            }
          }

          // add ___VERTER_COMP___ in declaration
          locations.declaration.push({
            type: LocationType.Declaration,
            generated: true,
            context: "post",
            declaration: {
              type: "const",
              name: "___VERTER_COMP___",
              content: wrap
                ? `___VERTER_defineComponent(____VERTER_COMP_OPTION__)`
                : "____VERTER_COMP_OPTION__",
            },
          });
        }
      }

      return;
    }
    case "template": {
      // process template

      // override <template> to function ___VERTER__TEMPLATE_RENDER()
      s.overwrite(
        block.tag.pos.open.start,
        block.tag.pos.open.end,
        "function ___VERTER__TEMPLATE_RENDER() {\n<>"
      );

      // override </template>
      s.overwrite(
        block.tag.pos.close.start,
        block.tag.pos.close.end,
        "\n</>}\n___VERTER__TEMPLATE_RENDER();\n"
      );

      // if (context.template) {
      //     s.overwrite(templateStartOffset, context.template.loc.start.offset, `function ___VERTER__TEMPLATE_RENDER${generic ? `<${generic}>` : ''}() {\n<>`)

      //     s.overwrite(templateEndOffset, templateEndOffset + '</template>'.length, '\n</>\n}')

      //     s.appendRight(templateStartOffset, '\n\n/* verter:template-render start */\n')
      //     s.appendRight(templateEndOffset + '</template>'.length, '\n/* verter:template-render end */\n')
      // }

      // /process template

      return;
    }

    default: {
      throw new Error("Not recognised");
    }
  }
}

function clearTags(block: VerterSFCBlock, s: MagicString) {
  // clear opening tag
  s.remove(block.tag.pos.open.start, block.tag.pos.open.end);
  // clear closing tag
  s.remove(block.tag.pos.close.start, block.tag.pos.close.end);
}

function scriptTagProcess(block: VerterSFCBlock, context: ParseScriptContext) {
  if (context.isSetup && !block.block.attrs.setup) {
    clearTags(block, context.s);
    return;
  }

  const preGeneric = `\n${
    context.isAsync ? "async " : ""
  }function ___VERTER_COMPONENT__`;
  const postGeneric = `() {\n`;

  const generic = block.block.attrs.generic;
  // generic information will be kept intact for the source-map
  if (typeof generic === "string") {
    const tagContent = context.s.original.slice(
      block.tag.pos.open.start,
      block.tag.pos.open.end
    );

    const genericIndex = tagContent.indexOf(generic);

    // replace before generic with `preGeneric`
    context.s.overwrite(
      block.tag.pos.open.start,
      block.tag.pos.open.start + genericIndex,
      preGeneric + "<"
    );

    // replace after generic with `postGeneric`
    context.s.overwrite(
      block.tag.pos.open.start + genericIndex + generic.length,
      block.tag.pos.open.end,
      ">" + postGeneric
    );
  } else {
    context.s.overwrite(
      block.tag.pos.open.start,
      block.tag.pos.open.end,
      preGeneric + postGeneric
    );
  }

  context.s.overwrite(
    block.tag.pos.close.start,
    block.tag.pos.close.end,
    "\n}\n"
  );
}

function genericProcess(
  context: ParseScriptContext,
  locations: LocationByType
) {
  if (!context.generic) {
    return undefined;
  }
  const genericDeclaration = locations.generic[0];
  if (!genericDeclaration) return undefined;

  function getGenericComponentName(name: string) {
    return "_VUE_TS__" + name;
  }

  function replaceComponentNameUsage(name: string, content: string) {
    const regex = new RegExp(`\\b${name}\\b`, "g");
    return content.replace(regex, getGenericComponentName(name));
  }

  const genericNames = genericDeclaration.items.map((x) => x.name);
  const sanitisedNames = genericNames.map(sanitiseGenericNames);

  function sanitiseGenericNames(content: string | null | undefined) {
    if (!content) return content;
    return genericNames
      ? genericNames.reduce((prev, cur) => {
          return replaceComponentNameUsage(cur, prev);
        }, content)
      : content;
  }

  const CompGeneric = genericDeclaration.items
    .map((x) => {
      const name = getGenericComponentName(x.name);
      const constraint = sanitiseGenericNames(x.constraint);
      const defaultType = sanitiseGenericNames(x.default);

      return [
        name,
        constraint ? `extends ${constraint}` : undefined,
        `= ${defaultType || "any"}`,
      ]
        .filter(Boolean)
        .join(" ");
    })
    .join(", ");

  const InstanceGeneric = genericDeclaration.items
    .map((x) => {
      const name = x.name;
      const constraint = x.constraint || getGenericComponentName(x.name);
      const defaultType = x.default || getGenericComponentName(x.name);

      return [
        name,
        constraint ? `extends ${constraint}` : undefined,
        `= ${defaultType || "any"}`,
      ]
        .filter(Boolean)
        .join(" ");
    })
    .join(", ");

  return {
    /**
     * this is to be used for the external component
     */
    component: CompGeneric,
    /**
     * This is to be a direct replace from user declaration
     */
    instance: InstanceGeneric,

    genericNames,
    sanitisedNames,
  };
}
