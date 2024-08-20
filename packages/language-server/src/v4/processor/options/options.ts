import {
  LocationByType,
  ParseScriptContext,
  VerterSFCBlock,
  PrefixSTR,
  ParseContext,
  VerterASTBlock,
  type TypeLocationImport,
  LocationType,
} from "@verter/core";

import type { Statement } from "@babel/types";
import {
  isFunctionType,
  TS_NODE_TYPES,
  walkIdentifiers,
} from "@vue/compiler-core";
import { walk } from "vue/compiler-sfc";
import { getBlockFilename, importsLocationsToString } from "../utils";

export const OptionsExportName = PrefixSTR("ComponentOptions");
export const BindingContextExportName = PrefixSTR("BindingContext");
export const FullContextExportName = PrefixSTR("FullContext");
export const DefaultOptions = PrefixSTR("default");
export const ComponentExport = PrefixSTR("Component");
export const GenericOptions = PrefixSTR("GenericOptions");

export const ResolveRenderProps = PrefixSTR("resolveRenderProps");
export const ResolveProps = PrefixSTR("resolveProps");
export const ResolveExtraProps = PrefixSTR("resolveExtraProps");
export const ResolveEmits = PrefixSTR("resolveEmits");
export const ResolveSlots = PrefixSTR("resolveSlots");
export const ResolveModels = PrefixSTR("resolveModels");

export const PropsPropertyName = PrefixSTR("props");
export const WithDefaultsPropsPropertyName = PrefixSTR("withDefaultsProps");
export const EmitsPropertyName = PrefixSTR("emits");
export const SlotsPropertyName = PrefixSTR("slots");
export const ModelsPropertyName = PrefixSTR("models");

// const DefineProps = PrefixSTR("DefineProps");
// const LooseRequired = PrefixSTR("LooseRequired");

const PartialUndefined = PrefixSTR("PartialUndefined");

