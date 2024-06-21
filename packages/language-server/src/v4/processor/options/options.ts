import {
  LocationByType,
  ParseScriptContext,
  VerterSFCBlock,
  PrefixSTR,
} from "@verter/core";
import { parseSync, TsTypeAliasDeclaration } from "@swc/core";
import {
  isFunctionType,
  TS_NODE_TYPES,
  walkIdentifiers,
} from "@vue/compiler-core";
import { walk } from "vue/compiler-sfc";
import { parse as acornParse } from "acorn-loose";
import { offsetAt } from "../../../lib/documents/utils";
import { getBlockFilename } from "../utils";

export const OptionsExportName = PrefixSTR("ComponentOptions");
export const BindingContextExportName = PrefixSTR("BindingContext");
export const DefaultOptions = PrefixSTR("default");
export const ComponentExport = PrefixSTR("Component");
export const GenericOptions = PrefixSTR("GenericOptions");

let prevSize = 0;
function parseAst(content: string, isTypescript: boolean) {
  try {
    // for some reason each time parseSync is called the offset is not correct
    const offset = prevSize;
    prevSize += content.length + 1;

    const ast = parseSync(content, {
      syntax: isTypescript ? "typescript" : "ecmascript",
      target: "esnext",
      comments: false,
    });

    return {
      ast,
      offset,
    };
  } catch (e) {
    try {
      // try acorn loose and try
      const ast = acornParse(content, { ecmaVersion: "latest" });

      return {
        ast,
        offset: 0,
      };
    } catch {}

    console.error(e);
    return {
      ast: {
        body: [],
      },
      offset: prevSize,
    };
  }
}

