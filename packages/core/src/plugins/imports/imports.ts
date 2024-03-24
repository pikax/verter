import { PluginOption, LocationType } from "../types.js";

import { retrieveNodeString } from "../helpers.js";
import babel_types from "@babel/types";

export default {
  name: "Imports",
  walk(node, context) {
    const source = context.script?.loc.source;
    if (!source) return;

    const supportedTypes = new Set(["ImportDeclaration"] as Array<
      babel_types.Node["type"]
    >);

    if (!supportedTypes.has(node.type)) return;
    const content = retrieveNodeString(node, source);

    return {
      type: LocationType.Import,
      offset: context.script.loc.start.offset,

      generated: false,
      node,
      content: content,
    };
  },

  //   process(context) {
  //     if (!context.script) return;

  //     // const genericCode = `type __GENERIC__<${context.generic}> = {}`;
  //     // try {
  //     //   const ast = parse(genericCode, {
  //     //     sourceType: "module",
  //     //     plugins: ["typescript"],
  //     //   });

  //     //   const params =
  //     //     (ast.program.body[0] as TSTypeAliasDeclaration)?.typeParameters
  //     //       ?.params ?? [];

  //     //   const items = params.map((param, index) => ({
  //     //     name: param.name,
  //     //     content: retrieveNodeString(param, genericCode),
  //     //     constraint: retrieveNodeString(param.constraint, genericCode),
  //     //     default: retrieveNodeString(param.default, genericCode),
  //     //     index,
  //     //   }));

  //     //   return {
  //     //     type: LocationType.Generic,
  //     //     node: undefined,
  //     //     items,
  //     //   };
  //     // } catch (e) {
  //     //   console.error("e");
  //     //   return;
  //     // }
  //   },
} satisfies PluginOption;
