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
import {
  ModuleDeclaration,
  parseSync,
  Statement,
  TsTypeAliasDeclaration,
} from "@swc/core";
import {
  isFunctionType,
  TS_NODE_TYPES,
  walkIdentifiers,
} from "@vue/compiler-core";
import { walk } from "vue/compiler-sfc";
import { getBlockFilename, importsLocationsToString } from "../utils";

export const OptionsExportName = PrefixSTR("ComponentOptions");
export const BindingContextExportName = PrefixSTR("BindingContext");
export const DefaultOptions = PrefixSTR("default");
export const ComponentExport = PrefixSTR("Component");
export const GenericOptions = PrefixSTR("GenericOptions");

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

  if (scriptBlock) {
    const isSetup = context.isSetup;
    const isAsync = context.isAsync;
    const genericInfo = context.generic;
    const isTypescript = scriptBlock.block.lang?.startsWith("ts") ?? false;

    const _bindings = new Set<string>();

    function populateBindings(node: any) {
      const InvalidParentsType = new Set([
        "CallExpression",
        "MemberExpression",
        "ObjectExpression",
        "KeyValueProperty",
        "FormalParameters",
        "MethodProperty",
        "ExportDefaultDeclaration",
      ]);

      walk(node, {
        enter(node, parent) {
          if (InvalidParentsType.has(node.type)) {
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

          if (node.type === "Identifier") {
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

    // populateBindings(parsedAst.ast);

    // walk(parsedAst.ast, {
    //   enter(node) {
    //     if (isFunctionType(node)) {
    //       this.skip();
    //     }

    //     if (node.type === "AwaitExpression") {
    //       isAsync = true;
    //       this.skip();
    //     }
    //   },
    // });
    // extra script blocks
    {
      for (const b of blocks) {
        // if (b === scriptBlock) continue;
        if (!b.ast) continue;
        populateBindings(b.ast);
      }
    }

    const vueMacros = [
      ["defineProps", PrefixSTR("props")],
      ["withDefaults", PrefixSTR("props")],
      ["defineEmits", PrefixSTR("emits")],
      ["defineSlots", PrefixSTR("slots")],
      // TODO defineOptions
    ];
    const foundMacros = new Set<string>();
    // overrides the macro name with the variable name
    const macroOverride = new Map<string, string>();

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

    const bindings = Array.from(_bindings);
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

            for (const [macro, name] of vueMacros) {
              // @ts-expect-error
              const expresion = checkForSetupMethodCalls(macro, it);
              if (expresion) {
                foundMacros.add(macro);
                if (it.type === "VariableDeclaration") {
                  if (it.declarations[0].type === "VariableDeclarator") {
                    // @ts-expect-error
                    macroOverride.set(macro, it.declarations[0].id.name);
                  } else {
                    // this is probably a destructuring
                  }
                } else {
                  s.appendRight(
                    it.start + scriptBlock.block.loc.start.offset,
                    `const ${name} = `
                  );
                }
                break;
              }
              console.log("sss", expresion);
            }

            break;
            // vueMacros.forEach(([macro, prefix]) => {
            //   const expresion = checkForSetupMethodCalls(macro, it);
            //   if (expresion) {
            //     foundMacros.add(macro);
            //   }
            //   console.log("sss", expresion);
            // });
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

    // const extraBinding = Array.from(foundMacros.values())
    //   .map((x) => macroOverride.get(x) ?? vueMacros.find(([n]) => n === x)[1])
    //   .join(" & ");

    const propMacro =
      foundMacros.has("defineProps") || foundMacros.has("withDefaults");
    const propsBinding = propMacro
      ? `${
          macroOverride.get("defineProps") ?? macroOverride.get("withDefaults")
            ? `typeof ${
                macroOverride.get("defineProps") ??
                macroOverride.get("withDefaults")
              }`
            : "typeof " + vueMacros.find(([n]) => n === "defineProps")[1]
        }`
      : "";

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

      const typeofBindings = bindings
        .map((x) => `${x}: typeof ${x}`)
        .join(", ");

      if (isTypescript) {
        s.appendRight(
          scriptBlock.block.loc.end.offset,
          `\nreturn {} as {${typeofBindings}\n} ${
            propsBinding ? `& ${propsBinding}` : ""
          }\n`
        );
      } else {
        // if not typescript we need to the types in JSDoc to make sure the types are correct
        // because when the variables are const the types are not inferred correctly
        // e.g. const a = 1; a is inferred as number but it should be inferred as 1
        s.prependRight(
          scriptBlock.tag.pos.open.start,
          `/**\n * @returns {{${typeofBindings}}} \n*/`
        );

        s.appendRight(
          scriptBlock.block.loc.end.offset,
          `\nreturn {\n${bindings.join(", ")}\n}\n`
        );
      }

      s.appendRight(scriptBlock.block.loc.end.offset, "\n}\n");
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
          `\nexport function ${BindingContextExportName}() { return {} }\n`
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
        // @ts-expect-error
        declarationNode.start + defaultExportBlock.block.loc.start.offset;
      const end =
        // @ts-expect-error
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

  if (!scriptBlock.block.lang?.startsWith("ts")) {
    s.prepend("// @ts-nocheck\n");
  }

  // s.append("const ____VERTER_COMP_OPTION__COMPILED = defineComponent({})");

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
    content: s.toString(),
  };
}

export function checkForSetupMethodCalls(name: string, statement: Statement) {
  if (statement.type === "ExpressionStatement") {
    if (
      statement.expression.type === "CallExpression" &&
      "name" in statement.expression.callee &&
      // @ts-expect-error
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
        // @ts-expect-error
        declaration.init.callee.name === name
      ) {
        return declaration.init;
      }
    }
  }
  return null;
}
