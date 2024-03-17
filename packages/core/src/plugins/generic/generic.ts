import { TSTypeAliasDeclaration } from "@babel/types";
import { PluginOption, LocationType } from "../types.js";

import { parse } from "@babel/parser";
import { retrieveNodeString } from "../helpers.js";

export default {
  name: "Generic",
  process(context) {
    if (!context.generic) return;
    const genericCode = `type __GENERIC__<${context.generic}> = {}`;
    try {
      const ast = parse(genericCode, {
        sourceType: "module",
        plugins: ["typescript"],
      });

      const params =
        (ast.program.body[0] as TSTypeAliasDeclaration)?.typeParameters
          ?.params ?? [];

      const items = params.map((param, index) => ({
        name: param.name,
        content: retrieveNodeString(param, genericCode),
        constraint: retrieveNodeString(param.constraint, genericCode),
        default: retrieveNodeString(param.default, genericCode),
        index,
      }));

      return {
        type: LocationType.Generic,
        node: undefined,
        items,
      };
    } catch (e) {
      console.error("e");
      return;
    }
  },
} satisfies PluginOption;