export function processOptions(context: ParseContext) {
  const filename = getBlockFilename("options", context);
  const s = context.s.clone();

  const imports: TypeLocationImport[] = [];

  // remove unknown blocks
  const SUPPORTED_BLOCKS = new Set(["script"]);
  const blocks: VerterASTBlock[] = [];
  for (const block of context.blocks) {
    if (SUPPORTED_BLOCKS.has(block.tag.type)) {
      blocks.push(block);
    } else {
      s.remove(block.tag.pos.open.start, block.tag.pos.close.end);
    }
  }

  const scriptBlock =
    blocks.find(
      (x) => x.tag.type === "script" && x.block.setup === context.isSetup
    ) ?? blocks.find((x) => x.tag.type === "script");

  const isSetup = context.isSetup;
  const isAsync = context.isAsync;
  const genericInfo = context.generic;
  const isTypescript = scriptBlock.block.lang?.startsWith("ts") ?? false;

  const foundMacros = new Set<VueSetupMacros>();

  // overrides the macro name with the variable name
  const macroOverride = new Map<
    string,
    string | Array<string | [varName: string, modelName: string]>
  >();

  const macroVariableName = new Map<VueSetupMacros, string>();

  // @ts-expect-error
  context.foundMacros = foundMacros;
  // @ts-expect-error
  context.macroOverride = macroOverride;
  // @ts-expect-error
  context.macroVariableName = macroVariableName;

  if (scriptBlock) {
    const _bindings = new Set<string>();
    /**
     * contains all the declarations used
     */
    const _declarationBindings: string[] = [];

    const _importBindings: string[] = [];

    function populateBindings(node: any, source = "") {
      const InvalidParentsType = new Set([
        "CallExpression",
        "MemberExpression",
        "ObjectExpression",
        "KeyValueProperty",
        "FormalParameters",
        "MethodProperty",
        "ExportDefaultDeclaration",
        "FunctionBody",
        "BlockStatement",
        "ForStatement",
      ]);

      const declarationTypes = new Set([
        "VariableDeclaration",
        "FunctionDeclaration",
        "ClassDeclaration",
        "TsTypeAliasDeclaration",
        "ExpressionStatement",
      ]);

      const importTypes = new Set([
        "ImportSpecifier",
        "ImportDefaultSpecifier",
        "ImportNamespaceSpecifier",
      ]);

      walk(node, {
        enter(node, parent) {
          if (
            InvalidParentsType.has(node.type) ||
            node.type.endsWith("Statement")
          ) {
            this.skip();
            return;
          }
          if (
            parent &&
            parent.type.startsWith("TS") &&
            !TS_NODE_TYPES.includes(parent.type)
          ) {
            return this.skip();
          }

          if (declarationTypes.has(node.type)) {
            _declarationBindings.push(source.slice(node.start, node.end));
          } else if (importTypes.has(node.type)) {
            _importBindings.push(source.slice(node.start, node.end));
          } else if (node.type === "Identifier") {
            const pos = node.span ?? node;
            // invalid node, skip
            if (pos.start === pos.end) {
              return;
            }
            const name = (node as any).value ?? node.name;
            _bindings.add(name);
          }
        },
      });
    }

    // extra script blocks
    {
      for (const b of blocks) {
        if (!b.ast) continue;
        populateBindings(b.ast, b.block.loc.source);
      }
    }

    const vueMacros: Array<
      [macro: VueSetupMacros, overrideName: string, multiple?: boolean]
    > = [
      ["defineProps", PropsPropertyName],
      // ["withDefaults", PropsPropertyName],
      ["defineEmits", EmitsPropertyName],
      ["defineSlots", SlotsPropertyName],

      ["defineModel", ModelsPropertyName, true],
      // TODO defineOptions
    ];

    let defaultExportNode: Statement | null = null;
    let hasDefaultExportObject = false;
    let defaultExportBlock: VerterASTBlock | null = null;

    // TODO fix node type
    function processDefaultExport(node: any, block: VerterASTBlock) {
      switch (node.type as any) {
        case "ExportDefaultExpression":
        case "ExportDefaultDeclaration": {
          defaultExportBlock = block;
          defaultExportNode = node;
          if (node.declaration.type === "ObjectExpression") {
            hasDefaultExportObject = true;
          }
        }
      }
    }

    // move imports and exports to top
    if (scriptBlock.ast) {
      for (const it of scriptBlock.ast.body) {
        switch (it.type as any) {
          case "VariableDeclaration":
          case "ExpressionStatement": {
            if (!isSetup) {
              continue;
            }
            // check if is any defineMacros

            for (const [macro, verterName, multiple] of vueMacros) {
              // @ts-expect-error
              const expresion = checkForSetupMethodCalls(macro, it);
              if (expresion) {
                foundMacros.add(macro);
                if (it.type === "VariableDeclaration") {
                  if (it.declarations[0].type === "VariableDeclarator") {
                    // @ts-expect-error
                    const name = it.declarations[0].id.name;
                    const modelName =
                      macro === "defineModel"
                        ? // @ts-expect-error
                          it.declarations[0].init.arguments[0]?.type ===
                          "StringLiteral"
                          ? // @ts-expect-error
                            it.declarations[0].init.arguments[0].value
                          : undefined
                        : undefined;

                    if (multiple) {
                      let l = macroOverride.get(macro);
                      if (!l || !Array.isArray(l)) {
                        l = [];
                        macroOverride.set(macro, l);
                      }
                      if (modelName) {
                        l.push([name, modelName]);
                      } else {
                        l.push(name);
                      }

                      s.appendLeft(
                        it.declarations[0].init.start +
                          scriptBlock.block.loc.start.offset,
                        verterName + " = "
                      );
                    } else {
                      s.appendLeft(
                        it.declarations[0].init.start +
                          scriptBlock.block.loc.start.offset,
                        verterName + " = "
                      );
                      macroOverride.set(macro, verterName);

                      macroVariableName.set(macro, name);
                    }
                  } else {
                    // this is probably a destructuring
                  }
                  // append ___VERTER__ variable before the definition
                  /*
                    eg: const props = defineProps({})

                    res: let ___VERTER__props;
                         const props = ___VERTER__props = defineProps({})
                  */

                  s.appendRight(
                    it.start + scriptBlock.block.loc.start.offset,
                    `let ${verterName};\n`
                  );

                  _declarationBindings.push(
                    scriptBlock.block.loc.source.slice(it.start, it.end)
                  );
                } else {
                  // resolves the varName
                  const varName = multiple
                    ? `${verterName}_${
                        // @ts-expect-error
                        it.declarations?.[0]?.init.arguments[0]?.type ===
                        "StringLiteral" // @ts-expect-error
                          ? it.declarations[0].init.arguments[0].value
                          : "modelValue"
                      }`
                    : verterName;

                  s.appendRight(
                    it.start + scriptBlock.block.loc.start.offset,
                    `const ${varName} = `
                  );

                  _declarationBindings.push(
                    `const ${varName} = ` +
                      scriptBlock.block.loc.source.slice(it.start, it.end)
                  );

                  // expose as model
                  if (multiple) {
                    if (multiple) {
                      let l = macroOverride.get(macro);
                      if (!l || !Array.isArray(l)) {
                        l = [];
                        macroOverride.set(macro, l);
                      }
                      l.push(varName);
                    } else {
                      macroOverride.set(macro, varName);
                    }
                  }
                }
                break;
              }
            }

            const withDefaultsExpresion = checkForSetupMethodCalls(
              "withDefaults",
              it as Statement
            );
            if (withDefaultsExpresion) {
              foundMacros.add("withDefaults");

              const offset = scriptBlock.block.loc.start.offset;

              // create variables before the possible declaration
              s.appendRight(
                it.start + offset,
                `let ${[PropsPropertyName, WithDefaultsPropsPropertyName].join(
                  ","
                )};\n`
              );

              s.appendRight(
                withDefaultsExpresion.start + offset,
                `${WithDefaultsPropsPropertyName} = `
              );

              const definePropsExpression = withDefaultsExpresion.arguments[0];
              if (
                definePropsExpression.type === "CallExpression" &&
                definePropsExpression.callee.type === "Identifier" &&
                definePropsExpression.callee.name === "defineProps"
              ) {
                foundMacros.add("defineProps");
                s.appendRight(
                  definePropsExpression.start + offset,
                  `${PropsPropertyName} = `
                );
              }

              if (it.type === "VariableDeclaration") {
                if (
                  it.declarations[0].type === "VariableDeclarator" &&
                  it.declarations[0].id.type === "Identifier"
                ) {
                  const name = it.declarations[0].id.name;
                  macroOverride.set("withDefaults", name);
                }
              }

              // if (expresion.type === "VariableDeclaration") {
              //   if (expresion.declarations[0].type === "VariableDeclarator") {
              //   } else {
              //     // this is probably a destructuring
              //   }

              //   _declarationBindings.push(
              //     scriptBlock.block.loc.source.slice(expresion.start, expresion.end)
              //   );
              // } else {

              // }
            }

            break;
          }
          case "ExportDefaultExpression":
          case "ExportDefaultDeclaration": {
            processDefaultExport(it, scriptBlock);
            // if is options and generic do not move
            if (!isSetup && genericInfo) continue;
          }
          case "ImportDeclaration":
          case "ExportAllDeclaration":
          case "ExportDeclaration":
          case "ExportNamedDeclaration":
          case "TsNamespaceExportDeclaration":
          case "TsExportAssignment": {
            const offset = scriptBlock.block.loc.start.offset;
            const startIndex = it.start + offset;
            const endIndex = it.end + offset;
            try {
              s.move(startIndex, endIndex, 0);
            } catch (e) {
              console.error(e);
              debugger;
            }
            break;
          }
        }
      }
    }

    const propsBinding = foundMacros.has("defineProps")
      ? setupMacroReturn(
          "defineProps",
          (macroOverride.get("defineProps") as string) ??
            vueMacros.find(([n]) => n === "defineProps")[1]
        )
      : "";

    const withDefaultsBinding = foundMacros.has("withDefaults")
      ? setupMacroReturn(
          "withDefaults",
          (macroOverride.get("withDefaults") as string) ??
            WithDefaultsPropsPropertyName
        )
      : "";

    const emitBinding = foundMacros.has("defineEmits")
      ? setupMacroReturn(
          "defineEmits",
          (macroOverride.get("defineEmits") as string) ??
            vueMacros.find(([n]) => n === "defineEmits")[1]
        )
      : "";

    const modelsBinding = foundMacros.has("defineModel")
      ? setupMacroReturn(
          "defineModel",
          (macroOverride.get("defineModel") as string) ??
            vueMacros.find(([n]) => n === "defineModel")[1]
        )
      : "";

    // const slotsBinding = foundMacros.has("defineSlots")
    //   ? setupMacroReturn(
    //       "defineSlots",
    //       macroOverride.get("defineSlots") ??
    //         vueMacros.find(([n]) => n === "defineSlots")[1]
    //     )
    //   : "";

    // do work for <script ... >
    if (isSetup) {
      const preGeneric = `\nexport ${
        isAsync ? "async " : ""
      }function ${BindingContextExportName}`;
      const postGeneric = `() {\n`;

      // remove close tag
      s.remove(scriptBlock.tag.pos.close.start, scriptBlock.tag.pos.close.end);

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

      const templateIdentifiers = new Set(
        context.templateIdentifiers.map((x) => x.content)
      );
      const bindings = Array.from(_bindings).filter((x) =>
        templateIdentifiers.has(x)
      );
      const typeofBindings = bindings
        .map((x) => `${x}: typeof ${x}`)
        .join(", ");

      if (isTypescript) {
        const extraBindings = [
          propsBinding && `{ ${PropsPropertyName}: ${propsBinding} }`,
          withDefaultsBinding &&
            `{ ${WithDefaultsPropsPropertyName}: ${withDefaultsBinding} }`,
          emitBinding && `{ ${EmitsPropertyName}: ${emitBinding} }`,
          modelsBinding && `{ ${ModelsPropertyName}: ${modelsBinding} }`,
        ].filter(Boolean);
        s.appendRight(
          scriptBlock.block.loc.end.offset,
          `\nreturn /*##___VERTER_BINDING_RETURN___##*/{} as {${typeofBindings}\n}${
            extraBindings.length > 0 ? " & " + extraBindings.join(" & ") : ""
          } /*/##___VERTER_BINDING_RETURN___##*/\n`
        );
      } else {
        const extraBindings = [propsBinding, emitBinding].filter(Boolean);

        // if not typescript we need to the types in JSDoc to make sure the types are correct
        // because when the variables are const the types are not inferred correctly
        // e.g. const a = 1; a is inferred as number but it should be inferred as 1
        s.prependRight(
          scriptBlock.tag.pos.open.start,
          `/**\n * @returns {{${typeofBindings}}${
            extraBindings.length > 0 ? `& ${extraBindings.join(" & ")}` : ""
          }} \n*/`
        );

        s.appendRight(
          scriptBlock.block.loc.end.offset,
          `\nreturn /*##___VERTER_BINDING_RETURN___##*/{\n${bindings.join(
            ", "
          )}${
            extraBindings.length > 0
              ? ",\n..." +
                extraBindings
                  .map((x) => x.replace("typeof ", ""))
                  .join(",\n...")
              : ""
          }\n}/*/##___VERTER_BINDING_RETURN___##*/\n`
        );
      }

      s.appendRight(scriptBlock.block.loc.end.offset, "\n}\n");

      // const propType = PropsPropertyName + " : " + (propsBinding || "{}");
      // const emitType = EmitsPropertyName + " : " + (emitBinding || "{}");
      // const slotsType =
      //   SlotsPropertyName +
      //   " : " +
      //   (setupBindingReturn(
      //     macroOverride.get("defineSlots") ??
      //       vueMacros.find(([n]) => n === "defineSlots")[1]
      //   ) || "{}");

      // const extraTypes = [propType, emitType, slotsType].filter(Boolean);

      if (isTypescript) {
        s.append(
          `export ${isAsync ? "async" : ""} function ${FullContextExportName}${
            genericInfo ? `<${genericInfo.source}>` : ""
          }() {
 ${_declarationBindings.join("\n")}\n
 return /*##___VERTER_FULL_BINDING_RETURN___##*/{} as {${Array.from(_bindings)
   .filter((x) => !_importBindings.includes(x))
   .map((x) => `${x}: typeof ${x}`)
   .join(", ")}\n}/*##/___VERTER_FULL_BINDING_RETURN___##*/ }\n`
        );
      } else {
        s.append(`\n/**\n * @returns {{${Array.from(_bindings)
          .filter((x) => !_importBindings.includes(x))
          .map((x) => `${x}: typeof ${x}`)
          .join(", ")}}} \n*/\nexport ${
          isAsync ? "async" : ""
        } function ${FullContextExportName}(){
  ${_declarationBindings.join("\n")}\n
  return /*##___VERTER_FULL_BINDING_RETURN___##*/{\n${Array.from(_bindings)
    .filter((x) => !_importBindings.includes(x))
    .join(", ")}${
          // TODO ADD OTHER MACROS
          ", " +
          PropsPropertyName +
          ":" +
          (propsBinding ?? "").replace("typeof", "")
        }\n}/*/##___VERTER_FULL_BINDING_RETURN___##*/ }\n`);
      }
    } else {
      const exportIndex = s.original.indexOf("export default");

      if (genericInfo) {
        const preGeneric = `\nexport function ${BindingContextExportName}`;
        const postGeneric = `() {\n`;

        // remove close tag
        s.remove(
          scriptBlock.tag.pos.close.start,
          scriptBlock.tag.pos.close.end
        );

        const generic = context.generic?.source;
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
            genericIndex + genericInfo.source.length,
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
        s.append("\n}");

        s.update(exportIndex, exportIndex + "export default".length, `return`);
      } else {
        if (exportIndex >= 0) {
          s.update(
            exportIndex,
            exportIndex + "export default".length,
            `export const ${OptionsExportName} =`
          );
        } else {
          s.appendRight(
            scriptBlock.block.loc.end.offset,
            `\nexport const ${OptionsExportName} = {}\n`
          );
        }

        s.append(
          `\nexport function ${BindingContextExportName}() { return /*##___VERTER_BINDING_RETURN___##*/{}/*##/___VERTER_BINDING_RETURN___##*/ }\n
export function ${FullContextExportName}() { return /*##___VERTER_FULL_BINDING_RETURN___##*/{}/*##/___VERTER_FULL_BINDING_RETURN___##*/ }\n`
        );
      }
    }

    // remove block tags
    blocks.forEach((block) => {
      // check if the block was already handled
      if (block === scriptBlock && (isSetup || genericInfo)) return;
      s.remove(block.tag.pos.open.start, block.tag.pos.open.end);

      s.remove(block.tag.pos.close.start, block.tag.pos.close.end);

      const defaultExport = block.block.loc.source.indexOf("export default");
      if (defaultExport >= 0) {
        const start = block.block.loc.start.offset + defaultExport;
        s.update(
          start,
          start + "export default".length,
          `export const ${DefaultOptions} =`
        );
        if (block.ast) {
          block.ast.body.forEach((it) => processDefaultExport(it, block));
        }
      }
    });

    if (hasDefaultExportObject || !defaultExportNode) {
      imports.push({
        type: LocationType.Import,
        from: "vue",
        items: [
          {
            name: "defineComponent",
            alias: PrefixSTR("defineComponent"),
          },
        ],
      });
    }
    // wrap the export default in defineComponent
    if (hasDefaultExportObject) {
      // @ts-expect-error not the correct type
      const declarationNode = defaultExportNode.declaration as Statement;
      const start =
        declarationNode.start + defaultExportBlock.block.loc.start.offset;
      const end =
        declarationNode.end + defaultExportBlock.block.loc.start.offset;

      s.prependLeft(start, `${PrefixSTR("defineComponent")}(`);
      s.prependRight(end, `)`);
    } else if (!defaultExportNode) {
      s.append(
        `export const ${DefaultOptions} = ${PrefixSTR(
          "defineComponent"
        )}({});\n`
      );
    }
  }

  const importsString = importsLocationsToString(imports);
  if (importsString) {
    s.prepend(importsString + "\n");
  }
  const generic = genericInfo ? `<${genericInfo.names.join(",")}>` : "";

  const resolveExports = [
    [
      foundMacros.has("withDefaults") || foundMacros.has("defineProps"),
      ResolveRenderProps,
      foundMacros.has("withDefaults")
        ? WithDefaultsPropsPropertyName
        : PropsPropertyName,
      "$props",
    ],
    [
      foundMacros.has("withDefaults") || foundMacros.has("defineProps"),
      ResolveProps,
      foundMacros.has("defineProps")
        ? PropsPropertyName
        : WithDefaultsPropsPropertyName,
      "$props",
    ],
    [
      foundMacros.has("defineEmits"),
      ResolveEmits,
      EmitsPropertyName,
      "$emit",
      `UnionToIntersection<ModelToEmits<ReturnType<typeof ${ResolveModels}${generic}>>>`,
    ],
    [foundMacros.has("defineSlots"), ResolveSlots, SlotsPropertyName, "$slots"],
    [
      foundMacros.has("defineModel"),
      ResolveModels,
      ModelsPropertyName,
      undefined,
    ],
  ] as Array<
    [
      hasDeclaration: boolean,
      resolveName: string,
      propertyName: string,
      optionsAccessor: string,
      extraBindings: string | undefined
    ]
  >;

  for (const [
    hasDeclaration,
    resolveName,
    propertyName,
    optionsAccessor,
    extraBindings,
  ] of resolveExports) {
    s.append(`\nexport declare ${
      isAsync ? "async " : ""
    }function ${resolveName}${
      genericInfo ? `<${genericInfo.source}>` : ""
    }(): ${extraBindings ? "(" : ""} ${
      isSetup
        ? hasDeclaration
          ? `ReturnType<typeof ${BindingContextExportName}${generic}>${
              isAsync
                ? ` extends Promise<{ ${propertyName}: infer P } ? P : {}`
                : `['${propertyName}']`
            }`
          : `{}`
        : // check if has options accessor
        optionsAccessor
        ? `InstanceType<typeof DefaultOptions>['${optionsAccessor}']`
        : ""
    } ${extraBindings ? `) & ${extraBindings}` : ""}
  `);
  }
  // ResolveExtraProps
  s.append(
    `\nexport declare ${isAsync ? "async " : ""}function ${ResolveExtraProps}${
      genericInfo ? `<${genericInfo.source}>` : ""
    }(): ModelToProps<ReturnType<typeof ${ResolveModels}${generic}>>
    & UnionToIntersection<EmitMapToProps<OverloadParameters<ReturnType<typeof ${ResolveEmits}${generic}>>>>;`
  );

  // TODO append ___VERTER___ to prevent types from leaking
  s.append(`
/**
 * Utility for extracting the parameters from a function overload (for typed emits)
 * https://github.com/microsoft/TypeScript/issues/32164#issuecomment-1146737709
 */
export type OverloadParameters<T extends (...args: any[]) => any> = Parameters<
    OverloadUnion<T>
>

type OverloadProps<TOverload> = Pick<TOverload, keyof TOverload>

type OverloadUnionRecursive<
    TOverload,
    TPartialOverload = unknown,
> = TOverload extends (...args: infer TArgs) => infer TReturn
    ? TPartialOverload extends TOverload
    ? never
    :
    | OverloadUnionRecursive<
        TPartialOverload & TOverload,
        TPartialOverload &
        ((...args: TArgs) => TReturn) &
        OverloadProps<TOverload>
    >
    | ((...args: TArgs) => TReturn)
    : never

type OverloadUnion<TOverload extends (...args: any[]) => any> = Exclude<
    OverloadUnionRecursive<(() => never) & TOverload>,
    TOverload extends () => never ? never : () => never
>

export type UnionToIntersection<U> = (
    U extends any ? (k: U) => void : never
) extends (k: infer I) => void
    ? I
    : never
    
    
type EmitMapToProps<T> = T extends [
    infer E extends string,
    ...infer A
]
    ? { [K in \`on\${Capitalize<E>}\`]?: (...args: A) => void }
    : never;

type ModelToProps<T> = {
  [K in keyof T]: T[K] extends ModelRef<infer C> ? C : T[K] extends ModelRef<infer C> | undefined ? C | undefined : null
}

type ModelToEmits<T> = {} extends T ? () => any : {
    [K in keyof T]-?: (event: \`update:\${K & string}\`, arg: T[K] extends ModelRef<infer C> ? C : T[K] extends ModelRef<infer C> | undefined ? C | undefined : unknown) => any
}[keyof T]

type MakeOptionalIfUndefined<T> = {
    [K in keyof T as undefined extends T[K] ? K : never]?: T[K];
} extends infer Optional ? Omit<T, keyof Optional> & Optional
    : {}

import { ModelRef } from 'vue'
    `);

  //   s.append(`\nexport ${isAsync ? "async " : ""}function ${ResolveProps}${
  //     genericInfo ? `<${genericInfo.source}>` : ""
  //   }() {
  //    return ${
  //      isSetup
  //        ? `(${isAsync ? "await " : ""}${FullContextExportName}${
  //            genericInfo ? `<${genericInfo.names.join(",")}>` : ""
  //          }()).${PropsPropertyName}`
  //        : `new ${DefaultOptions}().$props`
  //    };
  // }
  // `);

  // if (!scriptBlock.block.lang?.startsWith("ts")) {
  //   s.prepend("// @ts-nocheck\n");
  // }

  // s.append("const ____VERTER_COMP_OPTION__COMPILED = defineComponent({})");

  const content = s.toString();

  return {
    languageId:
      scriptBlock.block.lang === "ts"
        ? "typescript"
        : scriptBlock.block.lang === "tsx"
        ? "tsx"
        : "javascript",
    filename,

    loc: {
      source: s.original,
    },

    s,
    content,
    bindingReturn: {
      start: content.indexOf("/*##___VERTER_BINDING_RETURN___##*/"),
      end: content.indexOf("/*/##___VERTER_BINDING_RETURN___##*/"),
    },
  };
}

