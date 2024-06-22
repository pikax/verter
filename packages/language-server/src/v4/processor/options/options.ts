import {
  LocationByType,
  ParseScriptContext,
  VerterSFCBlock,
  PrefixSTR,
  ParseContext,
  VerterASTBlock,
} from "@verter/core";
import { parseSync, TsTypeAliasDeclaration } from "@swc/core";
import {
  isFunctionType,
  TS_NODE_TYPES,
  walkIdentifiers,
} from "@vue/compiler-core";
import { walk } from "vue/compiler-sfc";
import { getBlockFilename } from "../utils";

export const OptionsExportName = PrefixSTR("ComponentOptions");
export const BindingContextExportName = PrefixSTR("BindingContext");
export const DefaultOptions = PrefixSTR("default");
export const ComponentExport = PrefixSTR("Component");
export const GenericOptions = PrefixSTR("GenericOptions");

export function processOptions(context: ParseContext) {
  const filename = getBlockFilename("options", context);
  const s = context.s.clone();

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
        "KeyValueProperty",
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

    const bindings = Array.from(_bindings);
    // move imports and exports to top
    if (scriptBlock.ast) {
      for (const it of scriptBlock.ast.body) {
        switch (it.type as any) {
          case "ExportDefaultExpression":
          case "ExportDefaultDeclaration": {
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
          `\nreturn {} as {${typeofBindings}\n}\n`
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
          `const ${DefaultOptions} =`
        );
      } else {
        s.prependRight(
          block.block.loc.start.offset,
          `const ${DefaultOptions} = {}\n`
        );
      }
    });
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