export function processOptions(context: ParseScriptContext) {
  const filename = getBlockFilename("options", context);
  const s = context.s.clone();

  // remove unknown blocks
  const SUPPORTED_BLOCKS = new Set(["script"]);
  const blocks: VerterSFCBlock[] = [];
  for (const block of context.blocks) {
    if (SUPPORTED_BLOCKS.has(block.tag.type)) {
      blocks.push(block);
    } else {
      s.remove(block.tag.pos.open.start, block.tag.pos.close.end);
    }
  }

  if (context.script) {
    const isSetup = context.isSetup;
    const isGeneric = !!context.generic;
    const genericInfo = genericProcess(context);
    const isTypescript = context.script.lang?.startsWith("ts") ?? false;

    const mainBlock =
      blocks.find(
        (x) => x.tag.type === "script" && x.block.setup === isSetup
      ) ?? blocks.find((x) => x.tag.type === "script");

    const parsedAst = parseAst(mainBlock.block.content, isTypescript);
    let isAsync = false;

    const _bindings = new Set<string>();

    function populateBindings(node: any) {
      const InvalidParentsType = new Set([
        "CallExpression",
        "MemberExpression",
        "KeyValueProperty",
        "MethodProperty",
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

    populateBindings(parsedAst.ast);

    walk(parsedAst.ast, {
      enter(node) {
        if (isFunctionType(node)) {
          this.skip();
        }

        if (node.type === "AwaitExpression") {
          isAsync = true;
          this.skip();
        }
      },
    });
    // extra script blocks
    {
      for (const b of blocks) {
        if (b === mainBlock) continue;
        if (b.tag.type !== "script") continue;
        const { ast } = parseAst(b.block.content, isTypescript);
        populateBindings(ast);
      }
    }

    const bindings = Array.from(_bindings);
    // move imports and exports to top
    {
      for (const it of parsedAst.ast.body) {
        switch (it.type) {
          case "ExportDefaultExpression":
          case "ExportDefaultDeclaration": {
            // if is options and generic do not move
            if (!isSetup && isGeneric) continue;
          }
          case "ImportDeclaration":
          case "ExportAllDeclaration":
          case "ExportDeclaration":
          case "ExportNamedDeclaration":
          case "TsNamespaceExportDeclaration":
          case "TsExportAssignment": {
            const offset = mainBlock.block.loc.start.offset;

            const decrement = !!it.span ? 1 : 0;
            const pos = it.span ?? it;

            const startIndex =
              offset + (pos.start - parsedAst.offset) - decrement;
            const endIndex = offset + (pos.end - parsedAst.offset) - decrement;

            s.move(startIndex, endIndex, 0);
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
      s.remove(mainBlock.tag.pos.close.start, mainBlock.tag.pos.close.end);

      const generic = context.generic;
      // generic information will be kept intact for the source-map
      if (typeof generic === "string") {
        // get <script ... >
        const tagContent = s.original.slice(
          mainBlock.tag.pos.open.start,
          mainBlock.tag.pos.open.end
        );

        const genericIndex =
          tagContent.indexOf(generic) + mainBlock.tag.pos.open.start;

        // replace before generic with `preGeneric`
        s.overwrite(
          mainBlock.tag.pos.open.start,
          genericIndex,
          preGeneric + "<"
        );

        // replace after generic with `postGeneric`
        s.overwrite(
          genericIndex + generic.length,
          context.script.loc.start.offset,
          ">" + postGeneric
        );
      } else {
        s.overwrite(
          mainBlock.tag.pos.open.start,
          mainBlock.tag.pos.open.end,
          preGeneric + postGeneric
        );
      }

      const typeofBindings = bindings
        .map((x) => `${x}: typeof ${x}`)
        .join(", ");

      if (isTypescript) {
        s.appendRight(
          mainBlock.block.loc.end.offset,
          `\nreturn {} as {${typeofBindings}\n}\n`
        );
      } else {
        // if not typescript we need to the types in JSDoc to make sure the types are correct
        // because when the variables are const the types are not inferred correctly
        // e.g. const a = 1; a is inferred as number but it should be inferred as 1
        s.prependRight(
          mainBlock.tag.pos.open.start,
          `/**\n * @returns {{${typeofBindings}}} \n*/`
        );

        s.appendRight(
          mainBlock.block.loc.end.offset,
          `\nreturn {\n${bindings.join(", ")}\n}\n`
        );
      }

      s.appendRight(mainBlock.block.loc.end.offset, "\n}\n");
    } else {
      const exportIndex = s.original.indexOf("export default");

      if (isGeneric) {
        const preGeneric = `\nexport function ${BindingContextExportName}`;
        const postGeneric = `() {\n`;

        // remove close tag
        s.remove(mainBlock.tag.pos.close.start, mainBlock.tag.pos.close.end);

        const generic = context.generic;
        // generic information will be kept intact for the source-map
        if (typeof generic === "string") {
          // get <script ... >
          const tagContent = s.original.slice(
            mainBlock.tag.pos.open.start,
            mainBlock.tag.pos.open.end
          );

          const genericIndex =
            tagContent.indexOf(generic) + mainBlock.tag.pos.open.start;

          // replace before generic with `preGeneric`
          s.overwrite(
            mainBlock.tag.pos.open.start,
            genericIndex,
            preGeneric + "<"
          );

          // replace after generic with `postGeneric`
          s.overwrite(
            genericIndex + generic.length,
            context.script.loc.start.offset,
            ">" + postGeneric
          );
        } else {
          s.overwrite(
            mainBlock.tag.pos.open.start,
            mainBlock.tag.pos.open.end,
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
            mainBlock.block.loc.end.offset,
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
      if (block === mainBlock && (isSetup || isGeneric)) return;
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
      context.script.lang === "ts"
        ? "typescript"
        : context.script.lang === "tsx"
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

// TODO move somewhere else
export function genericProcess(context: ParseScriptContext) {
  if (!context.generic) {
    return undefined;
  }
  // const genericDeclaration = locations.generic[0];
  // if (!genericDeclaration) return undefined;
  const genericCode = `type __GENERIC__<${context.generic}> = {}`;

  const {
    ast: {
      body: [genericNode],
    },
    offset,
  } = parseAst(genericCode, true);

  // TODO handle if the generic is broken, maybe with REGEX
  if (!genericNode.typeParams) return undefined;

  const params = (genericNode as TsTypeAliasDeclaration).typeParams?.parameters;

  // const params =
  //   (ast.body[0] as TsTypeAliasDeclaration)?.typeParameters?.params ?? [];

  function retrieveNodeString(node: any, source: string) {
    if (!node) return undefined;
    return source.slice(
      node.span.start - offset - 1,
      node.span.end - offset - 1
    );
  }

  const items = params.map((param, index) => ({
    name: param.name.value,
    content: retrieveNodeString(param, genericCode),
    constraint: retrieveNodeString(param.constraint, genericCode),
    default: retrieveNodeString(param.default, genericCode),
    index,
  }));

  function getGenericComponentName(name: string) {
    return "_VUE_TS__" + name;
  }

  function replaceComponentNameUsage(name: string, content: string) {
    const regex = new RegExp(`\\b${name}\\b`, "g");
    return content.replace(regex, getGenericComponentName(name));
  }

  const genericNames = items.map((x) => x.name);
  const sanitisedNames = genericNames.map(sanitiseGenericNames);

  function sanitiseGenericNames(content: string | null | undefined) {
    if (!content) return content;
    return genericNames
      ? genericNames.reduce((prev, cur) => {
          return replaceComponentNameUsage(cur, prev);
        }, content)
      : content;
  }

  const CompGeneric = items
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

  const InstanceGeneric = items
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