export function checkForSetupMethodCalls(name: string, statement: Statement) {
  if (statement.type === "ExpressionStatement") {
    if (
      statement.expression.type === "CallExpression" &&
      "name" in statement.expression.callee &&
      statement.expression.callee.name === name
    ) {
      return statement.expression;
    }
  } else if (
    statement.type === "VariableDeclaration" &&
    statement.declarations &&
    statement.declarations.length
  ) {
    for (let d = 0; d < statement.declarations.length; d++) {
      const declaration = statement.declarations[d];
      if (
        declaration?.init?.type === "CallExpression" &&
        "name" in declaration.init.callee &&
        declaration.init.callee.name === name
      ) {
        return declaration.init;
      }
    }
  }
  return null;
}

export type VueSetupMacros =
  | "defineProps"
  | "withDefaults"
  | "defineEmits"
  | "defineSlots"
  | "defineOptions"
  | "defineModel";

function setupBindingReturn(name: string) {
  if (!name) return "";
  return `typeof ${name}`;
}

function setupMacroReturn(
  macro: VueSetupMacros,
  name: string | Array<string | [varName: string, modelName: string]>
) {
  switch (macro) {
    case "defineProps":
    case "withDefaults":
      return `typeof ${name}`;
    case "defineEmits":
      return `typeof ${name}`;
    case "defineSlots":
      return `typeof ${name}`;

    case "defineModel":
      if (!Array.isArray(name)) {
        return `{ }`;
      }

      return `{ ${name.map((x) =>
        Array.isArray(x)
          ? `${x[1]}?: typeof ${x[0]}`
          : `modelValue?: typeof ${x}`
      )} }`;
    case "defineOptions":
      // TODO implement, not sure what to return here
      return "";
    // return `typeof ${name}`;
  }
}
